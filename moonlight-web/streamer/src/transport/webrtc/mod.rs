use std::{
    future::ready,
    pin::Pin,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use bytes::Bytes;
use common::{
    StreamSettings,
    api_bindings::{
        RtcIceCandidate, RtcSdpType, RtcSessionDescription, StreamClientMessage,
        StreamServerMessage, StreamSignalingMessage, TransportChannelId,
    },
    config::{PortRange, WebRtcConfig},
    ipc::{ServerIpcMessage, StreamerIpcMessage},
};
use log::{debug, error, info, warn};
use moonlight_common::stream::{
    bindings::{AudioConfig, DecodeResult, OpusMultistreamConfig, VideoDecodeUnit},
    video::VideoSetup,
};
use tokio::{
    runtime::Handle,
    spawn,
    sync::{
        Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::sleep,
};
use webrtc::{
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
        setting_engine::SettingEngine,
    },
    data_channel::{RTCDataChannel, data_channel_init::RTCDataChannelInit, data_channel_message::DataChannelMessage},
    ice::udp_network::{EphemeralUDP, UDPNetwork},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_connection_state::RTCIceConnectionState,
    },
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection,
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::{sdp_type::RTCSdpType, session_description::RTCSessionDescription},
    },
};

use crate::{
    convert::{
        from_webrtc_sdp, into_webrtc_ice, into_webrtc_ice_candidate, into_webrtc_network_type,
    },
    transport::{
        InboundPacket, OutboundPacket, TransportChannel, TransportError, TransportEvent,
        TransportEvents, TransportSender,
        webrtc::{
            audio::{WebRtcAudio, register_audio_codecs},
            video::{WebRtcVideo, register_video_codecs},
        },
    },
};

pub const TIMEOUT_DURATION: Duration = Duration::from_secs(10);

mod audio;
mod sender;
mod video;

struct WebRtcInner {
    peer: Arc<RTCPeerConnection>,
    stream_settings: StreamSettings,
    event_sender: Sender<TransportEvent>,
    general_channel: Arc<RTCDataChannel>,
    stats_channel: Mutex<Option<Arc<RTCDataChannel>>>,
    // TODO: use negotiated channels -> no rwlock required
    video: Mutex<WebRtcVideo>,
    audio: Mutex<WebRtcAudio>,
    // Timeout / Terminate
    pub timeout_terminate_request: Mutex<Option<Instant>>,
    // Input-only peer connection for hybrid mode
    input_peer: Mutex<Option<Arc<RTCPeerConnection>>>,
    // Stats channel on input peer (preferred in hybrid mode)
    input_stats_channel: Mutex<Option<Arc<RTCDataChannel>>>,
    // Store config for creating input peer
    rtc_config: RTCConfiguration,
    // Track last keepalive response time (for client-created keepalive channel)
    last_keepalive_response: Mutex<Instant>,
    // Track pings sent without pong response (for asymmetric connectivity detection)
    keepalive_pings_without_pong: Mutex<u32>,
    // Offer batching - delay offer to allow multiple track additions to be batched
    offer_batch_deadline: Arc<Mutex<Option<Instant>>>,
    offer_batch_task_running: Arc<Mutex<bool>>,
}

pub async fn new(
    stream_settings: StreamSettings,
    config: &WebRtcConfig,
    session_token: Option<String>,
) -> Result<(WebRTCTransportSender, WebRTCTransportEvents), anyhow::Error> {
    // -- Configure WebRTC
    let rtc_config = RTCConfiguration {
        ice_servers: config
            .ice_servers
            .clone()
            .into_iter()
            .map(into_webrtc_ice)
            .collect(),
        ..Default::default()
    };
    let mut api_settings = SettingEngine::default();

    // Configure ICE timeouts - give more time for recovery
    // disconnected_timeout: How long to wait before transitioning from Disconnected to Failed
    // keepalive_interval: How often to send STUN binding requests to keep NAT mappings alive
    // failed_timeout: (Note: this is set via disconnected_timeout in the API)
    api_settings.set_ice_timeouts(
        Some(std::time::Duration::from_secs(60)),  // disconnected_timeout - 60s to allow recovery
        Some(std::time::Duration::from_millis(500)), // keepalive_interval - more aggressive keepalives
        Some(std::time::Duration::from_secs(120)), // failed_timeout - 2 minutes before giving up
    );
    info!("[WebRTC]: ICE timeouts configured - disconnected: 60s, keepalive: 500ms, failed: 120s");

    if let Some(PortRange { min, max }) = config.port_range {
        match EphemeralUDP::new(min, max) {
            Ok(udp) => {
                api_settings.set_udp_network(UDPNetwork::Ephemeral(udp));
            }
            Err(err) => {
                warn!("[Stream]: Invalid port range in config: {err:?}");
            }
        }
    }
    if let Some(mapping) = config.nat_1to1.as_ref() {
        api_settings.set_nat_1to1_ips(
            mapping.ips.clone(),
            into_webrtc_ice_candidate(mapping.ice_candidate_type),
        );
    }
    api_settings.set_network_types(
        config
            .network_types
            .iter()
            .copied()
            .map(into_webrtc_network_type)
            .collect(),
    );

    api_settings.set_include_loopback_candidate(config.include_loopback_candidates);

    // -- Register media codecs
    // TODO: register them based on the sdp
    let mut api_media = MediaEngine::default();
    register_audio_codecs(&mut api_media).expect("failed to register audio codecs");
    register_video_codecs(&mut api_media, stream_settings.video_supported_formats)
        .expect("failed to register video codecs");

    // -- Build Api
    let mut api_registry = Registry::new();

    // Use the default set of Interceptors
    api_registry = register_default_interceptors(api_registry, &mut api_media)
        .expect("failed to register webrtc default interceptors");

    let api = APIBuilder::new()
        .with_setting_engine(api_settings)
        .with_media_engine(api_media)
        .with_interceptor_registry(api_registry)
        .build();

    let (event_sender, event_receiver) = channel::<TransportEvent>(20);

    // Send WebRTC Info
    if let Err(err) = event_sender
        .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
            StreamServerMessage::Setup {
                ice_servers: config.ice_servers.clone(),
                session_token,
            },
        )))
        .await
    {
        error!(
            "Failed to send WebRTC setup message, the client peer will likely not get created: {err:?}"
        );
    };

    // Clone config for potential input peer creation later
    let rtc_config_clone = rtc_config.clone();
    
    let peer = Arc::new(api.new_peer_connection(rtc_config).await?);

    let general_channel = peer.create_data_channel("general", None).await?;

    let runtime = Handle::current();
    let this_owned = Arc::new(WebRtcInner {
        peer: peer.clone(),
        stream_settings: stream_settings.clone(),
        event_sender,
        general_channel,
        stats_channel: Mutex::new(None),
        video: Mutex::new(WebRtcVideo::new(
            runtime.clone(),
            Arc::downgrade(&peer),
            stream_settings.video_supported_formats,
            stream_settings.video_frame_queue_size as usize,
        )),
        audio: Mutex::new(WebRtcAudio::new(
            runtime,
            Arc::downgrade(&peer),
            stream_settings.audio_sample_queue_size as usize,
        )),
        timeout_terminate_request: Mutex::new(None),
        input_peer: Mutex::new(None),
        input_stats_channel: Mutex::new(None),
        rtc_config: rtc_config_clone,
        last_keepalive_response: Mutex::new(Instant::now()),
        keepalive_pings_without_pong: Mutex::new(0),
        offer_batch_deadline: Arc::new(Mutex::new(None)),
        offer_batch_task_running: Arc::new(Mutex::new(false)),
    });

    let this = Arc::downgrade(&this_owned);

    // -- Connection state
    peer.on_ice_connection_state_change(create_event_handler(
        this.clone(),
        async move |this, state| {
            this.on_ice_connection_state_change(state).await;
        },
    ));
    peer.on_peer_connection_state_change(create_event_handler(
        this.clone(),
        async move |this, state| {
            this.on_peer_connection_state_change(state).await;
        },
    ));

    // -- Signaling
    peer.on_ice_candidate(create_event_handler(
        this.clone(),
        async move |this, candidate| {
            this.on_ice_candidate(candidate).await;
        },
    ));

    // -- Data Channels
    peer.on_data_channel(create_event_handler(
        this.clone(),
        async move |this, channel| {
            this.on_data_channel(channel).await;
        },
    ));

    // Note: Keepalive channel is created by client and handled in on_data_channel
    
    drop(peer);

    Ok((
        WebRTCTransportSender {
            inner: this_owned.clone(),
        },
        WebRTCTransportEvents { event_receiver },
    ))
}

// It compiling...
#[allow(clippy::complexity)]
fn create_event_handler<F, Args>(
    inner: Weak<WebRtcInner>,
    f: F,
) -> Box<
    dyn FnMut(Args) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync + 'static,
>
where
    Args: Send + 'static,
    F: AsyncFn(Arc<WebRtcInner>, Args) + Send + Sync + Clone + 'static,
    for<'a> F::CallRefFuture<'a>: Send,
{
    Box::new(move |args: Args| {
        let inner = inner.clone();
        let Some(inner) = inner.upgrade() else {
            debug!("Called webrtc event handler while the main type is already deallocated");
            return Box::pin(ready(())) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
        };

        let future = f.clone();
        Box::pin(async move {
            future(inner, args).await;
        }) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>
    })
        as Box<
            dyn FnMut(Args) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
                + Send
                + Sync
                + 'static,
        >
}
#[allow(clippy::complexity)]
fn create_channel_message_handler(
    inner: Weak<WebRtcInner>,
    channel: TransportChannel,
) -> Box<
    dyn FnMut(DataChannelMessage) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
        + Send
        + Sync
        + 'static,
> {
    create_event_handler(inner, async move |inner, message: DataChannelMessage| {
        let Some(packet) = InboundPacket::deserialize(channel, &message.data) else {
            return;
        };

        if let Err(err) = inner
            .event_sender
            .send(TransportEvent::RecvPacket(packet))
            .await
        {
            warn!("Failed to dispatch RecvPacket event: {err:?}");
        };
    })
}

impl WebRtcInner {
    // -- Handle Connection State
    async fn on_ice_connection_state_change(self: &Arc<Self>, state: RTCIceConnectionState) {
        // Log ICE connection state changes with detailed info
        info!("[ICE]: Connection state changed: {:?}", state);
        
        match state {
            RTCIceConnectionState::New => {
                info!("[ICE]: New - ICE agent is gathering candidates");
            }
            RTCIceConnectionState::Checking => {
                info!("[ICE]: Checking - ICE agent is checking candidate pairs");
            }
            RTCIceConnectionState::Connected => {
                info!("[ICE]: Connected - At least one candidate pair has succeeded");
            }
            RTCIceConnectionState::Completed => {
                info!("[ICE]: Completed - ICE checks have finished and candidate pair selected");
            }
            RTCIceConnectionState::Disconnected => {
                warn!("[ICE]: Disconnected - STUN keepalives not being acknowledged by peer!");
                warn!("[ICE]: This usually means the client stopped responding to STUN binding requests");
                warn!("[ICE]: Server will continue trying for 60s before declaring failure");
            }
            RTCIceConnectionState::Failed => {
                error!("[ICE]: Failed - All candidate pairs have failed. Connection unrecoverable.");
            }
            RTCIceConnectionState::Closed => {
                info!("[ICE]: Closed - ICE agent has been closed");
            }
            _ => {
                info!("[ICE]: Unknown state: {:?}", state);
            }
        }
    }
    
    async fn on_peer_connection_state_change(self: Arc<Self>, state: RTCPeerConnectionState) {
        info!("[WebRTC] Peer connection state changed: {:?}", state);
        
        #[allow(clippy::collapsible_if)]
        if matches!(state, RTCPeerConnectionState::Connected) {
            info!("[WebRTC] Connection established - clearing any pending termination");
            self.clear_terminate_request().await;
            if let Err(err) = self
                .event_sender
                .send(TransportEvent::StartStream {
                    settings: self.stream_settings.clone(),
                })
                .await
            {
                warn!("Failed to send peer connected event to stream: {err:?}");
            }
        } else if matches!(state, RTCPeerConnectionState::Closed) {
            info!("[WebRTC] Connection CLOSED - this is fatal, terminating");
            if let Err(err) = self.event_sender.send(TransportEvent::Closed).await {
                warn!("Failed to send peer closed event to stream: {err:?}");
                self.request_terminate().await;
            };
        } else if matches!(state, RTCPeerConnectionState::Failed) {
            // Only Failed is fatal - Disconnected can recover via ICE restart
            warn!("[WebRTC] Connection FAILED - this is fatal, requesting termination");
            self.request_terminate().await;
        } else if matches!(state, RTCPeerConnectionState::Disconnected) {
            // Disconnected is NOT fatal - ICE can recover
            // Don't request termination, just log it
            warn!("[WebRTC] Connection DISCONNECTED - waiting for ICE recovery (not fatal)");
            // Do NOT call request_terminate() here - let ICE try to recover
        } else {
            // For other states (New, Connecting), clear any pending termination
            info!("[WebRTC] Connection state: {:?} - clearing termination request", state);
            self.clear_terminate_request().await;
        }
    }

    // -- Handle Signaling
    async fn send_answer(&self) -> bool {
        let local_description = match self.peer.create_answer(None).await {
            Err(err) => {
                warn!("[Signaling]: failed to create answer: {err:?}");
                return false;
            }
            Ok(value) => value,
        };

        if let Err(err) = self
            .peer
            .set_local_description(local_description.clone())
            .await
        {
            warn!("[Signaling]: failed to set local description: {err:?}");
            return false;
        }

        debug!(
            "[Signaling] Sending Local Description as Answer: {:?}",
            local_description.sdp
        );

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                StreamServerMessage::WebRtc(StreamSignalingMessage::Description(
                    RtcSessionDescription {
                        ty: from_webrtc_sdp(local_description.sdp_type),
                        sdp: local_description.sdp,
                    },
                )),
            )))
            .await
        {
            warn!("Failed to send local description (answer) via web socket from peer: {err:?}");
        }

        true
    }
    /// Request to send an offer, with batching to prevent duplicate offers
    /// when multiple tracks are added in quick succession.
    /// 
    /// This uses a deadline-based batching approach:
    /// - First call sets a deadline 300ms in the future
    /// - Subsequent calls extend the deadline by 100ms (up to max 500ms from first call)
    /// - Offer is sent when deadline is reached
    /// - This ensures all tracks added within the batch window are included
    async fn send_offer(&self) -> bool {
        const INITIAL_BATCH_DELAY_MS: u64 = 300; // Initial delay to wait for more tracks
        const EXTENSION_MS: u64 = 100; // Extend deadline by this much for each new request
        const MAX_TOTAL_DELAY_MS: u64 = 500; // Maximum total delay from first request
        
        let now = Instant::now();
        
        let should_start_task: bool;
        {
            let mut deadline = self.offer_batch_deadline.lock().await;
            let mut task_running = self.offer_batch_task_running.lock().await;
            
            if let Some(current_deadline) = *deadline {
                // Already have a pending batch - extend the deadline if possible
                let time_since_first = now.duration_since(current_deadline - Duration::from_millis(INITIAL_BATCH_DELAY_MS));
                if time_since_first < Duration::from_millis(MAX_TOTAL_DELAY_MS - EXTENSION_MS) {
                    let new_deadline = now + Duration::from_millis(EXTENSION_MS);
                    if new_deadline > current_deadline {
                        *deadline = Some(new_deadline);
                        info!("[Signaling]: Extended offer batch deadline by {}ms (now {:?} from now)", 
                            EXTENSION_MS, new_deadline.duration_since(now));
                    }
                } else {
                    info!("[Signaling]: Offer batch at max delay, not extending further");
                }
                should_start_task = false;
            } else {
                // No pending batch - start a new one
                let new_deadline = now + Duration::from_millis(INITIAL_BATCH_DELAY_MS);
                *deadline = Some(new_deadline);
                should_start_task = !*task_running;
                if should_start_task {
                    *task_running = true;
                }
                info!("[Signaling]: Starting new offer batch, will send in {}ms", INITIAL_BATCH_DELAY_MS);
            }
        }
        
        if should_start_task {
            // Start the batch task
            let peer = self.peer.clone();
            let event_sender = self.event_sender.clone();
            let offer_batch_deadline = self.offer_batch_deadline.clone();
            let offer_batch_task_running = self.offer_batch_task_running.clone();
            
            tokio::spawn(async move {
                loop {
                    // Check deadline and sleep until it's reached
                    let sleep_duration = {
                        let deadline = offer_batch_deadline.lock().await;
                        match *deadline {
                            Some(d) => {
                                let now = Instant::now();
                                if d > now {
                                    d.duration_since(now)
                                } else {
                                    Duration::ZERO
                                }
                            }
                            None => {
                                // Deadline was cleared, abort
                                let mut task_running = offer_batch_task_running.lock().await;
                                *task_running = false;
                                return;
                            }
                        }
                    };
                    
                    if sleep_duration > Duration::ZERO {
                        sleep(sleep_duration).await;
                    }
                    
                    // Check if deadline was extended while we were sleeping
                    let should_send = {
                        let deadline = offer_batch_deadline.lock().await;
                        match *deadline {
                            Some(d) => Instant::now() >= d,
                            None => false,
                        }
                    };
                    
                    if should_send {
                        break;
                    }
                }
                
                // Clear the deadline and send the offer
                {
                    let mut deadline = offer_batch_deadline.lock().await;
                    *deadline = None;
                }
                
                info!("[Signaling]: Batch deadline reached, sending offer now");
                
                // Send offer
                let local_description = match peer.create_offer(None).await {
                    Err(err) => {
                        warn!("[Signaling]: failed to create batched offer: {err:?}");
                        let mut task_running = offer_batch_task_running.lock().await;
                        *task_running = false;
                        return;
                    }
                    Ok(value) => value,
                };

                if let Err(err) = peer.set_local_description(local_description.clone()).await {
                    warn!("[Signaling]: failed to set local description for batched offer: {err:?}");
                    let mut task_running = offer_batch_task_running.lock().await;
                    *task_running = false;
                    return;
                }

                info!("[Signaling]: Batched offer created and set (SDP length: {} bytes)", local_description.sdp.len());

                if let Err(err) = event_sender
                    .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                        StreamServerMessage::WebRtc(StreamSignalingMessage::Description(
                            RtcSessionDescription {
                                ty: from_webrtc_sdp(local_description.sdp_type),
                                sdp: local_description.sdp,
                            },
                        )),
                    )))
                    .await
                {
                    warn!("Failed to send batched offer via web socket: {err:?}");
                }
                
                let mut task_running = offer_batch_task_running.lock().await;
                *task_running = false;
            });
        }
        
        true
    }
    
    /// Actually send the offer (internal implementation)
    async fn send_offer_immediate(&self) -> bool {
        let local_description = match self.peer.create_offer(None).await {
            Err(err) => {
                warn!("[Signaling]: failed to create offer: {err:?}");
                return false;
            }
            Ok(value) => value,
        };

        if let Err(err) = self
            .peer
            .set_local_description(local_description.clone())
            .await
        {
            warn!("[Signaling]: failed to set local description: {err:?}");
            return false;
        }

        info!(
            "[Signaling]: Offer created and set as local description (SDP length: {} bytes)",
            local_description.sdp.len()
        );
        debug!(
            "[Signaling] Sending Local Description as Offer: {:?}",
            local_description.sdp
        );

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                StreamServerMessage::WebRtc(StreamSignalingMessage::Description(
                    RtcSessionDescription {
                        ty: from_webrtc_sdp(local_description.sdp_type),
                        sdp: local_description.sdp,
                    },
                )),
            )))
            .await
        {
            warn!("Failed to send local description (offer) via web socket from peer: {err:?}");
        };

        true
    }

    async fn on_ws_message(&self, message: StreamClientMessage) {
        match message {
            StreamClientMessage::WebRtc(StreamSignalingMessage::Description(description)) => {
                debug!("[Signaling] Received Remote Description: {:?}", description);

                let description = match &description.ty {
                    RtcSdpType::Offer => RTCSessionDescription::offer(description.sdp),
                    RtcSdpType::Answer => RTCSessionDescription::answer(description.sdp),
                    RtcSdpType::Pranswer => RTCSessionDescription::pranswer(description.sdp),
                    _ => {
                        warn!(
                            "[Signaling]: failed to handle RTCSdpType {:?}",
                            description.ty
                        );
                        return;
                    }
                };

                let Ok(description) = description else {
                    warn!("[Signaling]: Received invalid RTCSessionDescription");
                    return;
                };

                let remote_ty = description.sdp_type;
                if let Err(err) = self.peer.set_remote_description(description).await {
                    warn!("[Signaling]: failed to set remote description: {err:?}");
                    return;
                }

                // Send an answer (local description) if we got an offer
                if remote_ty == RTCSdpType::Offer {
                    self.send_answer().await;
                }
            }
            StreamClientMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(description)) => {
                debug!("[Signaling] Received Ice Candidate");

                if let Err(err) = self
                    .peer
                    .add_ice_candidate(RTCIceCandidateInit {
                        candidate: description.candidate,
                        sdp_mid: description.sdp_mid,
                        sdp_mline_index: description.sdp_mline_index,
                        username_fragment: description.username_fragment,
                    })
                    .await
                {
                    warn!("[Signaling]: failed to add ice candidate: {err:?}");
                }
            }
            // This should already be done
            StreamClientMessage::Init { .. } => {}
        }
    }

    async fn on_ice_candidate(&self, candidate: Option<RTCIceCandidate>) {
        let Some(candidate) = candidate else {
            return;
        };

        let Ok(candidate_json) = candidate.to_json() else {
            return;
        };

        debug!(
            "[Signaling] Sending Ice Candidate: {}",
            candidate_json.candidate
        );

        let message =
            StreamServerMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(RtcIceCandidate {
                candidate: candidate_json.candidate,
                sdp_mid: candidate_json.sdp_mid,
                sdp_mline_index: candidate_json.sdp_mline_index,
                username_fragment: candidate_json.username_fragment,
            }));

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                message,
            )))
            .await
        {
            error!("Failed to send web socket message from peer: {err:?}");
        };
    }

    async fn on_data_channel(self: Arc<Self>, channel: Arc<RTCDataChannel>) {
        let label = channel.label();
        debug!("adding data channel: \"{label}\"");

        let inner = Arc::downgrade(&self);

        match label {
            "stats" => {
                let mut stats = self.stats_channel.lock().await;

                channel.on_close({
                    let this = Arc::downgrade(&self);

                    Box::new(move ||{
                        let this = this.clone();

                        Box::pin(async move {
                            let Some(this) = this.upgrade() else {
                                warn!("Failed to close stats channel because the main type is already deallocated");
                                return;
                            };

                            this.close_stats().await;
                        })
                    })
                });

                *stats = Some(channel);
            }
            "mouse_reliable" | "mouse_absolute" | "mouse_relative" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::MOUSE_ABSOLUTE),
                ));
            }
            "touch" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::TOUCH),
                ));
            }
            "keyboard" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::KEYBOARD),
                ));
            }
            "controllers" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::CONTROLLERS),
                ));
            }
            _ if let Some(number) = label.strip_prefix("controller")
                && let Ok(id) = number.parse::<usize>()
                && id < InboundPacket::CONTROLLER_CHANNELS.len() =>
            {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(InboundPacket::CONTROLLER_CHANNELS[id]),
                ));
            }
            "keepalive" => {
                // Handle client-created keepalive channel
                info!("[Keepalive]: Received keepalive channel from client");
                
                let inner_weak = inner.clone();
                let channel_clone = channel.clone();
                
                // Set up message handler for client pings
                channel.on_message(Box::new(move |msg: DataChannelMessage| {
                    let inner = inner_weak.clone();
                    let channel = channel_clone.clone();
                    Box::pin(async move {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };
                        inner.handle_client_keepalive(channel, msg).await;
                    })
                }));
                
                // Start sending pings on this channel
                let inner_weak = Arc::downgrade(&self);
                let channel_for_ping = channel.clone();
                let event_sender = self.event_sender.clone();
                spawn(async move {
                    // Wait for channel to be ready
                    sleep(Duration::from_secs(1)).await;
                    
                    // Asymmetric connectivity detection threshold:
                    // If we've sent 3 pings (9 seconds) without receiving a pong,
                    // declare the connection dead. This catches cases where server->client
                    // path works but client->server path is broken.
                    const MAX_PINGS_WITHOUT_PONG: u32 = 3;
                    
                    loop {
                        let Some(inner) = inner_weak.upgrade() else {
                            info!("[Keepalive]: Inner dropped, stopping client keepalive task");
                            break;
                        };
                        
                        if channel_for_ping.ready_state() == webrtc::data_channel::data_channel_state::RTCDataChannelState::Open {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let ping_msg = format!(r#"{{"type":"ping","ts":{}}}"#, timestamp);
                            
                            // Increment pings without pong counter BEFORE sending
                            let pings_without_pong = {
                                let mut counter = inner.keepalive_pings_without_pong.lock().await;
                                *counter += 1;
                                *counter
                            };
                            
                            if let Err(err) = channel_for_ping.send_text(ping_msg).await {
                                warn!("[Keepalive]: Failed to send ping to client: {err:?}");
                            } else {
                                debug!("[Keepalive]: Sent ping #{} to client at {}", pings_without_pong, timestamp);
                            }
                            
                            // Check for asymmetric connectivity (server can send, client can't respond)
                            if pings_without_pong >= MAX_PINGS_WITHOUT_PONG {
                                let last_response = inner.last_keepalive_response.lock().await;
                                let elapsed = last_response.elapsed();
                                drop(last_response);
                                
                                warn!("[Keepalive]: ASYMMETRIC CONNECTIVITY DETECTED!");
                                warn!("[Keepalive]: Sent {} pings without receiving any pong response", pings_without_pong);
                                warn!("[Keepalive]: Last pong was {:?} ago - client->server path appears broken", elapsed);
                                warn!("[Keepalive]: Requesting termination due to one-way connectivity loss");
                                
                                // Request termination - the connection is effectively dead
                                // even though ICE might still think it's connected
                                inner.request_terminate().await;
                                
                                // Also send a close event to the server
                                let _ = event_sender
                                    .send(TransportEvent::Closed)
                                    .await;
                                
                                break;
                            }
                            
                            // Also log standard timeout warning
                            let last_response = inner.last_keepalive_response.lock().await;
                            let elapsed = last_response.elapsed();
                            drop(last_response);
                            
                            if elapsed > Duration::from_secs(10) {
                                warn!("[Keepalive]: No client keepalive response for {:?} - connection may be dead (pings without pong: {})", 
                                    elapsed, pings_without_pong);
                            }
                        } else {
                            debug!("[Keepalive]: Channel not open yet, state: {:?}", channel_for_ping.ready_state());
                        }
                        
                        sleep(Duration::from_secs(3)).await;
                    }
                });
            }
            _ => {
                debug!("[DataChannel]: Ignoring unknown channel: {}", label);
            }
        };
    }
    
    async fn handle_client_keepalive(&self, channel: Arc<RTCDataChannel>, msg: DataChannelMessage) {
        let data = match std::str::from_utf8(&msg.data) {
            Ok(s) => s,
            Err(_) => {
                warn!("[Keepalive]: Received non-UTF8 keepalive message from client");
                return;
            }
        };
        
        if data.contains(r#""type":"ping""#) {
            // Respond with pong
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let pong_msg = format!(r#"{{"type":"pong","ts":{}}}"#, timestamp);
            
            if let Err(err) = channel.send_text(pong_msg).await {
                warn!("[Keepalive]: Failed to send pong to client: {err:?}");
            } else {
                debug!("[Keepalive]: Received client ping, sent pong");
            }
        } else if data.contains(r#""type":"pong""#) {
            // Reset the pings-without-pong counter - client->server path is working
            let prev_count = {
                let mut counter = self.keepalive_pings_without_pong.lock().await;
                let prev = *counter;
                *counter = 0;
                prev
            };
            
            let mut last_response = self.last_keepalive_response.lock().await;
            let elapsed = last_response.elapsed();
            *last_response = Instant::now();
            
            if prev_count > 1 {
                info!("[Keepalive]: Received client pong after {} missed pings (round-trip: {:?}) - connectivity restored", 
                    prev_count, elapsed);
            } else {
                debug!("[Keepalive]: Received client pong (round-trip: {:?})", elapsed);
            }
        }
    }

    async fn close_stats(&self) {
        let mut stats = self.stats_channel.lock().await;

        *stats = None;
    }

    // -- Input Peer (Hybrid Mode)
    async fn create_input_peer(self: &Arc<Self>) {
        info!("[InputPeer]: Creating input-only peer connection");

        // Create a new peer connection for input only (no media)
        let api = APIBuilder::new().build();

        let input_peer = match api.new_peer_connection(self.rtc_config.clone()).await {
            Ok(peer) => Arc::new(peer),
            Err(err) => {
                error!("[InputPeer]: Failed to create input peer connection: {err:?}");
                return;
            }
        };

        let inner = Arc::downgrade(self);

        // -- ICE candidate handler for input peer
        input_peer.on_ice_candidate({
            let inner = inner.clone();
            Box::new(move |candidate: Option<RTCIceCandidate>| {
                let inner = inner.clone();
                Box::pin(async move {
                    let Some(inner) = inner.upgrade() else {
                        return;
                    };
                    inner.on_input_ice_candidate(candidate).await;
                })
            })
        });

        // -- Data channel handler for input peer (for channels created by client)
        input_peer.on_data_channel({
            let inner = inner.clone();
            Box::new(move |channel: Arc<RTCDataChannel>| {
                let inner = inner.clone();
                Box::pin(async move {
                    let Some(inner) = inner.upgrade() else {
                        return;
                    };
                    inner.on_input_data_channel(channel).await;
                })
            })
        });

        // -- Connection state handler
        input_peer.on_peer_connection_state_change({
            let inner = inner.clone();
            Box::new(move |state: RTCPeerConnectionState| {
                let inner = inner.clone();
                Box::pin(async move {
                    let Some(inner) = inner.upgrade() else {
                        return;
                    };
                    inner.on_input_peer_state_change(state).await;
                })
            })
        });

        // ===== CREATE DATA CHANNELS BEFORE GENERATING OFFER =====
        // Data channels MUST be created before the SDP offer so they are included in the offer
        // Since the server creates the channels, we must attach message handlers directly
        // (on_data_channel callback only fires for remotely-created channels)
        
        // Create ordered data channels for reliable input
        let ordered_config = RTCDataChannelInit {
            ordered: Some(true),
            ..Default::default()
        };
        
        // Create unordered data channels for low-latency input
        let unordered_config = RTCDataChannelInit {
            ordered: Some(false),
            max_retransmits: Some(0),
            ..Default::default()
        };

        // Helper to create channel and attach message handler
        async fn create_input_channel(
            peer: &RTCPeerConnection,
            name: &str,
            config: RTCDataChannelInit,
            inner: &Weak<WebRtcInner>,
            channel_type: TransportChannel,
        ) -> Option<Arc<RTCDataChannel>> {
            match peer.create_data_channel(name, Some(config)).await {
                Ok(channel) => {
                    channel.on_message(create_channel_message_handler(inner.clone(), channel_type));
                    info!("[InputPeer]: Created and attached handler for channel: {}", name);
                    Some(channel)
                }
                Err(err) => {
                    error!("[InputPeer]: Failed to create {} channel: {err:?}", name);
                    None
                }
            }
        }

        // Mouse channels
        create_input_channel(
            &input_peer, "mouse_reliable", ordered_config.clone(), &inner,
            TransportChannel(TransportChannelId::MOUSE_RELIABLE)
        ).await;
        create_input_channel(
            &input_peer, "mouse_absolute", unordered_config.clone(), &inner,
            TransportChannel(TransportChannelId::MOUSE_ABSOLUTE)
        ).await;
        create_input_channel(
            &input_peer, "mouse_relative", unordered_config.clone(), &inner,
            TransportChannel(TransportChannelId::MOUSE_RELATIVE)
        ).await;

        // Keyboard channel (ordered for key sequence integrity)
        create_input_channel(
            &input_peer, "keyboard", ordered_config.clone(), &inner,
            TransportChannel(TransportChannelId::KEYBOARD)
        ).await;

        // Touch channel (ordered)
        create_input_channel(
            &input_peer, "touch", ordered_config.clone(), &inner,
            TransportChannel(TransportChannelId::TOUCH)
        ).await;

        // Controllers channel (unordered for low latency)
        create_input_channel(
            &input_peer, "controllers", unordered_config.clone(), &inner,
            TransportChannel(TransportChannelId::CONTROLLERS)
        ).await;

        // Individual controller channels (controller0 through controller15)
        for i in 0..16 {
            let channel_name = format!("controller{}", i);
            create_input_channel(
                &input_peer, &channel_name, unordered_config.clone(), &inner,
                TransportChannel(InboundPacket::CONTROLLER_CHANNELS[i])
            ).await;
        }

        // Stats channel for latency info (ordered) - store reference for sending stats
        if let Ok(stats_channel) = input_peer.create_data_channel("stats", Some(ordered_config.clone())).await {
            stats_channel.on_close({
                let this = inner.clone();
                Box::new(move || {
                    let this = this.clone();
                    Box::pin(async move {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        let mut input_stats = this.input_stats_channel.lock().await;
                        *input_stats = None;
                        info!("[InputPeer]: Input stats channel closed");
                    })
                })
            });
            let mut input_stats = self.input_stats_channel.lock().await;
            *input_stats = Some(stats_channel);
            info!("[InputPeer]: Created and stored stats channel");
        } else {
            error!("[InputPeer]: Failed to create stats channel");
        }

        info!("[InputPeer]: Created all input data channels with message handlers");

        // Store the input peer
        {
            let mut input_peer_guard = self.input_peer.lock().await;
            *input_peer_guard = Some(input_peer.clone());
        }

        // Now create an offer for the input peer (server-initiated)
        // The offer will now include all the data channels we created
        match input_peer.create_offer(None).await {
            Ok(offer) => {
                if let Err(err) = input_peer.set_local_description(offer.clone()).await {
                    error!("[InputPeer]: Failed to set local description: {err:?}");
                    return;
                }

                info!("[InputPeer]: Sending offer to input client (SDP includes {} bytes)", offer.sdp.len());
                debug!("[InputPeer]: SDP offer:\n{}", offer.sdp);
                
                if let Err(err) = self
                    .event_sender
                    .send(TransportEvent::SendIpc(StreamerIpcMessage::InputSignaling(
                        StreamSignalingMessage::Description(RtcSessionDescription {
                            ty: from_webrtc_sdp(offer.sdp_type),
                            sdp: offer.sdp,
                        }),
                    )))
                    .await
                {
                    error!("[InputPeer]: Failed to send offer: {err:?}");
                }
            }
            Err(err) => {
                error!("[InputPeer]: Failed to create offer: {err:?}");
            }
        }
    }

    async fn on_input_signaling(&self, signaling: StreamSignalingMessage) {
        let input_peer_guard = self.input_peer.lock().await;
        let Some(ref input_peer) = *input_peer_guard else {
            warn!("[InputPeer]: Received signaling but input peer not created");
            return;
        };

        match signaling {
            StreamSignalingMessage::Description(description) => {
                debug!("[InputPeer]: Received remote description");

                let description = match &description.ty {
                    RtcSdpType::Offer => RTCSessionDescription::offer(description.sdp),
                    RtcSdpType::Answer => RTCSessionDescription::answer(description.sdp),
                    RtcSdpType::Pranswer => RTCSessionDescription::pranswer(description.sdp),
                    _ => {
                        warn!("[InputPeer]: Unsupported SDP type: {:?}", description.ty);
                        return;
                    }
                };

                let Ok(description) = description else {
                    warn!("[InputPeer]: Invalid RTCSessionDescription");
                    return;
                };

                if let Err(err) = input_peer.set_remote_description(description).await {
                    warn!("[InputPeer]: Failed to set remote description: {err:?}");
                }
            }
            StreamSignalingMessage::AddIceCandidate(candidate) => {
                debug!("[InputPeer]: Adding ICE candidate");

                if let Err(err) = input_peer
                    .add_ice_candidate(RTCIceCandidateInit {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                        username_fragment: candidate.username_fragment,
                    })
                    .await
                {
                    warn!("[InputPeer]: Failed to add ICE candidate: {err:?}");
                }
            }
        }
    }

    async fn on_input_ice_candidate(&self, candidate: Option<RTCIceCandidate>) {
        let Some(candidate) = candidate else {
            return;
        };

        let Ok(candidate_json) = candidate.to_json() else {
            return;
        };

        debug!("[InputPeer]: Sending ICE candidate to input client");

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::InputSignaling(
                StreamSignalingMessage::AddIceCandidate(RtcIceCandidate {
                    candidate: candidate_json.candidate,
                    sdp_mid: candidate_json.sdp_mid,
                    sdp_mline_index: candidate_json.sdp_mline_index,
                    username_fragment: candidate_json.username_fragment,
                }),
            )))
            .await
        {
            error!("[InputPeer]: Failed to send ICE candidate: {err:?}");
        }
    }

    async fn on_input_data_channel(self: Arc<Self>, channel: Arc<RTCDataChannel>) {
        let label = channel.label();
        info!("[InputPeer]: Data channel opened: \"{label}\"");

        let inner = Arc::downgrade(&self);

        // Set up message handler - same as primary peer, routes to same event_sender
        match label {
            "stats" => {
                // In hybrid mode, stats go to the input client
                info!("[InputPeer]: Stats channel opened on input peer");
                let mut input_stats = self.input_stats_channel.lock().await;

                channel.on_close({
                    let this = Arc::downgrade(&self);

                    Box::new(move || {
                        let this = this.clone();

                        Box::pin(async move {
                            let Some(this) = this.upgrade() else {
                                warn!("[InputPeer]: Failed to close input stats channel");
                                return;
                            };

                            let mut input_stats = this.input_stats_channel.lock().await;
                            *input_stats = None;
                            info!("[InputPeer]: Input stats channel closed");
                        })
                    })
                });

                *input_stats = Some(channel);
            }
            "mouse_reliable" | "mouse_absolute" | "mouse_relative" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::MOUSE_ABSOLUTE),
                ));
            }
            "touch" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::TOUCH),
                ));
            }
            "keyboard" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::KEYBOARD),
                ));
            }
            "controllers" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::CONTROLLERS),
                ));
            }
            _ if let Some(number) = label.strip_prefix("controller")
                && let Ok(id) = number.parse::<usize>()
                && id < InboundPacket::CONTROLLER_CHANNELS.len() =>
            {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(InboundPacket::CONTROLLER_CHANNELS[id]),
                ));
            }
            _ => {
                debug!("[InputPeer]: Unknown data channel: {label}");
            }
        };
    }

    async fn on_input_peer_state_change(&self, state: RTCPeerConnectionState) {
        info!("[InputPeer]: Connection state changed: {:?}", state);

        if matches!(state, RTCPeerConnectionState::Connected) {
            // Notify that input peer is ready
            if let Err(err) = self
                .event_sender
                .send(TransportEvent::SendIpc(StreamerIpcMessage::InputReady))
                .await
            {
                warn!("[InputPeer]: Failed to send InputReady: {err:?}");
            }
        } else if matches!(
            state,
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Closed
        ) {
            info!("[InputPeer]: Input peer disconnected");
            // Clean up input peer
            let mut input_peer_guard = self.input_peer.lock().await;
            *input_peer_guard = None;
        }
    }

    async fn close_input_peer(&self) {
        // Clean up input stats channel
        {
            let mut input_stats = self.input_stats_channel.lock().await;
            *input_stats = None;
        }
        
        // Close and clean up input peer
        let mut input_peer_guard = self.input_peer.lock().await;
        if let Some(ref input_peer) = *input_peer_guard {
            let _ = input_peer.close().await;
        }
        *input_peer_guard = None;
        info!("[InputPeer]: Input peer closed");
    }

    // -- Termination
    async fn request_terminate(self: &Arc<Self>) {
        let this = self.clone();

        let mut terminate_request = self.timeout_terminate_request.lock().await;
        *terminate_request = Some(Instant::now());
        drop(terminate_request);

        spawn(async move {
            sleep(TIMEOUT_DURATION + Duration::from_millis(200)).await;

            let now = Instant::now();

            let terminate_request = this.timeout_terminate_request.lock().await;
            if let Some(terminate_request) = *terminate_request
                && (now - terminate_request) > TIMEOUT_DURATION
            {
                info!("Stopping because of timeout");
                if let Err(err) = this.event_sender.send(TransportEvent::Closed).await {
                    warn!("Failed to send that the peer should close: {err:?}");
                };
            }
        });
    }
    async fn clear_terminate_request(&self) {
        let mut request = self.timeout_terminate_request.lock().await;

        *request = None;
    }
}

pub struct WebRTCTransportEvents {
    event_receiver: Receiver<TransportEvent>,
}

impl TransportEvents for WebRTCTransportEvents {
    async fn poll_event(&mut self) -> Result<TransportEvent, TransportError> {
        self.event_receiver
            .recv()
            .await
            .ok_or(TransportError::Closed)
    }
}

pub struct WebRTCTransportSender {
    inner: Arc<WebRtcInner>,
}

#[async_trait]
impl TransportSender for WebRTCTransportSender {
    async fn setup_video(&self, setup: VideoSetup) -> i32 {
        let mut video = self.inner.video.lock().await;
        if video.setup(&self.inner, setup).await {
            0
        } else {
            -1
        }
    }
    async fn send_video_unit<'a>(
        &'a self,
        unit: &'a VideoDecodeUnit<'a>,
    ) -> Result<DecodeResult, TransportError> {
        let mut video = self.inner.video.lock().await;
        Ok(video.send_decode_unit(unit).await)
    }

    async fn setup_audio(
        &self,
        audio_config: AudioConfig,
        stream_config: OpusMultistreamConfig,
    ) -> i32 {
        let mut audio = self.inner.audio.lock().await;

        audio.setup(&self.inner, audio_config, stream_config).await
    }
    async fn send_audio_sample(&self, data: &[u8]) -> Result<(), TransportError> {
        let mut audio = self.inner.audio.lock().await;

        audio.send_audio_sample(data).await;

        Ok(())
    }

    async fn send(&self, packet: OutboundPacket) -> Result<(), TransportError> {
        let mut buffer = Vec::new();

        let Some((channel, range)) = packet.serialize(&mut buffer) else {
            warn!("Failed to serialize packet: {packet:?}");
            return Ok(());
        };

        let bytes = Bytes::from(buffer);
        let bytes = bytes.slice(range);

        match channel.0 {
            TransportChannelId::GENERAL => match self.inner.general_channel.send(&bytes).await {
                Ok(_) => {}
                Err(webrtc::Error::ErrDataChannelNotOpen) => {
                    return Err(TransportError::ChannelClosed);
                }
                _ => {}
            },
            TransportChannelId::STATS => {
                // In hybrid mode, prefer the input stats channel (native client)
                // Fall back to primary stats channel if input not available
                let input_stats = self.inner.input_stats_channel.lock().await;
                if let Some(input_stats) = input_stats.as_ref() {
                    match input_stats.send(&bytes).await {
                        Ok(_) => {}
                        Err(webrtc::Error::ErrDataChannelNotOpen) => {
                            return Err(TransportError::ChannelClosed);
                        }
                        _ => {}
                    }
                } else {
                    // Fall back to primary stats channel
                    drop(input_stats);
                    let stats = self.inner.stats_channel.lock().await;
                    if let Some(stats) = stats.as_ref() {
                        match stats.send(&bytes).await {
                            Ok(_) => {}
                            Err(webrtc::Error::ErrDataChannelNotOpen) => {
                                return Err(TransportError::ChannelClosed);
                            }
                            _ => {}
                        }
                    } else {
                        return Err(TransportError::ChannelClosed);
                    }
                }
            }
            _ => {
                warn!("Cannot send data on channel {channel:?}");
                return Err(TransportError::ChannelClosed);
            }
        }
        Ok(())
    }

    async fn on_ipc_message(&self, message: ServerIpcMessage) -> Result<(), TransportError> {
        match message {
            ServerIpcMessage::WebSocket(message) => {
                self.inner.on_ws_message(message).await;
            }
            ServerIpcMessage::InputJoined => {
                info!("[WebRTC]: Input connection joined - creating input peer");
                self.inner.clone().create_input_peer().await;
            }
            ServerIpcMessage::InputWebSocket(signaling) => {
                debug!("[WebRTC]: Received input signaling message");
                self.inner.on_input_signaling(signaling).await;
            }
            ServerIpcMessage::InputDisconnected => {
                info!("[WebRTC]: Input connection disconnected");
                self.inner.close_input_peer().await;
            }
            ServerIpcMessage::Init { .. } | ServerIpcMessage::Stop => {
                // These are handled elsewhere
            }
        }
        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.inner
            .peer
            .close()
            .await
            .map_err(|err| TransportError::Implementation(err.into()))?;

        Ok(())
    }
}

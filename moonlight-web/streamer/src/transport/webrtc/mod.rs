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
    async fn on_ice_connection_state_change(self: &Arc<Self>, _state: RTCIceConnectionState) {}
    async fn on_peer_connection_state_change(self: Arc<Self>, state: RTCPeerConnectionState) {
        #[allow(clippy::collapsible_if)]
        if matches!(state, RTCPeerConnectionState::Connected) {
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
            if let Err(err) = self.event_sender.send(TransportEvent::Closed).await {
                warn!("Failed to send peer closed event to stream: {err:?}");
                self.request_terminate().await;
            };
        } else if matches!(
            state,
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected
        ) {
            self.request_terminate().await;
        } else {
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
    async fn send_offer(&self) -> bool {
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
            _ => {}
        };
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

        // Mouse channels
        if let Err(err) = input_peer.create_data_channel("mouse_reliable", Some(ordered_config.clone())).await {
            error!("[InputPeer]: Failed to create mouse_reliable channel: {err:?}");
        }
        if let Err(err) = input_peer.create_data_channel("mouse_absolute", Some(unordered_config.clone())).await {
            error!("[InputPeer]: Failed to create mouse_absolute channel: {err:?}");
        }
        if let Err(err) = input_peer.create_data_channel("mouse_relative", Some(unordered_config.clone())).await {
            error!("[InputPeer]: Failed to create mouse_relative channel: {err:?}");
        }

        // Keyboard channel (ordered for key sequence integrity)
        if let Err(err) = input_peer.create_data_channel("keyboard", Some(ordered_config.clone())).await {
            error!("[InputPeer]: Failed to create keyboard channel: {err:?}");
        }

        // Touch channel (ordered)
        if let Err(err) = input_peer.create_data_channel("touch", Some(ordered_config.clone())).await {
            error!("[InputPeer]: Failed to create touch channel: {err:?}");
        }

        // Controllers channel (unordered for low latency)
        if let Err(err) = input_peer.create_data_channel("controllers", Some(unordered_config.clone())).await {
            error!("[InputPeer]: Failed to create controllers channel: {err:?}");
        }

        // Individual controller channels (controller0 through controller15)
        for i in 0..16 {
            let channel_name = format!("controller{}", i);
            if let Err(err) = input_peer.create_data_channel(&channel_name, Some(unordered_config.clone())).await {
                error!("[InputPeer]: Failed to create {} channel: {err:?}", channel_name);
            }
        }

        // Stats channel for latency info (ordered)
        if let Err(err) = input_peer.create_data_channel("stats", Some(ordered_config.clone())).await {
            error!("[InputPeer]: Failed to create stats channel: {err:?}");
        }

        info!("[InputPeer]: Created all input data channels");

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

//! WebTransport transport implementation
//!
//! This module provides a WebTransport-based transport layer for streaming
//! video, audio, and input data. WebTransport uses HTTP/3 (QUIC) and provides
//! both unreliable datagrams (for video) and reliable streams (for audio/input).

use std::{
    net::SocketAddr,
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use common::{
    StreamSettings,
    api_bindings::{
        RtcIceCandidate, RtcSdpType, RtcSessionDescription, StreamClientMessage,
        StreamServerMessage, StreamSignalingMessage, TransportChannelId,
    },
    config::WebTransportConfig,
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
};
use wtransport::{Endpoint, ServerConfig};

use crate::transport::{
    InboundPacket, OutboundPacket, TransportChannel, TransportError, TransportEvent,
    TransportEvents, TransportSender,
};

mod audio;
mod cert;
mod channels;
mod video;

pub use cert::CertPair;

pub const TIMEOUT_DURATION: Duration = Duration::from_secs(10);

struct WebTransportInner {
    endpoint: Arc<Endpoint<wtransport::endpoint::endpoint_side::Server>>,
    stream_settings: StreamSettings,
    event_sender: Sender<TransportEvent>,
    // Main session (video/audio) - from WebView
    // wtransport 0.6 uses Connection instead of Session
    main_session: Mutex<Option<Arc<wtransport::Connection>>>,
    // Input session (input only) - from native client with session token
    input_session: Mutex<Option<Arc<wtransport::Connection>>>,
    session_token: Option<String>,
    cert_hash: String,
    timeout_terminate_request: Mutex<Option<std::time::Instant>>,
    // Video and audio handlers
    video: Mutex<video::WebTransportVideo>,
    audio: Mutex<audio::WebTransportAudio>,
    // Input channels (for input session)
    input_channels: Mutex<Option<channels::WebTransportChannels>>,
    // Shutdown signal for session acceptance loop
    shutdown: tokio::sync::watch::Sender<bool>,
}

pub async fn new(
    stream_settings: StreamSettings,
    config: &WebTransportConfig,
    session_token: Option<String>,
) -> Result<(WebTransportSender, WebTransportEvents), anyhow::Error> {
    info!("[WebTransport]: Initializing WebTransport server");
    
    // Load or generate certificate
    let cert_pair = if let (Some(cert_path), Some(key_path)) = (
        config.certificate_path.as_ref(),
        config.private_key_path.as_ref(),
    ) {
        cert::CertPair::load_from_files_async(Path::new(cert_path), Path::new(key_path)).await
            .context("Failed to load certificate from files")?
    } else {
        info!("[WebTransport]: No certificate paths provided, generating self-signed certificate");
        cert::CertPair::generate_self_signed()
            .context("Failed to generate self-signed certificate")?
    };
    
    let cert_hash = cert_pair.hash().to_string();
    info!("[WebTransport]: Certificate hash: {}", cert_hash);
    
    // Determine bind address
    let bind_addr = config.bind_address.unwrap_or_else(|| {
        SocketAddr::from(([0, 0, 0, 0], 4433))
    });
    
    // Create server config with identity
    let identity = cert_pair.into_identity();
    let server_config = ServerConfig::builder()
        .with_bind_address(bind_addr)
        .with_identity(identity)
        .build();
    
    // Create endpoint
    let endpoint = Arc::new(
        Endpoint::server(server_config)
            .context("Failed to create WebTransport endpoint")?
    );
    
    info!("[WebTransport]: Server endpoint created on {}", bind_addr);
    
    let (event_sender, event_receiver) = channel::<TransportEvent>(20);
    
    // Create shutdown signal for graceful cleanup
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    
    // Create inner structure
    let inner = Arc::new(WebTransportInner {
        endpoint: endpoint.clone(),
        stream_settings: stream_settings.clone(),
        event_sender: event_sender.clone(),
        main_session: Mutex::new(None),
        input_session: Mutex::new(None),
        session_token: session_token.clone(),
        cert_hash: cert_hash.clone(),
        timeout_terminate_request: Mutex::new(None),
        video: Mutex::new(video::WebTransportVideo::new(
            Handle::current(),
            stream_settings.video_supported_formats,
            stream_settings.video_frame_queue_size as usize,
        )),
        audio: Mutex::new(audio::WebTransportAudio::new(
            Handle::current(),
            stream_settings.audio_sample_queue_size as usize,
        )),
        input_channels: Mutex::new(None),
        shutdown: shutdown_tx,
    });
    
    // Construct WebTransport URL
    // Format: https://host:port/path (WebTransport uses HTTPS)
    let webtransport_url = format!(
        "https://{}:{}/webtransport",
        bind_addr.ip(),
        bind_addr.port()
    );
    
    // Construct WebTransport input URL for hybrid mode
    let webtransport_input_url = format!(
        "https://{}:{}/webtransport/input",
        bind_addr.ip(),
        bind_addr.port()
    );
    
    // Send WebTransport setup info to client (with certificate hash)
    if let Err(err) = event_sender
        .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
            StreamServerMessage::Setup {
                ice_servers: vec![], // Not used for WebTransport, but kept for compatibility
                session_token: session_token.clone(),
                webtransport_url: Some(webtransport_url),
                webtransport_input_url: Some(webtransport_input_url),
                cert_hash: Some(cert_hash.clone()),
                available_transports: vec![common::api_bindings::AvailableTransport::WebTransport],
            },
        )))
        .await
    {
        error!("Failed to send WebTransport setup message: {err:?}");
    };
    
    // Spawn session acceptance loop with shutdown signal
    let inner_clone = Arc::downgrade(&inner);
    spawn(async move {
        accept_sessions(endpoint, inner_clone, shutdown_rx).await;
    });
    
    // Spawn packet receiving loop for input channels
    let inner_for_packets = Arc::downgrade(&inner);
    spawn(async move {
        poll_input_packets(inner_for_packets).await;
    });
    
    Ok((
        WebTransportSender {
            inner: inner.clone(),
        },
        WebTransportEvents { event_receiver },
    ))
}

/// Accept incoming WebTransport sessions
async fn accept_sessions(
    endpoint: Arc<Endpoint<wtransport::endpoint::endpoint_side::Server>>,
    inner: Weak<WebTransportInner>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    info!("[WebTransport]: Starting session acceptance loop");
    
    // wtransport 0.6 API: endpoint.accept() returns IncomingSession
    // Then await that to get SessionRequest, then call .accept() to get Connection
    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("[WebTransport]: Shutdown signal received, stopping session acceptance");
                    break;
                }
            }
            // Accept incoming sessions
            incoming_session = endpoint.accept() => {
                // Await to get the SessionRequest
                let session_request = match incoming_session.await {
                    Ok(req) => req,
                    Err(e) => {
                        warn!("[WebTransport]: Failed to get session request: {e:?}");
                        continue;
                    }
                };
                let Some(inner) = inner.upgrade() else {
                    warn!("[WebTransport]: Inner dropped, stopping session acceptance");
                    break;
                };
                
                let inner_clone = inner.clone();
                spawn(async move {
                    if let Err(err) = handle_session_request(session_request, inner_clone).await {
                        warn!("[WebTransport]: Error handling session request: {err:?}");
                    }
                });
            }
        }
    }
    
    // Drop the endpoint Arc reference when loop exits
    // This allows the endpoint to be deallocated when all references are dropped
    drop(endpoint);
    info!("[WebTransport]: Session acceptance loop ended, endpoint reference released");
}

/// Handle an incoming session request (wtransport 0.6 API)
async fn handle_session_request(
    session_request: wtransport::endpoint::SessionRequest,
    inner: Arc<WebTransportInner>,
) -> Result<(), anyhow::Error> {
    let path = session_request.path().to_string();
    let remote_addr = session_request.remote_address();
    info!("[WebTransport]: Session request from {} for path: {}", remote_addr, path);
    
    // Determine session type based on path:
    // - /webtransport or /wt -> main session (video/audio)
    // - /webtransport/input or /wt/input -> input session
    let is_input_session = path.contains("/input");
    
    // Validate the request
    if is_input_session {
        // Input session requires a session token
        if inner.session_token.is_none() {
            warn!("[WebTransport]: Input session requested but no session token configured");
            session_request.forbidden().await;
            return Ok(());
        }
        
        // Check if we already have an input session
        let input_session = inner.input_session.lock().await;
        if input_session.is_some() {
            warn!("[WebTransport]: Rejecting duplicate input session");
            session_request.too_many_requests().await;
            return Ok(());
        }
        drop(input_session);
    } else {
        // Main session - check if we already have one
        let main_session = inner.main_session.lock().await;
        if main_session.is_some() {
            warn!("[WebTransport]: Rejecting duplicate main session");
            session_request.too_many_requests().await;
            return Ok(());
        }
        drop(main_session);
    }
    
    // Accept the connection - returns Connection
    let connection = session_request.accept().await?;
    info!("[WebTransport]: Connection accepted for {} session", if is_input_session { "input" } else { "main" });
    
    if !is_input_session {
        // Main session (video/audio)
        let mut main_session = inner.main_session.lock().await;
        info!("[WebTransport]: Setting as main session (video/audio)");
        let connection_arc = Arc::new(connection);
        
        // wtransport 0.6 uses send_datagram() directly on Connection (synchronous)
        // Video will use connection.send_datagram(data) 
        // Store the connection for video sending
        let mut video = inner.video.lock().await;
        video.set_connection(connection_arc.clone());
        drop(video);
        
        // Set up audio stream (unidirectional, server -> client)
        // wtransport Connection API verified:
        // - open_uni() -> Result<OpeningUniStream, ConnectionError>
        // - Note: Requires double await! connection.open_uni().await?.await?
        // The client will accept this stream using accept_uni()
        // We'll create the stream when audio is configured
        info!("[WebTransport]: Audio stream will be created when audio is configured");
        
        *main_session = Some(connection_arc);
        
        // Notify that stream can start
        if let Err(err) = inner
            .event_sender
            .send(TransportEvent::StartStream {
                settings: inner.stream_settings.clone(),
            })
            .await
        {
            warn!("[WebTransport]: Failed to send StartStream event: {err:?}");
        }
    } else {
        // Input session
        let mut input_session = inner.input_session.lock().await;
        info!("[WebTransport]: Setting up input session");
        let connection_arc = Arc::new(connection);
        
        // Set up input channels
        let channels = channels::WebTransportChannels::new();
        channels.setup_channels_on_session(connection_arc.clone()).await
            .context("Failed to set up input channels")?;
        
        // Store channels
        let mut input_channels = inner.input_channels.lock().await;
        *input_channels = Some(channels);
        drop(input_channels);
        
        // Spawn task to handle incoming bidirectional streams for input
        let inner_clone = Arc::downgrade(&inner);
        let conn_for_streams = connection_arc.clone();
        spawn(async move {
            handle_input_streams(conn_for_streams, inner_clone).await;
        });
        
        *input_session = Some(connection_arc);
        
        // Notify that input is ready
        if let Err(err) = inner
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::InputReady))
            .await
        {
            warn!("[WebTransport]: Failed to send InputReady event: {err:?}");
        }
    }
    
    Ok(())
}

/// Handle incoming bidirectional streams for input channels
async fn handle_input_streams(
    connection: Arc<wtransport::Connection>,
    inner: Weak<WebTransportInner>,
) {
    info!("[WebTransport]: Starting input stream handler");
    
    // Accept incoming bidirectional streams from the client
    // The client will create streams for each input channel type
    // We need to accept them and route to the appropriate channel handler
    
    // wtransport Connection API verified:
    // - accept_bi() -> Result<(SendStream, RecvStream), ConnectionError>
    // - accept_uni() -> Result<RecvStream, ConnectionError>
    
    loop {
        let Some(inner) = inner.upgrade() else {
            break;
        };
        
        // Accept bidirectional stream using verified wtransport API
        match connection.accept_bi().await {
            Ok((send_stream, recv_stream)) => {
                // The client sends channel ID as the first byte
                // For now, we'll determine it from the stream or use a protocol
                // Read channel ID from first message
                let mut channel_id_buf = [0u8; 1];
                let mut recv = recv_stream;
                match recv.read(&mut channel_id_buf).await {
                    Ok(Some(_)) => {
                        let channel_id = channel_id_buf[0];
                        let channels = inner.input_channels.lock().await;
                        if let Some(ref channels) = *channels {
                            channels.handle_incoming_stream(recv, send_stream, channel_id).await;
                        }
                    }
                    _ => {
                        warn!("[WebTransport]: Failed to read channel ID from stream");
                        continue;
                    }
                }
            }
            Err(e) => {
                debug!("[WebTransport]: accept_bi ended: {e:?}");
                break;
            }
        }
    }
    
    info!("[WebTransport]: Input stream handler ended");
}

/// Poll for incoming packets from input channels
async fn poll_input_packets(inner: Weak<WebTransportInner>) {
    loop {
        let Some(inner) = inner.upgrade() else {
            break;
        };
        
        let mut input_channels = inner.input_channels.lock().await;
        if let Some(ref mut channels) = *input_channels {
            // Try to receive a packet (non-blocking)
            if let Some(packet) = channels.try_receive_packet() {
                if let Err(err) = inner
                    .event_sender
                    .send(TransportEvent::RecvPacket(packet))
                    .await
                {
                    warn!("[WebTransport]: Failed to send RecvPacket event: {err:?}");
                }
            }
        }
        
        drop(input_channels);
        
        // Small delay to avoid busy-waiting
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    }
}

pub struct WebTransportEvents {
    event_receiver: Receiver<TransportEvent>,
}

#[async_trait::async_trait]
impl TransportEvents for WebTransportEvents {
    async fn poll_event(&mut self) -> Result<TransportEvent, TransportError> {
        self.event_receiver
            .recv()
            .await
            .ok_or(TransportError::Closed)
    }
}

pub struct WebTransportSender {
    inner: Arc<WebTransportInner>,
}

#[async_trait]
impl TransportSender for WebTransportSender {
    async fn setup_video(&self, setup: VideoSetup) -> i32 {
        let mut video = self.inner.video.lock().await;
        if video.setup(setup).await {
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
        
        // If stream writer not set yet, try to create it from main session
        {
            let writer_guard = audio.stream_writer.lock().await;
            if writer_guard.is_none() {
                drop(writer_guard);
                
                let main_session = self.inner.main_session.lock().await;
                if let Some(ref session) = *main_session {
                    // wtransport Connection.open_uni() verified:
                    // Returns Result<OpeningUniStream, ConnectionError>
                    // Requires double await: connection.open_uni().await?.await?
                    match session.open_uni().await {
                        Ok(opening) => {
                            match opening.await {
                                Ok(send_stream) => {
                                    drop(main_session); // Release lock before setting
                                    audio.set_stream_writer(send_stream);
                                    info!("[WebTransport]: Audio unidirectional stream created");
                                }
                                Err(e) => {
                                    warn!("[WebTransport]: Failed to open audio stream (second await): {e:?}");
                                }
                            }
                        }
                        Err(e) => {
                            warn!("[WebTransport]: Failed to open audio stream: {e:?}");
                        }
                    }
                }
            }
        }
        
        audio.setup(audio_config, stream_config).await
    }

    async fn send_audio_sample(&self, data: &[u8]) -> Result<(), TransportError> {
        let mut audio = self.inner.audio.lock().await;
        audio.send_audio_sample(data).await;
        Ok(())
    }

    async fn send(&self, packet: OutboundPacket) -> Result<(), TransportError> {
        // Send packet via input channels (if available) or main session
        let input_channels = self.inner.input_channels.lock().await;
        if let Some(ref channels) = *input_channels {
            channels.send_packet(packet).await
                .map_err(|e| TransportError::Implementation(e.into()))
        } else {
            // If no input channels, this is likely a stats/general message
            // For now, log and return error - we'll handle this when we implement
            // the full channel routing
            warn!("[WebTransport]: Cannot send packet - no input channels set up");
            Err(TransportError::ChannelClosed)
        }
    }

    async fn on_ipc_message(&self, message: ServerIpcMessage) -> Result<(), TransportError> {
        match message {
            ServerIpcMessage::WebSocket(_) => {
                // WebTransport doesn't use WebSocket signaling
                Ok(())
            }
            ServerIpcMessage::InputJoined => {
                info!("[WebTransport]: Input connection joined - input channels should be ready");
                // Input channels are set up when input session is established
                Ok(())
            }
            ServerIpcMessage::InputWebSocket(_) => {
                // WebTransport doesn't use WebSocket signaling for input
                Ok(())
            }
            ServerIpcMessage::InputDisconnected => {
                info!("[WebTransport]: Input connection disconnected");
                let mut input_channels = self.inner.input_channels.lock().await;
                *input_channels = None;
                Ok(())
            }
            ServerIpcMessage::Init { .. } | ServerIpcMessage::Stop => {
                // Handled elsewhere
                Ok(())
            }
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        info!("[WebTransport]: Closing transport");
        
        // Signal shutdown to the session acceptance loop
        // This will cause the loop to exit and drop its Arc<Endpoint> reference
        let _ = self.inner.shutdown.send(true);
        
        // Close the WebTransport sessions
        if let Some(main) = self.inner.main_session.lock().await.take() {
            main.close(wtransport::VarInt::from_u32(0), b"close");
            info!("[WebTransport]: Main session closed");
        }
        if let Some(input) = self.inner.input_session.lock().await.take() {
            input.close(wtransport::VarInt::from_u32(0), b"close");
            info!("[WebTransport]: Input session closed");
        }
        
        // Give the session acceptance loop time to exit and release the endpoint
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        info!("[WebTransport]: Transport closed");
        Ok(())
    }
}

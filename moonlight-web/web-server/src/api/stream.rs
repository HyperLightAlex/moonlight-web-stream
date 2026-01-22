use std::process::Stdio;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use actix_web::{
    Error, HttpRequest, HttpResponse, get, post, rt as actix_rt,
    web::{Data, Json, Payload},
};
use actix_ws::{Closed, Message, Session};
use common::{
    StreamSettings,
    api_bindings::{
        PostCancelRequest, PostCancelResponse, StreamClientMessage, StreamServerMessage,
    },
    ipc::{ServerIpcMessage, StreamerConfig, StreamerIpcMessage, create_child_ipc},
    serialize_json,
};
use log::{debug, error, info, warn};
use moonlight_common::stream::bindings::SupportedVideoFormats;
use tokio::{process::Command, spawn};

use crate::app::{
    App as WebApp, AppError,
    host::{App, AppId, HostId},
    session::{InputToStreamerMessage, SessionEvent},
    user::AuthenticatedUser,
};

#[get("/host/stream")]
pub async fn start_host(
    web_app: Data<WebApp>,
    mut user: AuthenticatedUser,
    request: HttpRequest,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut stream) = actix_ws::handle(&request, payload)?;

    let client_unique_id = user.host_unique_id().await?;

    let web_app = web_app.clone();
    actix_rt::spawn(async move {
        // -- Init and Configure
        let message;
        loop {
            message = match stream.recv().await {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Binary(_))) => {
                    return;
                }
                Some(Ok(_)) => continue,
                Some(Err(_)) => {
                    return;
                }
                None => {
                    return;
                }
            };
            break;
        }

        let message = match serde_json::from_str::<StreamClientMessage>(&message) {
            Ok(value) => value,
            Err(_) => {
                return;
            }
        };

        let StreamClientMessage::Init {
            host_id,
            app_id,
            bitrate,
            packet_size,
            fps,
            width,
            height,
            video_frame_queue_size,
            play_audio_local,
            audio_sample_queue_size,
            video_supported_formats,
            video_colorspace,
            video_color_range_full,
            hybrid_mode,
        } = message
        else {
            let _ = session.close(None).await;

            warn!("WebSocket didn't send init as first message, closing it");
            return;
        };

        let host_id = HostId(host_id);
        let app_id = AppId(app_id);

        // Generate session token for hybrid mode and register with session manager
        let (session_token, hybrid_session_id, input_msg_rx) = if hybrid_mode {
            let token = uuid::Uuid::new_v4().to_string();
            let (session_id, _, input_rx) = web_app
                .session_manager()
                .register_session(token.clone())
                .await;
            info!(
                "[Stream]: Hybrid mode enabled, session_id: {}, token: {}",
                session_id, token
            );
            (Some(token), Some(session_id), Some(input_rx))
        } else {
            (None, None, None)
        };

        let stream_settings = StreamSettings {
            bitrate,
            packet_size,
            fps,
            width,
            height,
            video_frame_queue_size,
            audio_sample_queue_size,
            play_audio_local,
            video_supported_formats: SupportedVideoFormats::from_bits(video_supported_formats)
                .unwrap_or_else(|| {
                    warn!("[Stream]: Received invalid supported video formats");
                    SupportedVideoFormats::H264
                }),
            video_colorspace: video_colorspace.into(),
            video_color_range_full,
            hybrid_mode,
        };

        // -- Collect host data
        let mut host = match user.host(host_id).await {
            Ok(host) => host,
            Err(AppError::HostNotFound) => {
                let _ = send_ws_message(&mut session, StreamServerMessage::HostNotFound).await;
                let _ = session.close(None).await;
                return;
            }
            Err(err) => {
                warn!("failed to start stream for host {host_id:?} (at host): {err:?}");

                let _ =
                    send_ws_message(&mut session, StreamServerMessage::InternalServerError).await;
                let _ = session.close(None).await;
                return;
            }
        };

        // When embedded in Fuji, look up the game from Fuji's library
        // The app_id is a hash of the game title (generated in /api/apps)
        // When not embedded, use Sunshine's app list directly
        let app: App;
        let mut fuji_game_id_early: Option<String> = None;
        
        {
            use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            
            if is_embedded_in_fuji().await {
                info!("[Stream]: Looking up game from Fuji's library (app_id={})", app_id.0);
                
                match fuji_client().get_games(None, None).await {
                    Ok(fuji_games) => {
                        // Find the game by matching the hashed title
                        let found_game = fuji_games.games.into_iter().find(|game| {
                            let mut hasher = DefaultHasher::new();
                            game.title.hash(&mut hasher);
                            let hashed_id = (hasher.finish() & 0x7FFFFFFF) as u32;
                            hashed_id == app_id.0
                        });
                        
                        if let Some(game) = found_game {
                            info!("[Stream]: Found Fuji game: '{}' (id={})", game.title, game.id);
                            fuji_game_id_early = Some(game.id.clone());
                            
                            // Create a synthetic App for the rest of the flow
                            app = App {
                                id: AppId(app_id.0),
                                title: game.title,
                                is_hdr_supported: false,
                            };
                        } else {
                            warn!("[Stream]: Game not found in Fuji library for app_id={}", app_id.0);
                            let _ = send_ws_message(&mut session, StreamServerMessage::AppNotFound).await;
                            let _ = session.close(None).await;
                            return;
                        }
                    }
                    Err(e) => {
                        warn!("[Stream]: Failed to get Fuji games: {:?}, falling back to Sunshine", e);
                        // Fall through to Sunshine lookup below
                        let apps = match host.list_apps(&mut user).await {
                            Ok(apps) => apps,
                            Err(err) => {
                                warn!("failed to start stream for host {host_id:?} (at list_apps): {err:?}");
                                let _ = send_ws_message(&mut session, StreamServerMessage::InternalServerError).await;
                                let _ = session.close(None).await;
                                return;
                            }
                        };
                        
                        let Some(found_app) = apps.into_iter().find(|a| a.id == app_id) else {
                            warn!("failed to start stream for host {host_id:?} because the app couldn't be found!");
                            let _ = send_ws_message(&mut session, StreamServerMessage::AppNotFound).await;
                            let _ = session.close(None).await;
                            return;
                        };
                        app = found_app;
                    }
                }
            } else {
                // Not embedded - use Sunshine's app list directly
                let apps = match host.list_apps(&mut user).await {
                    Ok(apps) => apps,
                    Err(err) => {
                        warn!("failed to start stream for host {host_id:?} (at list_apps): {err:?}");
                        let _ = send_ws_message(&mut session, StreamServerMessage::InternalServerError).await;
                        let _ = session.close(None).await;
                        return;
                    }
                };

                let Some(found_app) = apps.into_iter().find(|a| a.id == app_id) else {
                    warn!("failed to start stream for host {host_id:?} because the app couldn't be found!");
                    let _ = send_ws_message(&mut session, StreamServerMessage::AppNotFound).await;
                    let _ = session.close(None).await;
                    return;
                };
                app = found_app;
            }
        }

        let (address, mut http_port) = match host.address_port(&mut user).await {
            Ok(address_port) => address_port,
            Err(err) => {
                warn!("failed to start stream for host {host_id:?} (at get address_port): {err:?}");

                let _ =
                    send_ws_message(&mut session, StreamServerMessage::InternalServerError).await;
                let _ = session.close(None).await;
                return;
            }
        };

        // When embedded in Fuji, get fresh Sunshine port (it may have changed since pairing)
        {
            use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};
            
            if is_embedded_in_fuji().await {
                match fuji_client().get_sunshine_ports().await {
                    Ok((fresh_http_port, _)) => {
                        if fresh_http_port != http_port {
                            info!("[Stream]: Sunshine HTTP port changed {} -> {}", http_port, fresh_http_port);
                        }
                        http_port = fresh_http_port;
                    }
                    Err(e) => {
                        warn!("[Stream]: Failed to get fresh Sunshine ports: {:?}, using stored {}", e, http_port);
                    }
                }
            }
        }

        let pair_info = match host.pair_info(&mut user).await {
            Ok(pair_info) => pair_info,
            Err(err) => {
                warn!("failed to start stream for host {host_id:?} (at get pair_info): {err:?}");

                let _ =
                    send_ws_message(&mut session, StreamServerMessage::InternalServerError).await;
                let _ = session.close(None).await;
                return;
            }
        };

        // Store app title and id before moving app into message
        let app_title = app.title.clone();
        let requested_app_id = app.id.0;

        // -- Stream Orchestration via Fuji (when embedded)
        // Fuji handles all the decision-making:
        // 1. Check if a different game is running
        // 2. Cancel the previous game if needed  
        // 3. Return "launch" or "resume" action AND the Sunshine app_index
        // The streamer will then execute exactly what Fuji decided.
        let mut launch_mode: Option<String> = None;
        let mut sunshine_app_index: Option<u32> = None; // The actual Sunshine app ID to use
        let mut fuji_game_id: Option<String> = fuji_game_id_early; // Use ID found during app lookup
        
        {
            use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};

            info!("[Stream]: Checking if embedded in Fuji for stream orchestration...");
            let embedded = is_embedded_in_fuji().await;
            info!("[Stream]: is_embedded_in_fuji() = {}", embedded);

            if embedded {
                info!("[Stream]: Embedded in Fuji, using stream orchestration");

                // Use the game ID found during app lookup (or find it now as fallback)
                if fuji_game_id.is_none() {
                    info!("[Stream]: Looking up Fuji game ID for '{}'...", app_title);
                    if let Ok(fuji_games) = fuji_client().get_games(None, None).await {
                        if let Some(game) = fuji_games.games.iter().find(|g| {
                            g.title.to_lowercase() == app_title.to_lowercase()
                        }) {
                            fuji_game_id = Some(game.id.clone());
                            info!("[Stream]: Found Fuji game '{}' (id={})", game.title, game.id);
                        }
                    }
                }

                if let Some(ref game_id) = fuji_game_id {
                    info!("[Stream]: Using Fuji game ID: {}", game_id);

                    // Call Fuji's stream orchestration endpoint
                    // This handles cancel if needed and returns the action + Sunshine app_index
                    let _ = send_ws_message(
                        &mut session,
                        StreamServerMessage::StageStarting {
                            stage: "Preparing Stream".to_string(),
                        },
                    )
                    .await;

                    match fuji_client().stream_launch(game_id).await {
                        Ok(response) => {
                            if response.success {
                                // Get the Sunshine app_index - this is CRITICAL for streaming
                                sunshine_app_index = response.app_index;
                                info!("[Stream]: Fuji returned Sunshine app_index: {:?}", sunshine_app_index);
                                
                                if let Some(action) = &response.action {
                                    launch_mode = Some(action.clone());
                                    info!(
                                        "[Stream]: Fuji orchestration decided: action={}, appIndex={:?}, cancelledPrevious={:?}",
                                        action,
                                        sunshine_app_index,
                                        response.cancelled_previous
                                    );

                                    if response.cancelled_previous.unwrap_or(false) {
                                        let _ = send_ws_message(
                                            &mut session,
                                            StreamServerMessage::StageComplete {
                                                stage: "Stopped Previous Game".to_string(),
                                            },
                                        )
                                        .await;
                                    }
                                }
                            } else {
                                warn!("[Stream]: Fuji orchestration failed: {:?}", response.error);
                            }
                        }
                        Err(e) => {
                            warn!("[Stream]: Fuji stream orchestration failed: {:?}, streamer will decide", e);
                        }
                    }
                } else {
                    warn!("[Stream]: No Fuji game ID available, streamer will decide launch mode");
                }

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::StageComplete {
                        stage: "Preparing Stream".to_string(),
                    },
                )
                .await;
            } else {
                info!("[Stream]: NOT embedded in Fuji, streamer will decide launch mode");
            }
        }
        
        // Determine the actual app_id to send to the streamer
        // When embedded in Fuji, use the Sunshine app_index from orchestration
        // Otherwise, use the original app_id from the request
        let streamer_app_id = sunshine_app_index.unwrap_or(app_id.0);
        info!("[Stream]: Using app_id {} for streamer (original: {}, sunshine_app_index: {:?})", 
              streamer_app_id, app_id.0, sunshine_app_index);

        // -- Send App info
        let _ = send_ws_message(
            &mut session,
            StreamServerMessage::UpdateApp { app: app.into() },
        )
        .await;

        // -- Starting stage: launch streamer
        let _ = send_ws_message(
            &mut session,
            StreamServerMessage::StageStarting {
                stage: "Launch Streamer".to_string(),
            },
        )
        .await;

        // Clean up any orphaned streamer processes before starting a new one
        // This handles edge cases where previous streamers weren't properly killed
        {
            use crate::app::streamer_manager::streamer_manager;
            
            info!("[Stream]: Cleaning up orphaned streamers before starting new session...");
            streamer_manager().cleanup_before_new_session().await;
        }

        // Spawn child
        // On Windows, use CREATE_NO_WINDOW to prevent console window from appearing
        #[cfg(windows)]
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        
        let mut cmd = Command::new(&web_app.config().streamer_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        
        let (mut child, stdin, stdout) = match cmd.spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.take()
                    && let Some(stdout) = child.stdout.take()
                {
                    (child, stdin, stdout)
                } else {
                    error!("[Stream]: streamer process didn't include a stdin or stdout");

                    let _ = send_ws_message(&mut session, StreamServerMessage::InternalServerError)
                        .await;
                    let _ = session.close(None).await;

                    if let Err(err) = child.kill().await {
                        warn!("[Stream]: failed to kill child: {err:?}");
                    }

                    return;
                }
            }
            Err(err) => {
                error!("[Stream]: failed to spawn streamer process: {err:?}");

                let _ =
                    send_ws_message(&mut session, StreamServerMessage::InternalServerError).await;
                let _ = session.close(None).await;
                return;
            }
        };

        // Register the streamer process for tracking
        let child_pid = child.id();
        {
            use crate::app::streamer_manager::streamer_manager;
            streamer_manager().register(&child, hybrid_session_id.clone()).await;
        }

        // Create ipc
        let (mut ipc_sender, mut ipc_receiver) = create_child_ipc::<
            ServerIpcMessage,
            StreamerIpcMessage,
        >(
            "Streamer", stdin, stdout, child.stderr.take()
        )
        .await;

        // Clone web_app for use in spawned task (for session cleanup)
        let web_app_cleanup = web_app.clone();
        let hybrid_session_id_cleanup = hybrid_session_id.clone();

        // Clone IPC sender for input message forwarding
        let mut ipc_sender_for_input = ipc_sender.clone();

        // Spawn task to forward input messages to streamer (if hybrid mode)
        if let Some(mut input_rx) = input_msg_rx {
            let hybrid_session_id_input = hybrid_session_id.clone();
            info!("[Stream]: Starting input message forwarder for session {:?}", hybrid_session_id_input);
            spawn(async move {
                info!("[Stream]: Input message forwarder task started");
                while let Some(msg) = input_rx.recv().await {
                    match msg {
                        InputToStreamerMessage::Joined => {
                            info!("[Stream]: >>> Input connection joined, forwarding to streamer via IPC");
                            ipc_sender_for_input.send(ServerIpcMessage::InputJoined).await;
                        }
                        InputToStreamerMessage::Signaling(signaling) => {
                            info!("[Stream]: >>> Forwarding input signaling to streamer: {:?}", signaling);
                            ipc_sender_for_input
                                .send(ServerIpcMessage::InputWebSocket(signaling))
                                .await;
                        }
                        InputToStreamerMessage::Disconnected => {
                            info!("[Stream]: >>> Input connection disconnected, notifying streamer");
                            ipc_sender_for_input
                                .send(ServerIpcMessage::InputDisconnected)
                                .await;
                        }
                    }
                }
                info!(
                    "[Stream]: Input message receiver closed for session {:?}",
                    hybrid_session_id_input
                );
            });
        } else {
            info!("[Stream]: No input_msg_rx, hybrid mode not enabled or receiver not created");
        }

        // Clone for forwarding input signaling from streamer
        let web_app_for_input = web_app.clone();
        let hybrid_session_id_for_input = hybrid_session_id.clone();

        // Set up session event receiver for hybrid mode
        let (session_event_tx, mut session_event_rx) = tokio::sync::mpsc::channel::<SessionEvent>(32);
        if let Some(ref session_id) = hybrid_session_id_for_input {
            web_app_for_input
                .session_manager()
                .set_primary_notify(session_id, session_event_tx)
                .await;
        }

        // Redirect ipc message into ws, also handle session events
        spawn(async move {
            loop {
                tokio::select! {
                    ipc_msg = ipc_receiver.recv() => {
                        match ipc_msg {
                            Some(StreamerIpcMessage::WebSocket(message)) => {
                                if let Err(Closed) = send_ws_message(&mut session, message).await {
                                    warn!(
                                        "[Ipc]: Tried to send a ws message but the socket is already closed"
                                    );
                                    break;
                                }
                            }
                            Some(StreamerIpcMessage::InputSignaling(signaling)) => {
                                // Forward input signaling to input client via session manager
                                if let Some(ref session_id) = hybrid_session_id_for_input {
                                    debug!("[Ipc]: Forwarding input signaling from streamer to input client");
                                    web_app_for_input
                                        .session_manager()
                                        .send_to_input(
                                            session_id,
                                            crate::app::session::StreamerToInputMessage::Signaling(signaling),
                                        )
                                        .await;
                                }
                            }
                            Some(StreamerIpcMessage::InputReady) => {
                                if let Some(ref session_id) = hybrid_session_id_for_input {
                                    debug!("[Ipc]: Input peer ready, notifying input client");
                                    web_app_for_input
                                        .session_manager()
                                        .send_to_input(
                                            session_id,
                                            crate::app::session::StreamerToInputMessage::Ready,
                                        )
                                        .await;
                                }
                            }
                            Some(StreamerIpcMessage::Stop) => {
                                debug!("[Ipc]: ipc receiver stopped by streamer");
                                break;
                            }
                            None => {
                                debug!("[Ipc]: ipc receiver channel closed");
                                break;
                            }
                        }
                    }
                    session_event = session_event_rx.recv() => {
                        match session_event {
                            Some(SessionEvent::InputJoined) => {
                                debug!("[Stream]: Input connection joined");
                                if let Err(Closed) = send_ws_message(
                                    &mut session,
                                    StreamServerMessage::InputJoined,
                                ).await {
                                    warn!("[Stream]: Failed to send InputJoined to client");
                                    break;
                                }
                            }
                            Some(SessionEvent::InputDisconnected) => {
                                debug!("[Stream]: Input connection disconnected");
                                if let Err(Closed) = send_ws_message(
                                    &mut session,
                                    StreamServerMessage::InputDisconnected,
                                ).await {
                                    warn!("[Stream]: Failed to send InputDisconnected to client");
                                    break;
                                }
                            }
                            Some(SessionEvent::ReconnectionTokenAvailable(token)) => {
                                debug!("[Stream]: Reconnection token available: {}", token);
                                if let Err(Closed) = send_ws_message(
                                    &mut session,
                                    StreamServerMessage::ReconnectionTokenAvailable {
                                        session_token: token,
                                    },
                                ).await {
                                    warn!("[Stream]: Failed to send ReconnectionTokenAvailable to client");
                                    break;
                                }
                            }
                            Some(SessionEvent::PrimaryDisconnected) => {
                                // This shouldn't happen as we ARE the primary
                                warn!("[Stream]: Received unexpected PrimaryDisconnected event");
                            }
                            None => {
                                // Session event channel closed, continue with IPC only
                                debug!("[Stream]: Session event channel closed");
                            }
                        }
                    }
                }
            }
            info!("[Ipc]: ipc receiver loop ended");

            // Clean up hybrid session if applicable
            if let Some(session_id) = hybrid_session_id_cleanup {
                web_app_cleanup
                    .session_manager()
                    .primary_disconnected(&session_id)
                    .await;
            }

            // close the websocket when the streamer crashed / disconnected / whatever
            if let Err(err) = session.close(None).await {
                warn!("failed to close streamer web socket: {err}");
            }

            // Note: Don't stop the game here - let Sunshine keep it running
            // The game should only be stopped explicitly via /host/cancel or when switching games
            // This allows the user to reconnect to a running game

            // kill the streamer and unregister from manager
            {
                use crate::app::streamer_manager::streamer_manager;
                
                if let Err(err) = child.kill().await {
                    warn!("failed to kill streamer child: {err}");
                }
                
                // Unregister from process manager
                if let Some(pid) = child_pid {
                    streamer_manager().unregister(pid).await;
                    info!("[Stream]: Unregistered streamer process PID {}", pid);
                }
            }
        });

        // Send init into ipc
        // launch_mode tells the streamer whether to call host_launch or host_resume
        // If None, the streamer will check current_game itself (legacy behavior)
        ipc_sender
            .send(ServerIpcMessage::Init {
                config: StreamerConfig {
                    webrtc: web_app.config().webrtc.clone(),
                    log_level: web_app.config().log.level_filter,
                },
                stream_settings,
                host_address: address,
                host_http_port: http_port,
                client_unique_id: Some(client_unique_id),
                client_private_key: pair_info.client_private_key,
                client_certificate: pair_info.client_certificate,
                server_certificate: pair_info.server_certificate,
                app_id: streamer_app_id,
                session_token,
                launch_mode,
            })
            .await;

        // Redirect ws message into ipc
        while let Some(Ok(Message::Text(text))) = stream.recv().await {
            let Ok(message) = serde_json::from_str::<StreamClientMessage>(&text) else {
                warn!("[Stream]: failed to deserialize from json");
                return;
            };

            ipc_sender.send(ServerIpcMessage::WebSocket(message)).await;
        }

        // Primary WebSocket closed - client disconnected
        // Send stop signal to streamer so it exits immediately
        // This triggers Sunshine's "paused" state faster (instead of waiting for timeout)
        // The game keeps running - only the stream is stopped
        info!("[Stream]: Primary WebSocket closed, sending stop signal to streamer");
        ipc_sender.send(ServerIpcMessage::Stop).await;
        info!("[Stream]: Stop signal sent, game will keep running for potential reconnection");
    });

    Ok(response)
}

async fn send_ws_message(sender: &mut Session, message: StreamServerMessage) -> Result<(), Closed> {
    let Some(json) = serialize_json(&message) else {
        return Ok(());
    };

    sender.text(json).await
}

#[post("/host/cancel")]
pub async fn cancel_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PostCancelRequest>,
) -> Result<Json<PostCancelResponse>, AppError> {
    use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};

    let host_id = HostId(request.host_id);

    let mut host = user.host(host_id).await?;

    // When embedded in Fuji, use the stream orchestration stop endpoint
    // This properly stops both the game and the Sunshine stream
    if is_embedded_in_fuji().await {
        info!("[Stream]: Embedded in Fuji, stopping stream via orchestration API");

        if let Err(e) = fuji_client().stream_stop().await {
            warn!("[Stream]: Fuji stream stop failed: {:?}, falling back to Sunshine cancel", e);
            // Fall back to direct Sunshine cancel
            host.cancel_app(&mut user).await?;
        }
    } else {
        // Not embedded - just cancel via Sunshine directly
        host.cancel_app(&mut user).await?;
    }

    Ok(Json(PostCancelResponse { success: true }))
}

/// Session status response for clients
#[derive(serde::Serialize)]
pub struct SessionStatusResponse {
    /// Whether there's an active game running (even if not currently streaming)
    pub has_running_game: bool,
    /// The Sunshine app ID of the running game (0 if none)
    pub current_game_id: u32,
    /// Game information if available from Fuji
    pub game: Option<SessionGameInfo>,
}

#[derive(serde::Serialize)]
pub struct SessionGameInfo {
    pub id: String,
    pub title: String,
}

/// GET /api/session
/// Returns current session/game status for the client
/// 
/// This allows the client to:
/// - Know if there's a running game to resume
/// - Show appropriate UI (Resume/Switch/Quit options)
#[get("/session")]
pub async fn get_session(
    mut user: AuthenticatedUser,
) -> Result<Json<SessionStatusResponse>, AppError> {
    use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};

    info!("[Session]: Checking session status...");

    // Try to get current game from Fuji first (more reliable for Fuji-managed games)
    // Fuji tracks actual game process state, not just streaming state
    if is_embedded_in_fuji().await {
        info!("[Session]: Embedded in Fuji, checking Fuji session state");
        if let Ok(status) = fuji_client().get_status().await {
            info!("[Session]: Fuji status - streaming.active={}, currentGame={:?}", 
                status.streaming.active, 
                status.streaming.current_game.as_ref().map(|g| &g.title));
            
            if status.streaming.active {
                if let Some(game) = status.streaming.current_game {
                    info!("[Session]: Fuji reports active game: {}", game.title);
                    return Ok(Json(SessionStatusResponse {
                        has_running_game: true,
                        current_game_id: 0, // Fuji doesn't use Sunshine app IDs
                        game: Some(SessionGameInfo {
                            id: game.id,
                            title: game.title,
                        }),
                    }));
                }
            }
        } else {
            warn!("[Session]: Failed to get Fuji status");
        }
    }

    // Fall back to checking Sunshine's current_game via host info
    // Note: Sunshine's current_game may be 0 even if game is running (when stream is paused)
    info!("[Session]: Checking Sunshine current_game as fallback");
    let hosts = user.hosts().await?;
    
    for mut host in hosts {
        if let Ok(info) = host.detailed_host(&mut user).await {
            info!("[Session]: Sunshine host info - current_game={}", info.current_game);
            if info.current_game != 0 {
                info!("[Session]: Sunshine reports active game ID: {}", info.current_game);
                return Ok(Json(SessionStatusResponse {
                    has_running_game: true,
                    current_game_id: info.current_game,
                    game: None, // We don't have the title from Sunshine directly
                }));
            }
        } else {
            warn!("[Session]: Failed to get Sunshine host info");
        }
    }

    info!("[Session]: No active game detected");
    Ok(Json(SessionStatusResponse {
        has_running_game: false,
        current_game_id: 0,
        game: None,
    }))
}

/// Response for pause endpoint
#[derive(serde::Serialize)]
pub struct SessionPauseResponse {
    pub success: bool,
    pub message: String,
}

/// POST /api/session/pause
/// Pauses the current streaming session without stopping the game.
/// 
/// This cleanly disconnects the streamer from Sunshine, which causes Sunshine
/// to enter a "paused" state. The game continues running and can be resumed
/// by starting a new stream for the same game.
/// 
/// Use this when:
/// - User closes the streaming WebView
/// - User wants to temporarily disconnect but keep the game running
/// 
/// For stopping the game entirely, use POST /api/cancel or POST /api/session/end
#[post("/session/pause")]
pub async fn pause_session(
    _user: AuthenticatedUser,
) -> Result<Json<SessionPauseResponse>, AppError> {
    use crate::app::streamer_manager::streamer_manager;

    info!("[Session]: Pause requested - killing streamer to pause stream (game will keep running)");

    // Kill all tracked streamer processes
    // This disconnects from Sunshine, causing it to enter "paused" state
    // The game keeps running and can be resumed later
    streamer_manager().kill_all_tracked().await;

    // Also clean up any orphaned streamers
    streamer_manager().kill_orphaned_streamers().await;

    info!("[Session]: Stream paused - Sunshine should now be in paused state");

    Ok(Json(SessionPauseResponse {
        success: true,
        message: "Stream paused. Game is still running and can be resumed.".to_string(),
    }))
}

/// Request for ending session
#[derive(serde::Deserialize)]
pub struct SessionEndRequest {
    /// Host ID (optional, uses first host if not provided)
    pub host_id: Option<u32>,
}

/// Response for end session endpoint
#[derive(serde::Serialize)]
pub struct SessionEndResponse {
    pub success: bool,
    pub message: String,
}

/// POST /api/session/end
/// Ends the current streaming session AND stops the running game.
/// 
/// This is a convenience endpoint that:
/// 1. Kills the streamer process
/// 2. Calls Sunshine cancel to stop the stream
/// 3. If embedded in Fuji, stops the game via internal API
/// 
/// Use this when the user wants to completely quit (not just pause).
#[post("/session/end")]
pub async fn end_session(
    mut user: AuthenticatedUser,
    body: Option<Json<SessionEndRequest>>,
) -> Result<Json<SessionEndResponse>, AppError> {
    use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};
    use crate::app::streamer_manager::streamer_manager;

    info!("[Session]: End session requested - stopping stream and game");

    // First kill all streamer processes
    streamer_manager().kill_all_tracked().await;
    streamer_manager().kill_orphaned_streamers().await;

    // End Fuji session if embedded (stops the game process)
    if is_embedded_in_fuji().await {
        info!("[Session]: Ending Fuji session to stop game");
        if let Err(e) = fuji_client().end_session().await {
            warn!("[Session]: Failed to end Fuji session: {:?}", e);
        }
    }

    // Cancel via Sunshine (resets current_game to 0)
    let hosts = user.hosts().await?;
    
    for mut host in hosts {
        // Try to get host_id from request, or just cancel on all hosts
        if let Some(ref req) = body {
            if let Some(req_host_id) = req.host_id {
                if host.id().0 != req_host_id {
                    continue;
                }
            }
        }
        
        if let Err(e) = host.cancel_app(&mut user).await {
            warn!("[Session]: Failed to cancel on host: {:?}", e);
        } else {
            info!("[Session]: Cancelled stream on host");
        }
    }

    info!("[Session]: Session ended - stream stopped and game closed");

    Ok(Json(SessionEndResponse {
        success: true,
        message: "Session ended. Stream stopped and game closed.".to_string(),
    }))
}

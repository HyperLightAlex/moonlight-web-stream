//! Input-only WebSocket endpoint for hybrid streaming mode
//!
//! This endpoint handles the native input connection that joins an existing
//! hybrid streaming session using a session token.

use actix_web::{Error, HttpRequest, HttpResponse, get, rt as actix_rt, web::{Data, Payload}};
use actix_ws::{Closed, Message, Session};
use common::{
    api_bindings::{InputClientMessage, InputErrorCode, InputServerMessage},
    serialize_json,
};
use log::{debug, error, info, warn};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::app::{
    App,
    session::{InputToStreamerMessage, SessionError, StreamerToInputMessage},
};

/// WebSocket endpoint for input-only connections in hybrid mode
#[get("/host/input")]
pub async fn input_connect(
    web_app: Data<App>,
    request: HttpRequest,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut stream) = actix_ws::handle(&request, payload)?;

    let web_app = web_app.clone();
    actix_rt::spawn(async move {
        // -- Wait for Join message
        let join_message = loop {
            let message = match stream.recv().await {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Binary(_))) => {
                    warn!("[Input]: Received binary message before join, closing");
                    let _ = session.close(None).await;
                    return;
                }
                Some(Ok(Message::Ping(data))) => {
                    let _ = session.pong(&data).await;
                    continue;
                }
                Some(Ok(_)) => continue,
                Some(Err(err)) => {
                    warn!("[Input]: WebSocket error before join: {err:?}");
                    return;
                }
                None => {
                    debug!("[Input]: WebSocket closed before join");
                    return;
                }
            };
            break message;
        };

        let join_message = match serde_json::from_str::<InputClientMessage>(&join_message) {
            Ok(value) => value,
            Err(err) => {
                warn!("[Input]: Failed to parse join message: {err:?}");
                let _ = send_error(
                    &mut session,
                    InputErrorCode::TokenInvalid,
                    "Invalid message format",
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };

        let session_token = match join_message {
            InputClientMessage::Join { session_token } => session_token,
            InputClientMessage::WebRtc(_) => {
                warn!("[Input]: Expected Join message but got WebRtc");
                let _ = send_error(
                    &mut session,
                    InputErrorCode::TokenInvalid,
                    "Expected Join message",
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };

        info!("[Input]: Received join request with token: {}", session_token);

        // -- Validate token and claim session
        let (session_id, input_to_streamer_tx, streamer_to_input_rx) = match web_app
            .session_manager()
            .claim_session(&session_token)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                let (code, message) = match err {
                    SessionError::TokenExpired => {
                        (InputErrorCode::TokenExpired, "Session token has expired")
                    }
                    SessionError::TokenInvalid => {
                        (InputErrorCode::TokenInvalid, "Invalid session token")
                    }
                    SessionError::SessionNotFound => {
                        (InputErrorCode::SessionNotFound, "No session found for token")
                    }
                    SessionError::InputAlreadyConnected => (
                        InputErrorCode::InputAlreadyConnected,
                        "Input connection already established",
                    ),
                    SessionError::PrimaryDisconnected => (
                        InputErrorCode::SessionNotFound,
                        "Primary connection has disconnected",
                    ),
                    SessionError::SessionShuttingDown => (
                        InputErrorCode::SessionNotFound,
                        "Session is shutting down",
                    ),
                };
                warn!("[Input]: Token validation failed: {}", message);
                let _ = send_error(&mut session, code, message).await;
                let _ = session.close(None).await;
                return;
            }
        };

        info!(
            "[Input]: Session {} claimed successfully, sending accepted",
            session_id
        );

        // -- Send Accepted with ICE servers
        let ice_servers = web_app.config().webrtc.ice_servers.clone();
        if let Err(Closed) = send_message(
            &mut session,
            InputServerMessage::Accepted { ice_servers },
        )
        .await
        {
            warn!("[Input]: Failed to send Accepted message, connection closed");
            return;
        }

        // Notify streamer that input has joined
        if let Err(err) = input_to_streamer_tx
            .send(InputToStreamerMessage::Joined)
            .await
        {
            error!("[Input]: Failed to notify streamer of join: {err:?}");
            let _ = session.close(None).await;
            return;
        }

        // -- Handle bidirectional message flow
        handle_input_session(
            session_id.clone(),
            session,
            stream,
            input_to_streamer_tx,
            streamer_to_input_rx,
            web_app,
        )
        .await;

        info!("[Input]: Session {} input connection closed", session_id);
    });

    Ok(response)
}

/// Handle the input session after successful join
async fn handle_input_session(
    session_id: String,
    mut ws_session: Session,
    mut ws_stream: actix_ws::MessageStream,
    input_to_streamer_tx: Sender<InputToStreamerMessage>,
    mut streamer_to_input_rx: Receiver<StreamerToInputMessage>,
    web_app: Data<App>,
) {
    loop {
        tokio::select! {
            // Messages from WebSocket (input client)
            ws_msg = ws_stream.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<InputClientMessage>(&text) {
                            Ok(InputClientMessage::WebRtc(signaling)) => {
                                info!("[Input]: >>> Received signaling from client: {:?}", signaling);
                                info!("[Input]: >>> Sending to input_to_streamer_tx channel");
                                if let Err(err) = input_to_streamer_tx
                                    .send(InputToStreamerMessage::Signaling(signaling))
                                    .await
                                {
                                    warn!("[Input]: Failed to forward signaling to streamer: {err:?}");
                                    break;
                                }
                                info!("[Input]: >>> Signaling sent successfully to channel");
                            }
                            Ok(InputClientMessage::Join { .. }) => {
                                warn!("[Input]: Received unexpected Join message after session established");
                            }
                            Err(err) => {
                                warn!("[Input]: Failed to parse message: {err:?}");
                            }
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // Binary data might be input data in the future
                        debug!("[Input]: Received {} bytes of binary data", data.len());
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = ws_session.pong(&data).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => {
                        debug!("[Input]: Received close from client");
                        break;
                    }
                    Some(Ok(Message::Continuation(_))) => {}
                    Some(Ok(Message::Nop)) => {}
                    Some(Err(err)) => {
                        warn!("[Input]: WebSocket error: {err:?}");
                        break;
                    }
                    None => {
                        debug!("[Input]: WebSocket stream ended");
                        break;
                    }
                }
            }

            // Messages from streamer (via session manager)
            streamer_msg = streamer_to_input_rx.recv() => {
                match streamer_msg {
                    Some(StreamerToInputMessage::Signaling(signaling)) => {
                        debug!("[Input]: Forwarding signaling to client");
                        if let Err(Closed) = send_message(
                            &mut ws_session,
                            InputServerMessage::WebRtc(signaling),
                        ).await {
                            warn!("[Input]: Failed to send signaling, connection closed");
                            break;
                        }
                    }
                    Some(StreamerToInputMessage::Ready) => {
                        debug!("[Input]: Streamer signaled input peer ready");
                        // Could send a status update to the client if needed
                    }
                    None => {
                        // Streamer channel closed (primary disconnected)
                        info!("[Input]: Streamer channel closed, notifying client");
                        let _ = send_message(
                            &mut ws_session,
                            InputServerMessage::PrimaryDisconnected,
                        ).await;
                        break;
                    }
                }
            }
        }
    }

    // Notify streamer that input disconnected
    let _ = input_to_streamer_tx
        .send(InputToStreamerMessage::Disconnected)
        .await;

    // Clean up session
    web_app
        .session_manager()
        .input_disconnected(&session_id)
        .await;

    // Close WebSocket
    let _ = ws_session.close(None).await;
}

async fn send_message(session: &mut Session, message: InputServerMessage) -> Result<(), Closed> {
    let Some(json) = serialize_json(&message) else {
        return Ok(());
    };
    session.text(json).await
}

async fn send_error(session: &mut Session, code: InputErrorCode, message: &str) -> Result<(), Closed> {
    send_message(
        session,
        InputServerMessage::Error {
            code,
            message: message.to_string(),
        },
    )
    .await
}

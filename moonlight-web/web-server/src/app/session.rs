//! Hybrid Streaming Session Manager
//!
//! Manages sessions for hybrid streaming mode where a WebView handles video/audio
//! and a native client handles input via separate WebRTC connections.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use common::api_bindings::StreamSignalingMessage;
use log::{debug, info, warn};
use tokio::{
    spawn,
    sync::{Mutex, mpsc::{Receiver, Sender, channel}},
    time::interval,
};

/// Duration after which a session token expires if input connection doesn't join
pub const TOKEN_EXPIRATION_SECS: u64 = 30;

/// Interval for cleaning up expired sessions
pub const CLEANUP_INTERVAL_SECS: u64 = 10;

/// Timeout for WebRTC peer connection establishment after input joins
pub const INPUT_PEER_TIMEOUT_SECS: u64 = 30;

/// Unique identifier for a hybrid streaming session
pub type SessionId = String;

/// Token used by the input connection to join a session
pub type SessionToken = String;

/// Represents a hybrid streaming session
#[derive(Debug)]
pub struct HybridSession {
    /// Unique session identifier
    pub id: SessionId,
    /// Token for input connection to join (consumed after use)
    pub token: Option<SessionToken>,
    /// When the token expires
    pub token_expires_at: Instant,
    /// Whether the input connection has joined
    pub input_connected: bool,
    /// Channel to notify primary stream of events (e.g., input disconnected)
    pub primary_notify: Option<Sender<SessionEvent>>,
    /// Channel to notify input connection of events (e.g., primary disconnected)
    pub input_notify: Option<Sender<SessionEvent>>,
    /// Channel to forward messages from input connection to streamer (via primary's IPC)
    pub input_to_streamer_tx: Option<Sender<InputToStreamerMessage>>,
    /// Channel to receive messages from streamer for input connection
    pub streamer_to_input_tx: Option<Sender<StreamerToInputMessage>>,
    /// Created timestamp for debugging
    pub created_at: Instant,
}

/// Events that can be sent between primary and input connections
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// The primary stream has disconnected
    PrimaryDisconnected,
    /// The input connection has disconnected
    InputDisconnected,
    /// The input connection successfully joined
    InputJoined,
    /// A new reconnection token is available (sent to primary)
    ReconnectionTokenAvailable(SessionToken),
}

/// Messages from the input connection to be forwarded to the streamer
#[derive(Debug)]
pub enum InputToStreamerMessage {
    /// Input connection joined
    Joined,
    /// WebRTC signaling message from input client
    Signaling(StreamSignalingMessage),
    /// Input connection disconnected
    Disconnected,
}

/// Messages from the streamer to be forwarded to the input connection
#[derive(Debug)]
pub enum StreamerToInputMessage {
    /// WebRTC signaling message for input client
    Signaling(StreamSignalingMessage),
    /// Input peer connection is ready
    Ready,
}

/// Errors that can occur during session operations
#[derive(Debug, Clone)]
pub enum SessionError {
    /// The session token has expired
    TokenExpired,
    /// The session token is invalid
    TokenInvalid,
    /// No session found for the given token
    SessionNotFound,
    /// An input connection is already established for this session
    InputAlreadyConnected,
    /// The primary connection has disconnected
    PrimaryDisconnected,
    /// The session is being shut down
    SessionShuttingDown,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenExpired => write!(f, "Session token has expired"),
            Self::TokenInvalid => write!(f, "Invalid session token"),
            Self::SessionNotFound => write!(f, "No session found for token"),
            Self::InputAlreadyConnected => write!(f, "Input connection already established"),
            Self::PrimaryDisconnected => write!(f, "Primary connection has disconnected"),
            Self::SessionShuttingDown => write!(f, "Session is shutting down"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Manager for hybrid streaming sessions
#[derive(Debug)]
pub struct SessionManager {
    /// Active sessions keyed by session ID
    sessions: Arc<Mutex<HashMap<SessionId, HybridSession>>>,
    /// Index from token to session ID for quick lookup
    token_index: Arc<Mutex<HashMap<SessionToken, SessionId>>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    /// Create a new session manager and start the cleanup task
    pub fn new() -> Self {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let token_index = Arc::new(Mutex::new(HashMap::new()));

        // Start background cleanup task
        let sessions_cleanup = sessions.clone();
        let token_index_cleanup = token_index.clone();
        spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
            loop {
                cleanup_interval.tick().await;
                Self::do_cleanup(&sessions_cleanup, &token_index_cleanup).await;
            }
        });

        Self {
            sessions,
            token_index,
        }
    }

    /// Internal cleanup implementation
    async fn do_cleanup(
        sessions: &Arc<Mutex<HashMap<SessionId, HybridSession>>>,
        token_index: &Arc<Mutex<HashMap<SessionToken, SessionId>>>,
    ) {
        let now = Instant::now();
        let mut expired_sessions = Vec::new();
        let mut expired_tokens = Vec::new();

        {
            let sessions_lock = sessions.lock().await;
            for (id, session) in sessions_lock.iter() {
                // Only clean up if token hasn't been claimed and is expired
                if session.token.is_some() && now > session.token_expires_at && !session.input_connected {
                    expired_sessions.push(id.clone());
                    if let Some(ref token) = session.token {
                        expired_tokens.push(token.clone());
                    }
                }
            }
        }

        if !expired_sessions.is_empty() {
            let mut sessions_lock = sessions.lock().await;
            let mut token_index_lock = token_index.lock().await;

            for id in &expired_sessions {
                if let Some(session) = sessions_lock.remove(id) {
                    // Notify primary that the token expired (input never joined)
                    if let Some(ref notify) = session.primary_notify {
                        let _ = notify.send(SessionEvent::InputDisconnected).await;
                    }
                }
                debug!("[SessionManager] Cleaned up expired session {}", id);
            }

            for token in &expired_tokens {
                token_index_lock.remove(token);
            }

            info!(
                "[SessionManager] Cleaned up {} expired sessions",
                expired_sessions.len()
            );
        }
    }

    /// Register a new hybrid session with a generated token
    /// Returns the session ID, token, and a receiver for input messages to forward to streamer
    pub async fn register_session(
        &self,
        token: SessionToken,
    ) -> (SessionId, SessionToken, Receiver<InputToStreamerMessage>) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(TOKEN_EXPIRATION_SECS);

        // Create channel for input -> streamer messages
        let (input_to_streamer_tx, input_to_streamer_rx) = channel(32);

        let session = HybridSession {
            id: session_id.clone(),
            token: Some(token.clone()),
            token_expires_at: expires_at,
            input_connected: false,
            primary_notify: None,
            input_notify: None,
            input_to_streamer_tx: Some(input_to_streamer_tx),
            streamer_to_input_tx: None,
            created_at: now,
        };

        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(session_id.clone(), session);
        }

        {
            let mut token_index = self.token_index.lock().await;
            token_index.insert(token.clone(), session_id.clone());
        }

        info!(
            "[SessionManager] Registered session {} with token {} (expires in {}s)",
            session_id, token, TOKEN_EXPIRATION_SECS
        );

        (session_id, token, input_to_streamer_rx)
    }

    /// Attempt to claim a session using the provided token
    /// Returns the session ID and channels for communication if successful
    pub async fn claim_session(
        &self,
        token: &str,
    ) -> Result<(SessionId, Sender<InputToStreamerMessage>, Receiver<StreamerToInputMessage>), SessionError> {
        let session_id = {
            let token_index = self.token_index.lock().await;
            token_index.get(token).cloned()
        };

        let Some(session_id) = session_id else {
            warn!("[SessionManager] Token not found: {}", token);
            return Err(SessionError::SessionNotFound);
        };

        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get_mut(&session_id) else {
            warn!("[SessionManager] Session not found for token: {}", token);
            // Clean up orphaned token
            drop(sessions);
            let mut token_index = self.token_index.lock().await;
            token_index.remove(token);
            return Err(SessionError::SessionNotFound);
        };

        // Check if token is expired
        if Instant::now() > session.token_expires_at {
            warn!(
                "[SessionManager] Token expired for session {}: {}",
                session_id, token
            );
            return Err(SessionError::TokenExpired);
        }

        // Check if input is already connected
        if session.input_connected {
            warn!(
                "[SessionManager] Input already connected for session {}",
                session_id
            );
            return Err(SessionError::InputAlreadyConnected);
        }

        // Get or create the input_to_streamer sender
        let input_to_streamer_tx = session
            .input_to_streamer_tx
            .clone()
            .ok_or(SessionError::SessionNotFound)?;

        // Create channel for streamer -> input messages
        let (streamer_to_input_tx, streamer_to_input_rx) = channel(32);
        session.streamer_to_input_tx = Some(streamer_to_input_tx);

        // Consume the token
        session.token = None;
        session.input_connected = true;

        // Remove from token index (need to drop sessions lock first)
        let session_id_clone = session_id.clone();
        drop(sessions);
        
        {
            let mut token_index = self.token_index.lock().await;
            token_index.remove(token);
        }

        // Notify primary that input joined
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(&session_id_clone) {
            if let Some(ref notify) = session.primary_notify {
                let _ = notify.send(SessionEvent::InputJoined).await;
            }
        }

        info!(
            "[SessionManager] Session {} claimed by input connection",
            session_id_clone
        );

        Ok((session_id_clone, input_to_streamer_tx, streamer_to_input_rx))
    }

    /// Send a message from streamer to the input connection
    pub async fn send_to_input(&self, session_id: &str, message: StreamerToInputMessage) -> bool {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(session_id) {
            if let Some(ref tx) = session.streamer_to_input_tx {
                return tx.send(message).await.is_ok();
            }
        }
        false
    }

    /// Set the notification channel for the primary stream
    pub async fn set_primary_notify(&self, session_id: &str, notify: Sender<SessionEvent>) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.primary_notify = Some(notify);
            debug!(
                "[SessionManager] Set primary notify channel for session {}",
                session_id
            );
        }
    }

    /// Set the notification channel for the input connection
    pub async fn set_input_notify(&self, session_id: &str, notify: Sender<SessionEvent>) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.input_notify = Some(notify);
            debug!(
                "[SessionManager] Set input notify channel for session {}",
                session_id
            );
        }
    }

    /// Called when the primary stream disconnects
    pub async fn primary_disconnected(&self, session_id: &str) {
        let session = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(session_id)
        };

        if let Some(session) = session {
            // Clean up token index if token wasn't claimed
            if let Some(ref token) = session.token {
                let mut token_index = self.token_index.lock().await;
                token_index.remove(token);
            }

            // Notify input connection if connected
            if let Some(ref notify) = session.input_notify {
                let _ = notify.send(SessionEvent::PrimaryDisconnected).await;
            }

            info!(
                "[SessionManager] Primary disconnected, removed session {}",
                session_id
            );
        }
    }

    /// Called when the input connection disconnects
    /// Returns a new token if reconnection is allowed
    pub async fn input_disconnected(&self, session_id: &str) -> Option<SessionToken> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.get_mut(session_id)?;

        session.input_connected = false;
        session.input_notify = None;
        session.streamer_to_input_tx = None;

        info!(
            "[SessionManager] Input disconnected from session {}",
            session_id
        );

        // Generate a new token to allow reconnection
        let new_token = uuid::Uuid::new_v4().to_string();
        let now = Instant::now();
        session.token = Some(new_token.clone());
        session.token_expires_at = now + Duration::from_secs(TOKEN_EXPIRATION_SECS);

        // Create new channel for input -> streamer messages
        let (input_to_streamer_tx, _input_to_streamer_rx) = channel(32);
        session.input_to_streamer_tx = Some(input_to_streamer_tx);

        // Notify primary stream of disconnection and new token
        if let Some(ref notify) = session.primary_notify {
            let _ = notify.send(SessionEvent::InputDisconnected).await;
            let _ = notify
                .send(SessionEvent::ReconnectionTokenAvailable(new_token.clone()))
                .await;
        }

        // Update token index
        drop(sessions);
        {
            let mut token_index = self.token_index.lock().await;
            token_index.insert(new_token.clone(), session_id.to_string());
        }

        info!(
            "[SessionManager] Generated reconnection token for session {}: {}",
            session_id, new_token
        );

        Some(new_token)
    }

    /// Check if a session exists and is still active (primary connected)
    pub async fn is_session_active(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions.contains_key(session_id)
    }

    /// Force close a session (e.g., on error)
    pub async fn force_close_session(&self, session_id: &str) {
        let session = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(session_id)
        };

        if let Some(session) = session {
            // Clean up token index if token wasn't claimed
            if let Some(ref token) = session.token {
                let mut token_index = self.token_index.lock().await;
                token_index.remove(token);
            }

            // Notify both connections
            if let Some(ref notify) = session.primary_notify {
                let _ = notify.send(SessionEvent::InputDisconnected).await;
            }
            if let Some(ref notify) = session.input_notify {
                let _ = notify.send(SessionEvent::PrimaryDisconnected).await;
            }

            info!(
                "[SessionManager] Force closed session {}",
                session_id
            );
        }
    }

    /// Get a session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<SessionId> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).map(|s| s.id.clone())
    }

    /// Check if input is connected for a session
    pub async fn is_input_connected(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions
            .get(session_id)
            .map(|s| s.input_connected)
            .unwrap_or(false)
    }

    /// Clean up expired sessions (should be called periodically)
    pub async fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut expired_sessions = Vec::new();
        let mut expired_tokens = Vec::new();

        {
            let sessions = self.sessions.lock().await;
            for (id, session) in sessions.iter() {
                // Only clean up if token hasn't been claimed and is expired
                if session.token.is_some() && now > session.token_expires_at && !session.input_connected
                {
                    expired_sessions.push(id.clone());
                    if let Some(ref token) = session.token {
                        expired_tokens.push(token.clone());
                    }
                }
            }
        }

        if !expired_sessions.is_empty() {
            let mut sessions = self.sessions.lock().await;
            let mut token_index = self.token_index.lock().await;

            for id in &expired_sessions {
                sessions.remove(id);
                debug!("[SessionManager] Cleaned up expired session {}", id);
            }

            for token in &expired_tokens {
                token_index.remove(token);
            }

            info!(
                "[SessionManager] Cleaned up {} expired sessions",
                expired_sessions.len()
            );
        }
    }

    /// Get the count of active sessions (for debugging/monitoring)
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.lock().await;
        sessions.len()
    }
}

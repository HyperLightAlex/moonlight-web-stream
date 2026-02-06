//! Fuji Internal API Client
//!
//! This module provides a client for communicating with Fuji's internal API.
//! The internal API runs on localhost:47991 when the web server is embedded
//! within the Fuji desktop application.
//!
//! The internal API provides:
//! - Game list with metadata and artwork
//! - Game launch/stop via Fuji's platform-aware logic
//! - Streaming session management
//! - Host capabilities information

use log::{debug, info, warn, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default Fuji internal API port
const FUJI_INTERNAL_API_PORT: u16 = 47991;

/// Fuji internal API base URL
fn fuji_internal_url() -> String {
    format!("http://127.0.0.1:{}/internal", FUJI_INTERNAL_API_PORT)
}

/// Error types for Fuji internal API operations
#[derive(Debug, thiserror::Error)]
pub enum FujiInternalError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Internal API not available")]
    NotAvailable,
    #[error("Game not found: {0}")]
    GameNotFound(String),
    #[error("Launch failed: {0}")]
    LaunchFailed(String),
}

/// Game metadata from Fuji
#[derive(Debug, Clone, Deserialize)]
pub struct FujiGame {
    pub id: String,
    pub title: String,
    pub platform: String,
    #[serde(rename = "platformId")]
    pub platform_id: Option<String>,
    #[serde(rename = "executablePath")]
    pub executable_path: Option<String>,
    #[serde(rename = "installPath")]
    pub install_path: Option<String>,
    #[serde(rename = "launchCommand")]
    pub launch_command: Option<String>,
    #[serde(rename = "lastPlayed")]
    pub last_played: Option<String>,
    pub metadata: Option<FujiGameMetadata>,
    pub artwork: Option<FujiGameArtwork>,
    #[serde(rename = "isRunning")]
    pub is_running: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FujiGameMetadata {
    pub summary: Option<String>,
    pub genres: Option<Vec<String>>,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FujiGameArtwork {
    #[serde(rename = "hasCover")]
    pub has_cover: bool,
    #[serde(rename = "coverUrl")]
    pub cover_url: Option<String>,
    #[serde(rename = "hasScreenshots")]
    pub has_screenshots: bool,
    #[serde(rename = "screenshotUrls")]
    pub screenshot_urls: Option<Vec<String>>,
}

/// Game list response
#[derive(Debug, Deserialize)]
pub struct FujiGamesResponse {
    pub games: Vec<FujiGame>,
    pub count: usize,
    #[serde(rename = "lastScanTime")]
    pub last_scan_time: Option<String>,
}

/// Sunshine app (from Fuji's proxy to Sunshine)
/// When synced from Fuji, apps include fuji_game_id for direct mapping
#[derive(Debug, Deserialize, Clone)]
pub struct SunshineApp {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub is_hdr_supported: bool,
    /// Fuji game ID (e.g., "steam_1229240") - present for Fuji-synced games
    pub fuji_game_id: Option<String>,
}

/// Sunshine apps list response (from Fuji's proxy endpoint)
#[derive(Debug, Deserialize)]
pub struct SunshineAppsResponse {
    pub apps: Vec<SunshineApp>,
}

/// Game launch request
#[derive(Debug, Serialize)]
pub struct LaunchRequest {
    #[serde(rename = "streamMode")]
    pub stream_mode: bool,
}

/// Game launch response
#[derive(Debug, Deserialize)]
pub struct LaunchResponse {
    pub success: bool,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub game: Option<LaunchGameInfo>,
    pub process: Option<LaunchProcessInfo>,
    pub error: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LaunchGameInfo {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct LaunchProcessInfo {
    pub pid: Option<u32>,
    #[serde(rename = "startTime")]
    pub start_time: Option<String>,
}

/// Stop request
#[derive(Debug, Serialize)]
pub struct StopRequest {
    pub force: bool,
}

/// Stop response  
#[derive(Debug, Deserialize)]
pub struct StopResponse {
    pub success: bool,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// Stream launch request (for orchestration endpoint)
#[derive(Debug, Serialize)]
pub struct StreamLaunchRequest {
    #[serde(rename = "gameId")]
    pub game_id: String,
}

/// Stream launch request with both IDs (for orchestration endpoint)
#[derive(Debug, Serialize)]
pub struct StreamLaunchWithIdsRequest {
    /// Fuji game ID (for internal tracking, optional)
    #[serde(rename = "fujiGameId", skip_serializing_if = "Option::is_none")]
    pub fuji_game_id: Option<String>,
    /// Sunshine's real app ID (for streaming)
    #[serde(rename = "sunshineAppId")]
    pub sunshine_app_id: u32,
    /// Game title (for matching/logging)
    pub title: String,
}

/// Stream launch response (from orchestration endpoint)
#[derive(Debug, Deserialize)]
pub struct StreamLaunchResponse {
    pub success: bool,
    /// "launch" or "resume"
    pub action: Option<String>,
    /// True if a previous game was cancelled before launching this one
    #[serde(rename = "cancelledPrevious")]
    pub cancelled_previous: Option<bool>,
    /// Sunshine app index (for /launch or /resume calls)
    #[serde(rename = "appIndex")]
    pub app_index: Option<u32>,
    /// Game info
    pub game: Option<StreamLaunchGameInfo>,
    /// Error message if success is false
    pub error: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamLaunchGameInfo {
    pub id: String,
    pub title: String,
    #[serde(rename = "sunshineAppId")]
    pub sunshine_app_id: Option<u32>,
}

/// Stream started notification request
#[derive(Debug, Serialize)]
pub struct StreamStartedRequest {
    #[serde(rename = "gameId")]
    pub game_id: String,
}

/// Session response
#[derive(Debug, Deserialize)]
pub struct SessionResponse {
    pub active: bool,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub game: Option<SessionGameInfo>,
    #[serde(rename = "startTime")]
    pub start_time: Option<String>,
    pub duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct SessionGameInfo {
    pub id: String,
    pub title: String,
}

/// Fuji status response
#[derive(Debug, Deserialize)]
pub struct FujiStatus {
    pub version: String,
    pub sunshine: SunshineStatus,
    #[serde(rename = "webServer")]
    pub web_server: WebServerStatus,
    pub network: NetworkStatus,
    pub streaming: StreamingStatus,
}

#[derive(Debug, Deserialize)]
pub struct SunshineStatus {
    pub running: bool,
    pub version: Option<String>,
    pub port: u16,  // Legacy field
    #[serde(rename = "httpPort")]
    pub http_port: Option<u16>,
    #[serde(rename = "httpsPort")]
    pub https_port: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct WebServerStatus {
    pub running: bool,
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkStatus {
    #[serde(rename = "localIp")]
    pub local_ip: Option<String>,
    #[serde(rename = "externalIp")]
    pub external_ip: Option<String>,
    #[serde(rename = "upnpAvailable")]
    pub upnp_available: bool,
}

#[derive(Debug, Deserialize)]
pub struct StreamingStatus {
    pub active: bool,
    #[serde(rename = "currentGame")]
    pub current_game: Option<SessionGameInfo>,
}

/// Client for Fuji's internal API
pub struct FujiInternalClient {
    client: Client,
    base_url: String,
}

impl Default for FujiInternalClient {
    fn default() -> Self {
        Self::new()
    }
}

impl FujiInternalClient {
    /// Create a new client with default settings
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: fuji_internal_url(),
        }
    }

    /// Check if Fuji's internal API is available
    pub async fn is_available(&self) -> bool {
        match self.get_status().await {
            Ok(_) => true,
            Err(e) => {
                debug!("Fuji internal API not available: {}", e);
                false
            }
        }
    }

    /// Get Fuji status
    pub async fn get_status(&self) -> Result<FujiStatus, FujiInternalError> {
        let url = format!("{}/status", self.base_url);
        
        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            return Err(FujiInternalError::ApiError(
                format!("Status check failed: {}", response.status())
            ));
        }

        let status: FujiStatus = response.json().await?;
        Ok(status)
    }

    /// Get current Sunshine ports from Fuji
    /// Returns (http_port, https_port)
    pub async fn get_sunshine_ports(&self) -> Result<(u16, u16), FujiInternalError> {
        let status = self.get_status().await?;
        let http_port = status.sunshine.http_port.unwrap_or(status.sunshine.port);
        let https_port = status.sunshine.https_port.unwrap_or(http_port + 1);
        info!("[Fuji] Got Sunshine ports - HTTP: {}, HTTPS: {}", http_port, https_port);
        Ok((http_port, https_port))
    }

    /// Get Sunshine apps list via Fuji (uses correct dynamic port)
    pub async fn get_sunshine_apps(&self) -> Result<SunshineAppsResponse, FujiInternalError> {
        let url = format!("{}/sunshine/apps", self.base_url);
        
        info!("[Fuji] Getting Sunshine apps via internal API");
        
        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("[Fuji] Get Sunshine apps failed: {} - {}", status, body);
            return Err(FujiInternalError::ApiError(
                format!("Get Sunshine apps failed: {} - {}", status, body)
            ));
        }

        let apps: SunshineAppsResponse = response.json().await?;
        info!("[Fuji] Got {} Sunshine apps", apps.apps.len());
        Ok(apps)
    }

    /// Get list of games from Fuji
    pub async fn get_games(&self, platform: Option<&str>, search: Option<&str>) -> Result<FujiGamesResponse, FujiInternalError> {
        let mut url = format!("{}/games", self.base_url);
        let mut params = vec![];
        
        if let Some(p) = platform {
            params.push(format!("platform={}", urlencoding::encode(p)));
        }
        if let Some(s) = search {
            params.push(format!("search={}", urlencoding::encode(s)));
        }
        
        if !params.is_empty() {
            url = format!("{}?{}", url, params.join("&"));
        }

        debug!("Fetching games from Fuji: {}", url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(FujiInternalError::ApiError(
                format!("Get games failed: {} - {}", status, body)
            ));
        }

        let games: FujiGamesResponse = response.json().await?;
        info!("Got {} games from Fuji", games.count);
        Ok(games)
    }

    /// Get a single game by ID
    pub async fn get_game(&self, game_id: &str) -> Result<FujiGame, FujiInternalError> {
        let url = format!("{}/games/{}", self.base_url, urlencoding::encode(game_id));

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FujiInternalError::GameNotFound(game_id.to_string()));
        }

        if !response.status().is_success() {
            return Err(FujiInternalError::ApiError(
                format!("Get game failed: {}", response.status())
            ));
        }

        let game: FujiGame = response.json().await?;
        Ok(game)
    }

    /// Get game cover image bytes
    pub async fn get_game_cover(&self, game_id: &str, size: Option<&str>) -> Result<Vec<u8>, FujiInternalError> {
        let mut url = format!("{}/games/{}/cover", self.base_url, urlencoding::encode(game_id));
        
        if let Some(s) = size {
            url = format!("{}?size={}", url, s);
        }

        debug!("Fetching cover for game {}", game_id);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FujiInternalError::GameNotFound(game_id.to_string()));
        }

        if !response.status().is_success() {
            return Err(FujiInternalError::ApiError(
                format!("Get cover failed: {}", response.status())
            ));
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Launch a game
    pub async fn launch_game(&self, game_id: &str, stream_mode: bool) -> Result<LaunchResponse, FujiInternalError> {
        let url = format!("{}/games/{}/launch", self.base_url, urlencoding::encode(game_id));
        
        let request = LaunchRequest { stream_mode };

        info!("Launching game {} via Fuji internal API", game_id);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::CONFLICT {
            // Game already running
            let launch_response: LaunchResponse = response.json().await?;
            return Ok(launch_response);
        }

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FujiInternalError::GameNotFound(game_id.to_string()));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Game launch failed: {} - {}", status, body);
            return Err(FujiInternalError::LaunchFailed(body));
        }

        let launch_response: LaunchResponse = response.json().await?;
        
        if launch_response.success {
            info!("Game {} launched successfully, session: {:?}", game_id, launch_response.session_id);
        } else {
            warn!("Game {} launch reported failure: {:?}", game_id, launch_response.error);
        }

        Ok(launch_response)
    }

    /// Stop a running game
    pub async fn stop_game(&self, game_id: &str, force: bool) -> Result<StopResponse, FujiInternalError> {
        let url = format!("{}/games/{}/stop", self.base_url, urlencoding::encode(game_id));
        
        let request = StopRequest { force };

        info!("Stopping game {} via Fuji internal API", game_id);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(FujiInternalError::ApiError(
                format!("Stop game failed: {} - {}", status, body)
            ));
        }

        let stop_response: StopResponse = response.json().await?;
        Ok(stop_response)
    }

    /// Get current streaming session
    pub async fn get_session(&self) -> Result<SessionResponse, FujiInternalError> {
        let url = format!("{}/session", self.base_url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(FujiInternalError::ApiError(
                format!("Get session failed: {}", response.status())
            ));
        }

        let session: SessionResponse = response.json().await?;
        Ok(session)
    }

    /// End current streaming session
    pub async fn end_session(&self) -> Result<(), FujiInternalError> {
        let url = format!("{}/session", self.base_url);

        let response = self.client.delete(&url).send().await?;

        if !response.status().is_success() {
            return Err(FujiInternalError::ApiError(
                format!("End session failed: {}", response.status())
            ));
        }

        Ok(())
    }

    /// Launch stream via orchestration endpoint (NEW - Fuji handles cancel/launch decision)
    /// 
    /// This is the main entry point for starting a stream. Fuji will:
    /// 1. Check if a different game is running
    /// 2. Cancel the previous game if needed
    /// 3. Return whether to "launch" or "resume"
    /// 
    /// The web server should pass this `action` to the streamer.
    pub async fn stream_launch(&self, game_id: &str) -> Result<StreamLaunchResponse, FujiInternalError> {
        let url = format!("{}/stream/launch", self.base_url);
        
        let request = StreamLaunchRequest { 
            game_id: game_id.to_string() 
        };

        info!("[Fuji] Calling stream orchestration for game: {}", game_id);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FujiInternalError::GameNotFound(game_id.to_string()));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("[Fuji] Stream launch failed: {} - {}", status, body);
            return Err(FujiInternalError::LaunchFailed(body));
        }

        let launch_response: StreamLaunchResponse = response.json().await?;
        
        if launch_response.success {
            info!(
                "[Fuji] Stream orchestration result: action={:?}, appIndex={:?}, cancelledPrevious={:?}", 
                launch_response.action,
                launch_response.app_index,
                launch_response.cancelled_previous
            );
        } else {
            warn!("[Fuji] Stream orchestration failed: {:?}", launch_response.error);
        }

        Ok(launch_response)
    }

    /// Launch stream with both Fuji game ID and Sunshine app ID
    /// 
    /// This variant passes both IDs to Fuji:
    /// - fuji_game_id: For Fuji's internal game tracking (optional)
    /// - sunshine_app_id: The REAL Sunshine app ID for streaming
    /// - title: Game title for matching/logging
    /// 
    /// Fuji uses these to track the session and make orchestration decisions.
    /// The sunshine_app_id is what the streamer will use for /launch calls.
    pub async fn stream_launch_with_sunshine_id(
        &self, 
        fuji_game_id: Option<&str>,
        sunshine_app_id: u32,
        title: &str,
    ) -> Result<StreamLaunchResponse, FujiInternalError> {
        let url = format!("{}/stream/launch", self.base_url);
        
        let request = StreamLaunchWithIdsRequest { 
            fuji_game_id: fuji_game_id.map(|s| s.to_string()),
            sunshine_app_id,
            title: title.to_string(),
        };

        info!(
            "[Fuji] Calling stream orchestration with IDs - fuji: {:?}, sunshine: {}, title: '{}'",
            fuji_game_id, sunshine_app_id, title
        );

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FujiInternalError::GameNotFound(title.to_string()));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("[Fuji] Stream launch failed: {} - {}", status, body);
            return Err(FujiInternalError::LaunchFailed(body));
        }

        let launch_response: StreamLaunchResponse = response.json().await?;
        
        if launch_response.success {
            info!(
                "[Fuji] Stream orchestration result: action={:?}, cancelledPrevious={:?}", 
                launch_response.action,
                launch_response.cancelled_previous
            );
        } else {
            warn!("[Fuji] Stream orchestration failed: {:?}", launch_response.error);
        }

        Ok(launch_response)
    }

    /// Notify Fuji that the stream has started successfully
    pub async fn stream_started(&self, game_id: &str) -> Result<(), FujiInternalError> {
        let url = format!("{}/stream/started", self.base_url);
        
        let request = StreamStartedRequest { 
            game_id: game_id.to_string() 
        };

        info!("[Fuji] Notifying stream started for game: {}", game_id);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("[Fuji] Stream started notification failed: {} - {}", status, body);
        }

        Ok(())
    }

    /// Stop the current stream (calls new stream/stop endpoint)
    pub async fn stream_stop(&self) -> Result<StopResponse, FujiInternalError> {
        let url = format!("{}/stream/stop", self.base_url);

        info!("[Fuji] Stopping stream via orchestration API");

        let response = self.client
            .post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(FujiInternalError::ApiError(
                format!("Stream stop failed: {} - {}", status, body)
            ));
        }

        let stop_response: StopResponse = response.json().await?;
        Ok(stop_response)
    }
}

// Global singleton client
lazy_static::lazy_static! {
    pub static ref FUJI_INTERNAL_CLIENT: FujiInternalClient = FujiInternalClient::new();
}

/// Check if running inside Fuji (internal API is available)
pub async fn is_embedded_in_fuji() -> bool {
    FUJI_INTERNAL_CLIENT.is_available().await
}

/// Get the global Fuji internal client
pub fn fuji_client() -> &'static FujiInternalClient {
    &FUJI_INTERNAL_CLIENT
}

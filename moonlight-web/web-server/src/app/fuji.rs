//! Fuji host detection and OTP auto-pairing support
//!
//! Fuji is a desktop app that wraps Sunshine with additional features.
//! Fuji's bundled Sunshine supports OTP (One-Time Password) pairing
//! which allows for automatic pairing without manual PIN entry.

use log::{debug, warn};
use reqwest::Client;
use serde::Deserialize;

/// Default credentials used by Fuji's bundled Sunshine
const FUJI_DEFAULT_USERNAME: &str = "username";
const FUJI_DEFAULT_PASSWORD: &str = "password";

/// OTP response from Fuji's Sunshine
#[derive(Debug, Deserialize)]
pub struct FujiOtpResponse {
    pub pin: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
}

/// Error types for Fuji operations
#[derive(Debug, thiserror::Error)]
pub enum FujiError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("OTP request failed with status: {0}")]
    OtpFailed(u16),
    #[error("Invalid OTP response")]
    InvalidResponse,
}

/// Check if a host is a Fuji host by attempting to access the OTP endpoint
///
/// Fuji's bundled Sunshine has an `/otp/request` endpoint that isn't present
/// in standard Sunshine installations.
pub async fn is_fuji_host(https_hostport: &str) -> bool {
    // Build a client that accepts self-signed certificates (Sunshine uses them)
    let client = match Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to build HTTP client for Fuji detection: {e}");
            return false;
        }
    };

    // Try to access the OTP endpoint with default Fuji credentials
    // If it succeeds or returns 200, it's a Fuji host
    let url = format!(
        "https://{}/otp/request?passphrase=test&deviceName=detection",
        https_hostport
    );

    match client
        .get(&url)
        .basic_auth(FUJI_DEFAULT_USERNAME, Some(FUJI_DEFAULT_PASSWORD))
        .send()
        .await
    {
        Ok(response) => {
            // Fuji will return 200 for successful OTP requests
            // Standard Sunshine will return 404 (endpoint doesn't exist)
            let is_fuji = response.status().is_success();
            debug!(
                "Fuji detection for {}: status={}, is_fuji={}",
                https_hostport,
                response.status(),
                is_fuji
            );
            is_fuji
        }
        Err(e) => {
            debug!("Fuji detection failed for {}: {}", https_hostport, e);
            false
        }
    }
}

/// Request an OTP from a Fuji host for automated pairing
///
/// The OTP can then be used with the standard Moonlight pairing flow
pub async fn request_fuji_otp(
    https_hostport: &str,
    passphrase: &str,
    device_name: &str,
) -> Result<FujiOtpResponse, FujiError> {
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let url = format!(
        "https://{}/otp/request?passphrase={}&deviceName={}",
        https_hostport,
        urlencoding::encode(passphrase),
        urlencoding::encode(device_name)
    );

    debug!("Requesting Fuji OTP from: {}", https_hostport);

    let response = client
        .get(&url)
        .basic_auth(FUJI_DEFAULT_USERNAME, Some(FUJI_DEFAULT_PASSWORD))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(FujiError::OtpFailed(response.status().as_u16()));
    }

    let otp: FujiOtpResponse = response.json().await?;

    debug!(
        "Received Fuji OTP, expires at: {}",
        otp.expires_at
    );

    Ok(otp)
}


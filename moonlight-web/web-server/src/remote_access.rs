//! Remote access discovery and caching for remote streaming support.
//!
//! This module discovers the server's external connectivity information
//! (external IP, NAT type, etc.) at startup and caches it for inclusion
//! in pairing responses.

use common::{
    api_bindings::{RemoteAccessInfo, RtcIceServer},
    config::{Config, RemoteConfig},
};
use log::info;

use crate::{
    stun::StunClient,
    upnp::UpnpStatus,
};

/// Provides cached remote access information for inclusion in API responses.
#[derive(Debug, Clone)]
pub struct RemoteAccessProvider {
    /// Cached remote access info, built at startup
    info: Option<RemoteAccessInfo>,
}

impl RemoteAccessProvider {
    /// Create a new RemoteAccessProvider by discovering external connectivity.
    pub fn new(
        config: &Config,
        upnp_status: Option<&UpnpStatus>,
    ) -> Self {
        if !config.remote.enabled {
            info!("[Remote] Remote access info disabled in config");
            return Self { info: None };
        }

        let info = build_remote_access_info(config, upnp_status);
        
        if let Some(ref info) = info {
            info!("[Remote] Remote access info available:");
            if let Some(ref ip) = info.external_ip {
                info!("[Remote]   External IP: {} (via {})", ip, info.discovery_method);
            }
            if let Some(ref hostname) = info.hostname {
                info!("[Remote]   Hostname: {}", hostname);
            }
            info!("[Remote]   NAT type: {}", info.nat_type);
            info!("[Remote]   TURN recommended: {}", info.turn_recommended);
        }

        Self { info }
    }

    /// Get the cached remote access info.
    pub fn get_info(&self) -> Option<RemoteAccessInfo> {
        self.info.clone()
    }

    /// Check if remote access info is available.
    pub fn is_available(&self) -> bool {
        self.info.is_some()
    }
}

/// Build RemoteAccessInfo from config and discovered network info.
fn build_remote_access_info(
    config: &Config,
    upnp_status: Option<&UpnpStatus>,
) -> Option<RemoteAccessInfo> {
    let remote_config = &config.remote;

    // Determine external IP and discovery method
    let (external_ip, discovery_method) = discover_external_ip(remote_config, upnp_status);

    // Get hostname from config (user-provided)
    let hostname = remote_config.hostname.clone();

    // If we have neither external IP nor hostname, remote access isn't available
    if external_ip.is_none() && hostname.is_none() {
        info!("[Remote] No external IP or hostname available");
        return None;
    }

    // Determine port
    let port = remote_config.port.unwrap_or(config.web_server.bind_address.port());

    // Check SSL availability
    let ssl_available = config.web_server.certificate.is_some();

    // If SSL is required but not available, disable remote access
    if remote_config.ssl_required && !ssl_available {
        info!("[Remote] SSL required but not configured, disabling remote access");
        return None;
    }

    // Detect NAT type and TURN recommendation
    let (nat_type, turn_recommended) = detect_nat_info(remote_config);

    // Build ICE servers list (include TURN if configured)
    let ice_servers = build_ice_servers(config);

    Some(RemoteAccessInfo {
        external_ip,
        hostname,
        port,
        ssl_available,
        discovery_method,
        nat_type,
        turn_recommended,
        ice_servers,
    })
}

/// Discover external IP using UPnP and/or STUN.
fn discover_external_ip(
    remote_config: &RemoteConfig,
    upnp_status: Option<&UpnpStatus>,
) -> (Option<String>, String) {
    // First, check UPnP
    if let Some(status) = upnp_status {
        if let Some(ip) = status.external_ip {
            return (Some(ip.to_string()), "upnp".to_string());
        }
    }

    // Fall back to STUN if enabled
    if remote_config.stun_discovery {
        let stun_client = StunClient::new();
        match stun_client.get_external_address() {
            Ok(result) => {
                return (Some(result.external_ip.to_string()), "stun".to_string());
            }
            Err(e) => {
                info!("[Remote] STUN discovery failed: {}", e);
            }
        }
    }

    (None, "none".to_string())
}

/// Detect NAT type and whether TURN is recommended.
fn detect_nat_info(remote_config: &RemoteConfig) -> (String, bool) {
    if !remote_config.stun_discovery {
        return ("unknown".to_string(), false);
    }

    let stun_client = StunClient::new();
    let result = stun_client.detect_nat_type();

    if result.success {
        let nat_type = result.nat_type;
        let turn_recommended = !nat_type.supports_direct_connection();
        (nat_type.as_str().to_string(), turn_recommended)
    } else {
        ("unknown".to_string(), false)
    }
}

/// Build ICE servers list including TURN if configured.
fn build_ice_servers(config: &Config) -> Option<Vec<RtcIceServer>> {
    let mut servers = config.webrtc.ice_servers.clone();

    // Add TURN server if configured
    if let Some(turn_server) = config.turn.to_ice_server() {
        servers.push(turn_server);
    }

    if servers.is_empty() {
        None
    } else {
        Some(servers)
    }
}


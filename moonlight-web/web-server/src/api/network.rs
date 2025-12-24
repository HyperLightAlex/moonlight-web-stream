//! Network status API endpoints for remote streaming diagnostics.

use actix_web::{get, web::Data, HttpResponse};
use serde::Serialize;

use crate::upnp::UpnpManager;

/// Network status response for remote streaming diagnostics
#[derive(Debug, Clone, Serialize)]
pub struct NetworkStatusResponse {
    /// UPnP status
    pub upnp: UpnpStatusResponse,
    /// Server's local IP addresses
    pub local_addresses: Vec<String>,
    /// Whether the server is likely accessible remotely
    pub remote_accessible: bool,
    /// Recommendations for improving remote access
    pub recommendations: Vec<String>,
}

/// UPnP-specific status
#[derive(Debug, Clone, Serialize)]
pub struct UpnpStatusResponse {
    /// Whether UPnP is enabled in config
    pub enabled: bool,
    /// Whether a UPnP gateway was found
    pub available: bool,
    /// External (public) IP address discovered via UPnP
    pub external_ip: Option<String>,
    /// Gateway device description
    pub gateway: Option<String>,
    /// List of successfully mapped ports
    pub mapped_ports: Vec<MappedPortResponse>,
    /// Last error if UPnP setup failed
    pub last_error: Option<String>,
}

/// Information about a mapped port
#[derive(Debug, Clone, Serialize)]
pub struct MappedPortResponse {
    pub external_port: u16,
    pub internal_port: u16,
    pub protocol: String,
    pub success: bool,
}

/// Get network status for remote streaming diagnostics
#[get("/network/status")]
pub async fn get_network_status(
    upnp_manager: Option<Data<UpnpManager>>,
) -> HttpResponse {
    let mut recommendations = Vec::new();
    let mut remote_accessible = false;

    let upnp_status = if let Some(manager) = upnp_manager {
        let status = manager.status().await;

        if status.available && status.external_ip.is_some() {
            remote_accessible = true;
        } else if !status.available {
            recommendations.push(
                "UPnP gateway not found. Your router may not support UPnP, or it may be disabled. \
                Consider enabling UPnP in your router settings or manually forwarding ports."
                    .to_string(),
            );
        }

        UpnpStatusResponse {
            enabled: true,
            available: status.available,
            external_ip: status.external_ip.map(|ip| ip.to_string()),
            gateway: status.gateway_description,
            mapped_ports: status
                .port_mappings
                .into_iter()
                .map(|m| MappedPortResponse {
                    external_port: m.external_port,
                    internal_port: m.internal_port,
                    protocol: match m.protocol {
                        igd_next::PortMappingProtocol::TCP => "TCP".to_string(),
                        igd_next::PortMappingProtocol::UDP => "UDP".to_string(),
                    },
                    success: m.success,
                })
                .collect(),
            last_error: status.last_error,
        }
    } else {
        recommendations.push(
            "UPnP is disabled. Enable it in the config to automatically set up port forwarding, \
            or manually forward ports for remote streaming."
                .to_string(),
        );

        UpnpStatusResponse {
            enabled: false,
            available: false,
            external_ip: None,
            gateway: None,
            mapped_ports: Vec::new(),
            last_error: None,
        }
    };

    // Get local addresses
    let local_addresses = get_local_addresses();

    if !remote_accessible && recommendations.is_empty() {
        recommendations.push(
            "Unable to determine remote accessibility. Consider using a VPN like Tailscale \
            for reliable remote access without port forwarding."
                .to_string(),
        );
    }

    let response = NetworkStatusResponse {
        upnp: upnp_status,
        local_addresses,
        remote_accessible,
        recommendations,
    };

    HttpResponse::Ok().json(response)
}

/// Get local IP addresses
fn get_local_addresses() -> Vec<String> {
    use std::net::UdpSocket;

    let mut addresses = Vec::new();

    // Try to get the primary local IP
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                addresses.push(addr.ip().to_string());
            }
        }
    }

    addresses
}


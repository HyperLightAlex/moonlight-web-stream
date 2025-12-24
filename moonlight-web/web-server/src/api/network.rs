//! Network status API endpoints for remote streaming diagnostics.

use actix_web::{get, web::Data, HttpResponse};
use log::info;
use serde::Serialize;

use crate::{
    stun::{NatType, StunClient},
    upnp::UpnpManager,
};

/// Network status response for remote streaming diagnostics
#[derive(Debug, Clone, Serialize)]
pub struct NetworkStatusResponse {
    /// UPnP status
    pub upnp: UpnpStatusResponse,
    /// NAT type detection results
    pub nat: NatStatusResponse,
    /// Server's local IP addresses
    pub local_addresses: Vec<String>,
    /// Whether the server is likely accessible remotely
    pub remote_accessible: bool,
    /// Whether direct P2P connections are likely to work
    pub direct_connection_possible: bool,
    /// Whether TURN relay may be needed
    pub turn_recommended: bool,
    /// Issues detected with the network configuration
    pub issues: Vec<NetworkIssue>,
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

/// NAT detection status
#[derive(Debug, Clone, Serialize)]
pub struct NatStatusResponse {
    /// Detected NAT type
    pub nat_type: String,
    /// Human-readable description of the NAT type
    pub description: String,
    /// External IP discovered via STUN
    pub external_ip_stun: Option<String>,
    /// External port discovered via STUN
    pub external_port_stun: Option<u16>,
    /// Whether NAT detection was successful
    pub detection_successful: bool,
    /// Error message if detection failed
    pub error: Option<String>,
}

/// Information about a mapped port
#[derive(Debug, Clone, Serialize)]
pub struct MappedPortResponse {
    pub external_port: u16,
    pub internal_port: u16,
    pub protocol: String,
    pub success: bool,
}

/// Network issues that may affect remote streaming
#[derive(Debug, Clone, Serialize)]
pub struct NetworkIssue {
    /// Issue severity: "warning" or "error"
    pub severity: String,
    /// Issue code for programmatic handling
    pub code: String,
    /// Human-readable description
    pub message: String,
}

/// Get network status for remote streaming diagnostics
#[get("/network/status")]
pub async fn get_network_status(upnp_manager: Option<Data<UpnpManager>>) -> HttpResponse {
    let mut recommendations = Vec::new();
    let mut issues = Vec::new();
    let mut remote_accessible = false;
    let mut direct_connection_possible = false;
    let mut turn_recommended = false;

    // Collect external IPs for comparison
    let mut upnp_external_ip: Option<String> = None;

    // === UPnP Status ===
    let upnp_status = if let Some(manager) = upnp_manager {
        let status = manager.status().await;

        if status.available {
            if let Some(ip) = status.external_ip {
                upnp_external_ip = Some(ip.to_string());
                remote_accessible = true;
            }

            let successful_ports = status.port_mappings.iter().filter(|m| m.success).count();
            let failed_ports = status.port_mappings.iter().filter(|m| !m.success).count();

            if failed_ports > 0 {
                issues.push(NetworkIssue {
                    severity: "warning".to_string(),
                    code: "upnp_partial_failure".to_string(),
                    message: format!(
                        "{} port(s) failed to map via UPnP. Some features may not work remotely.",
                        failed_ports
                    ),
                });
            }

            if successful_ports > 0 {
                info!(
                    "[Network] UPnP successfully mapped {} ports",
                    successful_ports
                );
            }
        } else {
            issues.push(NetworkIssue {
                severity: "warning".to_string(),
                code: "upnp_unavailable".to_string(),
                message: "UPnP gateway not found. Automatic port forwarding unavailable."
                    .to_string(),
            });

            recommendations.push(
                "Enable UPnP on your router, or manually forward ports for remote streaming."
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
            "UPnP is disabled. Enable it in config for automatic port forwarding, \
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

    // === NAT Type Detection via STUN ===
    let nat_status = {
        let client = StunClient::new();
        let result = client.detect_nat_type();

        if result.success {
            let nat_type = result.nat_type;
            direct_connection_possible = nat_type.supports_direct_connection();
            turn_recommended = !direct_connection_possible;

            // Check for problematic NAT types
            match nat_type {
                NatType::Symmetric => {
                    issues.push(NetworkIssue {
                        severity: "warning".to_string(),
                        code: "symmetric_nat".to_string(),
                        message: "Symmetric NAT detected. Direct connections may fail; TURN relay recommended.".to_string(),
                    });
                    recommendations.push(
                        "Your network uses Symmetric NAT. Consider using a VPN like Tailscale, or configure a TURN server for reliable remote access.".to_string()
                    );
                }
                NatType::CarrierGradeNat => {
                    issues.push(NetworkIssue {
                        severity: "error".to_string(),
                        code: "cgnat_detected".to_string(),
                        message: "Carrier-Grade NAT (CGNAT) detected. You're behind ISP-level NAT.".to_string(),
                    });
                    recommendations.push(
                        "Your ISP uses Carrier-Grade NAT (CGNAT). Direct connections are not possible. Use Tailscale VPN or contact your ISP for a public IP.".to_string()
                    );
                    remote_accessible = false;
                }
                NatType::DoubleNat => {
                    issues.push(NetworkIssue {
                        severity: "warning".to_string(),
                        code: "double_nat".to_string(),
                        message: "Double NAT detected. You may be behind multiple routers.".to_string(),
                    });
                    recommendations.push(
                        "Double NAT detected. Consider putting your inner router in bridge mode, or use Tailscale VPN for reliable remote access.".to_string()
                    );
                }
                NatType::Unknown => {
                    issues.push(NetworkIssue {
                        severity: "warning".to_string(),
                        code: "nat_unknown".to_string(),
                        message: "Could not determine NAT type. Remote connectivity is uncertain.".to_string(),
                    });
                }
                _ => {
                    // Good NAT types
                    if result.external_ip.is_some() {
                        remote_accessible = true;
                    }
                }
            }

            // Compare STUN and UPnP external IPs
            if let (Some(stun_ip), Some(upnp_ip)) =
                (result.external_ip.map(|ip| ip.to_string()), &upnp_external_ip)
            {
                if stun_ip != *upnp_ip {
                    issues.push(NetworkIssue {
                        severity: "warning".to_string(),
                        code: "ip_mismatch".to_string(),
                        message: format!(
                            "External IP mismatch: UPnP reports {} but STUN reports {}. This may indicate complex NAT or load balancing.",
                            upnp_ip, stun_ip
                        ),
                    });
                }
            }

            NatStatusResponse {
                nat_type: nat_type.as_str().to_string(),
                description: nat_type.description().to_string(),
                external_ip_stun: result.external_ip.map(|ip| ip.to_string()),
                external_port_stun: result.external_port,
                detection_successful: true,
                error: None,
            }
        } else {
            issues.push(NetworkIssue {
                severity: "warning".to_string(),
                code: "stun_failed".to_string(),
                message: "NAT type detection failed. Could not reach STUN servers.".to_string(),
            });

            NatStatusResponse {
                nat_type: NatType::Unknown.as_str().to_string(),
                description: NatType::Unknown.description().to_string(),
                external_ip_stun: None,
                external_port_stun: None,
                detection_successful: false,
                error: result.error,
            }
        }
    };

    // Get local addresses
    let local_addresses = get_local_addresses();

    // Add general recommendations if no specific ones
    if recommendations.is_empty() && !remote_accessible {
        recommendations.push(
            "Unable to verify remote accessibility. Consider using Tailscale VPN for reliable remote access without port forwarding.".to_string()
        );
    }

    // Add TURN recommendation if needed
    if turn_recommended && !recommendations.iter().any(|r| r.contains("TURN")) {
        recommendations.push(
            "Consider configuring a TURN server as fallback for users who can't establish direct connections.".to_string()
        );
    }

    let response = NetworkStatusResponse {
        upnp: upnp_status,
        nat: nat_status,
        local_addresses,
        remote_accessible,
        direct_connection_possible,
        turn_recommended,
        issues,
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

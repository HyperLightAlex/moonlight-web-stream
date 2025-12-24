//! UPnP (Universal Plug and Play) module for automatic port forwarding.
//!
//! This module provides automatic NAT traversal by configuring port forwarding
//! on compatible routers using the UPnP IGD (Internet Gateway Device) protocol.

use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};

use common::config::{PortRange, UpnpConfig};
use igd_next::{
    aio::tokio::Tokio,
    Gateway, PortMappingProtocol, SearchOptions,
};
use log::{debug, error, info, warn};
use tokio::sync::RwLock;

/// Result of a UPnP port mapping attempt
#[derive(Debug, Clone)]
pub struct PortMappingResult {
    pub external_ip: Ipv4Addr,
    pub external_port: u16,
    pub internal_port: u16,
    pub protocol: PortMappingProtocol,
    pub success: bool,
    pub error: Option<String>,
}

/// Status of the UPnP service
#[derive(Debug, Clone, Default)]
pub struct UpnpStatus {
    /// Whether UPnP is available on the network
    pub available: bool,
    /// External (public) IP address discovered via UPnP
    pub external_ip: Option<Ipv4Addr>,
    /// Gateway device description
    pub gateway_description: Option<String>,
    /// List of active port mappings
    pub port_mappings: Vec<PortMappingResult>,
    /// Last error message if UPnP setup failed
    pub last_error: Option<String>,
}

/// Manages UPnP port forwarding for the server
pub struct UpnpManager {
    config: UpnpConfig,
    server_port: u16,
    local_ip: Ipv4Addr,
    status: Arc<RwLock<UpnpStatus>>,
    gateway: Arc<RwLock<Option<Gateway>>>,
}

impl UpnpManager {
    /// Create a new UPnP manager
    pub fn new(config: UpnpConfig, server_port: u16, local_ip: Ipv4Addr) -> Self {
        Self {
            config,
            server_port,
            local_ip,
            status: Arc::new(RwLock::new(UpnpStatus::default())),
            gateway: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize UPnP and set up port forwarding
    pub async fn initialize(&self) -> Result<UpnpStatus, String> {
        if !self.config.enabled {
            info!("[UPnP] UPnP is disabled in configuration");
            return Ok(UpnpStatus::default());
        }

        info!("[UPnP] Searching for UPnP gateway device...");

        // Search for gateway with timeout
        let search_options = SearchOptions {
            timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        };

        let gateway = match igd_next::aio::tokio::search_gateway(search_options).await {
            Ok(gw) => {
                info!("[UPnP] Found gateway: {}", gw.addr);
                gw
            }
            Err(e) => {
                let error_msg = format!("Failed to find UPnP gateway: {e}");
                warn!("[UPnP] {}", error_msg);
                let mut status = self.status.write().await;
                status.available = false;
                status.last_error = Some(error_msg.clone());
                return Err(error_msg);
            }
        };

        // Get external IP
        let external_ip = match gateway.get_external_ip().await {
            Ok(ip) => {
                info!("[UPnP] External IP address: {}", ip);
                Some(ip)
            }
            Err(e) => {
                warn!("[UPnP] Failed to get external IP: {e}");
                None
            }
        };

        // Store gateway for later use
        *self.gateway.write().await = Some(gateway.clone());

        // Update status
        {
            let mut status = self.status.write().await;
            status.available = true;
            status.external_ip = external_ip;
            status.gateway_description = Some(gateway.addr.to_string());
        }

        // Set up port mappings
        let mut mappings = Vec::new();

        // Forward the web server port (HTTP/HTTPS)
        let http_result = self
            .add_port_mapping(&gateway, self.server_port, PortMappingProtocol::TCP)
            .await;
        mappings.push(http_result);

        // Forward WebRTC ports if configured
        if let Some(port_range) = &self.config.webrtc_ports {
            let webrtc_mappings = self.forward_port_range(&gateway, port_range).await;
            mappings.extend(webrtc_mappings);
        }

        // Update status with mappings
        {
            let mut status = self.status.write().await;
            status.port_mappings = mappings;
        }

        let status = self.status.read().await.clone();
        self.log_status(&status);

        Ok(status)
    }

    /// Add a single port mapping
    async fn add_port_mapping(
        &self,
        gateway: &Gateway,
        port: u16,
        protocol: PortMappingProtocol,
    ) -> PortMappingResult {
        let local_addr = SocketAddrV4::new(self.local_ip, port);
        let description = format!("{} - {}", self.config.description, protocol_name(protocol));

        debug!(
            "[UPnP] Adding port mapping: {} -> {} ({})",
            port,
            local_addr,
            protocol_name(protocol)
        );

        match gateway
            .add_port(
                protocol,
                port,
                local_addr,
                self.config.lease_duration_secs,
                &description,
            )
            .await
        {
            Ok(()) => {
                info!(
                    "[UPnP] Successfully mapped port {} ({}) -> {}",
                    port,
                    protocol_name(protocol),
                    local_addr
                );
                PortMappingResult {
                    external_ip: self.status.read().await.external_ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
                    external_port: port,
                    internal_port: port,
                    protocol,
                    success: true,
                    error: None,
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to map port {port}: {e}");
                warn!("[UPnP] {}", error_msg);
                PortMappingResult {
                    external_ip: Ipv4Addr::UNSPECIFIED,
                    external_port: port,
                    internal_port: port,
                    protocol,
                    success: false,
                    error: Some(error_msg),
                }
            }
        }
    }

    /// Forward a range of ports for WebRTC
    async fn forward_port_range(
        &self,
        gateway: &Gateway,
        range: &PortRange,
    ) -> Vec<PortMappingResult> {
        let mut results = Vec::new();

        info!(
            "[UPnP] Forwarding WebRTC port range: {}-{}",
            range.min, range.max
        );

        for port in range.min..=range.max {
            // Always forward UDP for WebRTC
            let udp_result = self.add_port_mapping(gateway, port, PortMappingProtocol::UDP).await;
            results.push(udp_result);

            // Optionally forward TCP for TURN fallback
            if self.config.forward_tcp {
                let tcp_result = self.add_port_mapping(gateway, port, PortMappingProtocol::TCP).await;
                results.push(tcp_result);
            }
        }

        results
    }

    /// Get current UPnP status
    pub async fn status(&self) -> UpnpStatus {
        self.status.read().await.clone()
    }

    /// Refresh the external IP address
    pub async fn refresh_external_ip(&self) -> Option<Ipv4Addr> {
        let gateway = self.gateway.read().await;
        if let Some(gw) = gateway.as_ref() {
            match gw.get_external_ip().await {
                Ok(ip) => {
                    let mut status = self.status.write().await;
                    status.external_ip = Some(ip);
                    Some(ip)
                }
                Err(e) => {
                    warn!("[UPnP] Failed to refresh external IP: {e}");
                    None
                }
            }
        } else {
            None
        }
    }

    /// Remove all port mappings (call on shutdown)
    pub async fn cleanup(&self) {
        let gateway = self.gateway.read().await;
        let Some(gw) = gateway.as_ref() else {
            return;
        };

        let status = self.status.read().await;
        for mapping in &status.port_mappings {
            if mapping.success {
                if let Err(e) = gw
                    .remove_port(mapping.protocol, mapping.external_port)
                    .await
                {
                    warn!(
                        "[UPnP] Failed to remove port mapping {}: {}",
                        mapping.external_port, e
                    );
                } else {
                    debug!(
                        "[UPnP] Removed port mapping {} ({})",
                        mapping.external_port,
                        protocol_name(mapping.protocol)
                    );
                }
            }
        }

        info!("[UPnP] Cleanup complete");
    }

    /// Log the current UPnP status
    fn log_status(&self, status: &UpnpStatus) {
        if !status.available {
            warn!("[UPnP] UPnP gateway not available");
            if let Some(err) = &status.last_error {
                warn!("[UPnP] Error: {}", err);
            }
            return;
        }

        info!("[UPnP] === UPnP Status ===");
        if let Some(ip) = status.external_ip {
            info!("[UPnP] External IP: {}", ip);
        }
        if let Some(gw) = &status.gateway_description {
            info!("[UPnP] Gateway: {}", gw);
        }

        let successful: Vec<_> = status.port_mappings.iter().filter(|m| m.success).collect();
        let failed: Vec<_> = status.port_mappings.iter().filter(|m| !m.success).collect();

        if !successful.is_empty() {
            info!("[UPnP] Successfully mapped {} ports:", successful.len());
            for m in &successful {
                info!(
                    "[UPnP]   - {} ({}) -> internal:{}",
                    m.external_port,
                    protocol_name(m.protocol),
                    m.internal_port
                );
            }
        }

        if !failed.is_empty() {
            warn!("[UPnP] Failed to map {} ports:", failed.len());
            for m in &failed {
                warn!(
                    "[UPnP]   - {} ({}): {}",
                    m.external_port,
                    protocol_name(m.protocol),
                    m.error.as_deref().unwrap_or("unknown error")
                );
            }
        }
    }
}

fn protocol_name(protocol: PortMappingProtocol) -> &'static str {
    match protocol {
        PortMappingProtocol::TCP => "TCP",
        PortMappingProtocol::UDP => "UDP",
    }
}

/// Helper to detect the local IP address to use for port forwarding
pub fn detect_local_ip() -> Option<Ipv4Addr> {
    // Try to get the local IP by creating a UDP socket and checking its address
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to a public DNS server (doesn't actually send data)
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;

    match local_addr {
        std::net::SocketAddr::V4(addr) => Some(*addr.ip()),
        std::net::SocketAddr::V6(_) => None,
    }
}


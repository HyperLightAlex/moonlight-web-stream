//! STUN (Session Traversal Utilities for NAT) module for NAT type detection.
//!
//! This module provides NAT type detection using STUN protocol, which helps
//! determine what kind of NAT the server is behind and whether direct
//! connections are possible.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::Duration,
};

use bytecodec::{DecodeExt, EncodeExt};
use log::{debug, info, warn};
use stun_codec::{
    rfc5389::{
        attributes::Software,
        methods::BINDING,
        Attribute,
    },
    Message, MessageClass, MessageDecoder, MessageEncoder, TransactionId,
};

/// Default STUN servers to use for NAT detection
pub const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun2.l.google.com:19302",
    "stun.cloudflare.com:3478",
];

/// NAT type classification based on RFC 3489
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// No NAT - direct public IP
    None,
    /// Full Cone NAT - any external host can send to mapped port
    FullCone,
    /// Restricted Cone NAT - only hosts we've sent to can reply
    RestrictedCone,
    /// Port Restricted Cone NAT - only hosts on same port we've sent to can reply
    PortRestricted,
    /// Symmetric NAT - different mapping for each destination (TURN required)
    Symmetric,
    /// Carrier-Grade NAT (100.64.x.x range) - ISP-level NAT
    CarrierGradeNat,
    /// Double NAT - behind multiple NAT layers
    DoubleNat,
    /// Could not determine NAT type
    Unknown,
}

impl NatType {
    /// Returns a human-readable description of the NAT type
    pub fn description(&self) -> &'static str {
        match self {
            NatType::None => "No NAT (direct public IP)",
            NatType::FullCone => "Full Cone NAT (best for P2P)",
            NatType::RestrictedCone => "Restricted Cone NAT (P2P possible)",
            NatType::PortRestricted => "Port Restricted Cone NAT (P2P may work)",
            NatType::Symmetric => "Symmetric NAT (TURN relay may be needed)",
            NatType::CarrierGradeNat => "Carrier-Grade NAT (ISP-level, TURN required)",
            NatType::DoubleNat => "Double NAT (complex setup, TURN recommended)",
            NatType::Unknown => "Unknown NAT type",
        }
    }

    /// Returns whether direct P2P connections are likely to work
    pub fn supports_direct_connection(&self) -> bool {
        matches!(
            self,
            NatType::None | NatType::FullCone | NatType::RestrictedCone | NatType::PortRestricted
        )
    }

    /// Serialize to string for JSON
    pub fn as_str(&self) -> &'static str {
        match self {
            NatType::None => "none",
            NatType::FullCone => "full_cone",
            NatType::RestrictedCone => "restricted_cone",
            NatType::PortRestricted => "port_restricted",
            NatType::Symmetric => "symmetric",
            NatType::CarrierGradeNat => "carrier_grade_nat",
            NatType::DoubleNat => "double_nat",
            NatType::Unknown => "unknown",
        }
    }
}

/// Result of STUN binding request
#[derive(Debug, Clone)]
pub struct StunResult {
    /// The external (mapped) IP address as seen by the STUN server
    pub external_ip: Ipv4Addr,
    /// The external (mapped) port as seen by the STUN server
    pub external_port: u16,
    /// The STUN server that was used
    pub stun_server: String,
}

/// Result of NAT type detection
#[derive(Debug, Clone)]
pub struct NatDetectionResult {
    /// Detected NAT type
    pub nat_type: NatType,
    /// External IP address (if detected)
    pub external_ip: Option<Ipv4Addr>,
    /// External port (if detected)
    pub external_port: Option<u16>,
    /// Whether the detection was successful
    pub success: bool,
    /// Error message if detection failed
    pub error: Option<String>,
}

/// STUN client for NAT detection
pub struct StunClient {
    timeout: Duration,
    stun_servers: Vec<String>,
}

impl Default for StunClient {
    fn default() -> Self {
        Self::new()
    }
}

impl StunClient {
    /// Create a new STUN client with default settings
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(3),
            stun_servers: DEFAULT_STUN_SERVERS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    /// Create a STUN client with custom STUN servers
    pub fn with_servers(servers: Vec<String>) -> Self {
        Self {
            timeout: Duration::from_secs(3),
            stun_servers: servers,
        }
    }

    /// Perform a simple STUN binding request to get external IP
    pub fn get_external_address(&self) -> Result<StunResult, String> {
        for server in &self.stun_servers {
            match self.binding_request(server) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    debug!("[STUN] Failed to query {}: {}", server, e);
                    continue;
                }
            }
        }
        Err("Failed to contact any STUN server".to_string())
    }

    /// Perform STUN binding request to a specific server
    fn binding_request(&self, server: &str) -> Result<StunResult, String> {
        // Resolve server address (prefer IPv4)
        let server_addr: SocketAddr = server
            .parse()
            .or_else(|_| {
                // Try to resolve hostname, preferring IPv4
                use std::net::ToSocketAddrs;
                let addrs: Vec<_> = server
                    .to_socket_addrs()
                    .map_err(|e| format!("Failed to resolve {}: {}", server, e))?
                    .collect();
                
                // Prefer IPv4 addresses
                addrs
                    .iter()
                    .find(|a| a.is_ipv4())
                    .or(addrs.first())
                    .copied()
                    .ok_or_else(|| format!("No addresses found for {}", server))
            })
            .map_err(|e| format!("Invalid server address {}: {}", server, e))?;

        // Create UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("Failed to bind UDP socket: {}", e))?;

        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;

        // Build STUN binding request
        let transaction_id = TransactionId::new(rand_transaction_id());
        let mut message = Message::<Attribute>::new(MessageClass::Request, BINDING, transaction_id);
        message.add_attribute(Attribute::Software(Software::new(
            "moonlight-web".to_string(),
        ).map_err(|e| format!("Failed to create software attribute: {}", e))?));

        // Encode message
        let mut encoder = MessageEncoder::new();
        let request_bytes = encoder
            .encode_into_bytes(message)
            .map_err(|e| format!("Failed to encode STUN request: {}", e))?;

        // Send request
        socket
            .send_to(&request_bytes, server_addr)
            .map_err(|e| format!("Failed to send STUN request: {}", e))?;

        // Receive response
        let mut buf = [0u8; 1024];
        let (len, _) = socket
            .recv_from(&mut buf)
            .map_err(|e| format!("Failed to receive STUN response: {}", e))?;

        // Decode response
        let mut decoder = MessageDecoder::<Attribute>::new();
        let response = decoder
            .decode_from_bytes(&buf[..len])
            .map_err(|e| format!("Failed to decode STUN response: {}", e))?
            .map_err(|e| format!("Incomplete STUN response: {:?}", e))?;

        // Verify transaction ID
        if response.transaction_id() != transaction_id {
            return Err("Transaction ID mismatch".to_string());
        }

        // Extract mapped address
        let mapped_addr = extract_mapped_address(&response)?;

        match mapped_addr {
            IpAddr::V4(ip) => Ok(StunResult {
                external_ip: ip,
                external_port: extract_mapped_port(&response).unwrap_or(0),
                stun_server: server.to_string(),
            }),
            IpAddr::V6(_) => Err("IPv6 addresses not supported yet".to_string()),
        }
    }

    /// Detect NAT type using multiple STUN tests
    pub fn detect_nat_type(&self) -> NatDetectionResult {
        info!("[STUN] Starting NAT type detection...");

        // Step 1: Get external address from first server
        let first_result = match self.get_external_address() {
            Ok(result) => {
                info!(
                    "[STUN] External address: {}:{}",
                    result.external_ip, result.external_port
                );
                result
            }
            Err(e) => {
                warn!("[STUN] Failed to get external address: {}", e);
                return NatDetectionResult {
                    nat_type: NatType::Unknown,
                    external_ip: None,
                    external_port: None,
                    success: false,
                    error: Some(e),
                };
            }
        };

        // Check for CGNAT (100.64.0.0/10)
        if is_cgnat_address(first_result.external_ip) {
            info!("[STUN] Detected Carrier-Grade NAT (CGNAT)");
            return NatDetectionResult {
                nat_type: NatType::CarrierGradeNat,
                external_ip: Some(first_result.external_ip),
                external_port: Some(first_result.external_port),
                success: true,
                error: None,
            };
        }

        // Check if we have a direct public IP (no NAT)
        if let Some(local_ip) = detect_local_ip() {
            if local_ip == first_result.external_ip {
                info!("[STUN] No NAT detected - direct public IP");
                return NatDetectionResult {
                    nat_type: NatType::None,
                    external_ip: Some(first_result.external_ip),
                    external_port: Some(first_result.external_port),
                    success: true,
                    error: None,
                };
            }
        }

        // Step 2: Query a second STUN server to detect Symmetric NAT
        if self.stun_servers.len() >= 2 {
            let second_server = &self.stun_servers[1];
            if let Ok(second_result) = self.binding_request(second_server) {
                // If external port differs between servers, it's Symmetric NAT
                if second_result.external_port != first_result.external_port {
                    info!(
                        "[STUN] Symmetric NAT detected - ports differ: {} vs {}",
                        first_result.external_port, second_result.external_port
                    );
                    return NatDetectionResult {
                        nat_type: NatType::Symmetric,
                        external_ip: Some(first_result.external_ip),
                        external_port: Some(first_result.external_port),
                        success: true,
                        error: None,
                    };
                }

                // If external IP differs, might be load-balanced or complex NAT
                if second_result.external_ip != first_result.external_ip {
                    info!(
                        "[STUN] External IPs differ between servers - may be Double NAT or load balanced"
                    );
                    return NatDetectionResult {
                        nat_type: NatType::DoubleNat,
                        external_ip: Some(first_result.external_ip),
                        external_port: Some(first_result.external_port),
                        success: true,
                        error: None,
                    };
                }
            }
        }

        // If we got here with consistent results, assume Port Restricted (most common)
        // Full detection of Full Cone vs Restricted vs Port Restricted requires
        // CHANGE-REQUEST which most modern STUN servers don't support
        info!("[STUN] NAT detected - assuming Port Restricted (most common)");
        NatDetectionResult {
            nat_type: NatType::PortRestricted,
            external_ip: Some(first_result.external_ip),
            external_port: Some(first_result.external_port),
            success: true,
            error: None,
        }
    }
}

/// Extract mapped address from STUN response
fn extract_mapped_address(response: &Message<Attribute>) -> Result<IpAddr, String> {
    // First try XOR-MAPPED-ADDRESS (preferred, RFC 5389)
    for attr in response.attributes() {
        if let Attribute::XorMappedAddress(xma) = attr {
            return Ok(xma.address().ip());
        }
    }

    // Fall back to MAPPED-ADDRESS (RFC 3489)
    for attr in response.attributes() {
        if let Attribute::MappedAddress(ma) = attr {
            return Ok(ma.address().ip());
        }
    }

    Err("No mapped address in STUN response".to_string())
}

/// Extract mapped port from STUN response
fn extract_mapped_port(response: &Message<Attribute>) -> Option<u16> {
    // First try XOR-MAPPED-ADDRESS
    for attr in response.attributes() {
        if let Attribute::XorMappedAddress(xma) = attr {
            return Some(xma.address().port());
        }
    }

    // Fall back to MAPPED-ADDRESS
    for attr in response.attributes() {
        if let Attribute::MappedAddress(ma) = attr {
            return Some(ma.address().port());
        }
    }

    None
}

/// Generate random transaction ID
fn rand_transaction_id() -> [u8; 12] {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    
    let mut id = [0u8; 12];
    let nanos = now.as_nanos() as u64;
    let secs = now.as_secs();
    
    // Mix time-based values for pseudo-randomness
    id[0..8].copy_from_slice(&nanos.to_le_bytes());
    id[8..12].copy_from_slice(&(secs as u32).to_le_bytes());
    
    // XOR with process ID for more uniqueness
    let pid = std::process::id();
    id[0] ^= (pid & 0xFF) as u8;
    id[1] ^= ((pid >> 8) & 0xFF) as u8;
    
    id
}

/// Check if an IP is in the CGNAT range (100.64.0.0/10)
fn is_cgnat_address(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127)
}

/// Detect local IP address
fn detect_local_ip() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    
    match addr {
        SocketAddr::V4(v4) => Some(*v4.ip()),
        SocketAddr::V6(_) => None,
    }
}

/// Check if a port is accessible from the internet
/// This makes an HTTP request to a port-checking service
pub async fn check_port_accessible(ip: Ipv4Addr, port: u16) -> Result<bool, String> {
    // For now, we'll just return unknown - implementing a reliable port check
    // requires either a callback service or specific port-check API
    // This is a placeholder for future implementation
    debug!("[STUN] Port accessibility check not yet implemented for {}:{}", ip, port);
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cgnat_detection() {
        assert!(is_cgnat_address(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_cgnat_address(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_cgnat_address(Ipv4Addr::new(100, 63, 0, 1)));
        assert!(!is_cgnat_address(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_cgnat_address(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn test_nat_type_str() {
        assert_eq!(NatType::None.as_str(), "none");
        assert_eq!(NatType::Symmetric.as_str(), "symmetric");
        assert_eq!(NatType::CarrierGradeNat.as_str(), "carrier_grade_nat");
    }
}


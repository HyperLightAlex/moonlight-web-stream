//! Certificate handling for WebTransport
//!
//! Generates self-signed certificates and calculates SHA-256 hash for client validation.

use std::path::Path;
use anyhow::{Context, Result};
use log::info;
use sha2::{Digest, Sha256};

/// Certificate and private key pair for WebTransport
pub struct CertPair {
    /// The wtransport Identity (cert chain + private key)
    pub identity: wtransport::tls::Identity,
    /// SHA-256 hash of the certificate (hex encoded)
    pub cert_hash: String,
}

impl CertPair {
    /// Generate a self-signed certificate using wtransport's built-in helper
    pub fn generate_self_signed() -> Result<Self> {
        info!("[WebTransport]: Generating self-signed certificate");
        
        // Use wtransport's self-signed certificate builder
        let identity = wtransport::tls::Identity::self_signed_builder()
            .subject_alt_names(&["localhost", "127.0.0.1", "::1"])
            .from_now_utc()
            .validity_days(365) // 1 year
            .build()
            .context("Failed to generate self-signed certificate")?;
        
        // Get the certificate DER bytes for hash calculation
        let cert = identity.certificate_chain().as_slice().first()
            .context("No certificate in chain")?;
        
        // Calculate SHA-256 hash of the DER-encoded certificate
        let der_bytes = cert.der();
        let hash = Sha256::digest(der_bytes);
        let cert_hash = hex::encode(hash);
        
        info!("[WebTransport]: Certificate generated, hash: {}", cert_hash);
        
        Ok(Self {
            identity,
            cert_hash,
        })
    }
    
    /// Load certificate from PEM files (async version)
    pub async fn load_from_files_async(cert_path: &Path, key_path: &Path) -> Result<Self> {
        info!(
            "[WebTransport]: Loading certificate from {} and {}",
            cert_path.display(),
            key_path.display()
        );
        
        // Load certificate chain from PEM file (async)
        let cert_chain = wtransport::tls::CertificateChain::load_pemfile(cert_path).await
            .with_context(|| format!("Failed to load certificate from {}", cert_path.display()))?;
        
        // Load private key from PEM file (async)
        let private_key = wtransport::tls::PrivateKey::load_pemfile(key_path).await
            .with_context(|| format!("Failed to load private key from {}", key_path.display()))?;
        
        // Create identity
        let identity = wtransport::tls::Identity::new(cert_chain.clone(), private_key);
        
        // Get the certificate DER bytes for hash calculation
        let cert = cert_chain.as_slice().first()
            .context("No certificate in chain")?;
        let der_bytes = cert.der();
        let hash = Sha256::digest(der_bytes);
        let cert_hash = hex::encode(hash);
        
        info!("[WebTransport]: Certificate loaded, hash: {}", cert_hash);
        
        Ok(Self {
            identity,
            cert_hash,
        })
    }
    
    /// Get certificate hash as hex string
    pub fn hash(&self) -> &str {
        &self.cert_hash
    }
    
    /// Consume and return the wtransport Identity
    pub fn into_identity(self) -> wtransport::tls::Identity {
        self.identity
    }
}

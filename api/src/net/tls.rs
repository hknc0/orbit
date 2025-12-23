use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::digest::{digest, SHA256};
use std::env;
use std::path::Path;
use tracing::info;
use wtransport::Identity;

// Dev certificate paths (generated via `make setup`)
const DEV_CERT_FILE: &str = "certs/cert.pem";
const DEV_KEY_FILE: &str = "certs/key.pem";

/// TLS configuration for WebTransport server
pub struct TlsConfig {
    /// The wtransport Identity containing certificate and key
    pub identity: Identity,
    /// Base64-encoded SHA-256 hash of the certificate (for browser flag)
    pub cert_hash: String,
}

impl TlsConfig {
    /// Load TLS configuration
    ///
    /// Production: Set TLS_CERT_PATH and TLS_KEY_PATH env vars
    /// Development: Run `make setup` first to generate certs/
    pub async fn load() -> Result<Self> {
        // Production: load from env-specified paths
        if let (Ok(cert_path), Ok(key_path)) =
            (env::var("TLS_CERT_PATH"), env::var("TLS_KEY_PATH"))
        {
            info!("Loading TLS certificate from environment paths");
            return Self::load_from_paths(&cert_path, &key_path).await;
        }

        // Development: load from certs/ directory
        if Path::new(DEV_CERT_FILE).exists() && Path::new(DEV_KEY_FILE).exists() {
            info!("Loading dev certificate from certs/");
            Self::load_from_paths(DEV_CERT_FILE, DEV_KEY_FILE).await
        } else {
            Err(anyhow!(
                "TLS certificate not found.\n\n\
                For development: Run `make setup` to generate dev certificates.\n\
                For production: Set TLS_CERT_PATH and TLS_KEY_PATH environment variables."
            ))
        }
    }

    /// Load certificate from PEM file paths
    async fn load_from_paths(cert_path: &str, key_path: &str) -> Result<Self> {
        let identity = Identity::load_pemfiles(cert_path, key_path)
            .await
            .context("Failed to load certificate from PEM files")?;

        let cert_hash = Self::compute_cert_hash(&identity);
        Self::log_cert_info(&cert_hash);

        Ok(Self {
            identity,
            cert_hash,
        })
    }

    /// Generate a self-signed certificate (for compatibility)
    pub async fn generate_self_signed() -> Result<Self> {
        Self::load().await
    }

    fn compute_cert_hash(identity: &Identity) -> String {
        identity
            .certificate_chain()
            .as_slice()
            .first()
            .map(|cert| {
                let der_bytes = cert.der();
                let hash = digest(&SHA256, der_bytes);
                STANDARD.encode(hash.as_ref())
            })
            .unwrap_or_default()
    }

    fn log_cert_info(cert_hash: &str) {
        info!("Certificate hash: {}", cert_hash);
        info!(
            "Chrome flag: --ignore-certificate-errors-spki-list={}",
            cert_hash
        );
    }

    /// Get the certificate hash for client configuration
    pub fn get_cert_hash(&self) -> &str {
        &self.cert_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires `make setup` to be run first
    async fn test_load_cert() {
        let config = TlsConfig::load().await.unwrap();
        assert!(!config.cert_hash.is_empty());
    }

    #[tokio::test]
    #[ignore] // Requires `make setup` to be run first
    async fn test_cert_hash_format() {
        let config = TlsConfig::load().await.unwrap();
        // Should be valid base64
        let decoded = STANDARD.decode(&config.cert_hash);
        assert!(decoded.is_ok());
        // SHA-256 produces 32 bytes
        assert_eq!(decoded.unwrap().len(), 32);
    }

    #[tokio::test]
    #[ignore] // Requires `make setup` to be run first
    async fn test_persistent_cert_same_hash() {
        // Loading twice should return the same hash (cert persisted)
        let config1 = TlsConfig::load().await.unwrap();
        let config2 = TlsConfig::load().await.unwrap();
        assert_eq!(config1.cert_hash, config2.cert_hash);
    }

    #[test]
    fn test_missing_cert_error() {
        // Verify error message is helpful when certs are missing
        let err_msg = "TLS certificate not found";
        assert!(err_msg.contains("certificate"));
    }
}

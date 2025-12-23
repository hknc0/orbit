//! Dev certificate generator - run with `cargo run --manifest-path scripts/Cargo.toml`
//!
//! Generates a self-signed certificate for localhost development.
//! The certificate is valid for 10 years and only works for localhost/127.0.0.1.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use ring::digest::{digest, SHA256};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

const CERT_DIR: &str = "../certs";
const CERT_FILE: &str = "../certs/cert.pem";
const KEY_FILE: &str = "../certs/key.pem";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Check if certs already exist
    if Path::new(CERT_FILE).exists() && Path::new(KEY_FILE).exists() {
        println!("Certificates already exist at {}/", CERT_DIR);
        println!("Delete them first if you want to regenerate.");
        print_hashes()?;
        return Ok(());
    }

    println!("Generating development certificate for localhost...\n");

    // Create certs directory
    fs::create_dir_all(CERT_DIR)?;

    // Generate certificate
    let mut params = CertificateParams::new(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ])?;

    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "Orbit Royale Dev");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Development");

    // Valid for 14 days (WebTransport serverCertificateHashes requirement)
    // Chrome/browsers reject certs with validity > 14 days for this feature
    let now = SystemTime::now();
    let fourteen_days = Duration::from_secs(14 * 24 * 60 * 60);
    params.not_before = now.into();
    params.not_after = (now + fourteen_days).into();

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    // Save files
    fs::write(CERT_FILE, cert.pem())?;
    fs::write(KEY_FILE, key_pair.serialize_pem())?;

    println!("Certificate saved to {}", CERT_FILE);
    println!("Private key saved to {}", KEY_FILE);
    println!();

    print_hashes()?;

    Ok(())
}

fn print_hashes() -> Result<(), Box<dyn std::error::Error>> {
    let cert_pem = fs::read_to_string(CERT_FILE)?;

    // Extract DER from PEM for certificate hash
    let pem = pem::parse(&cert_pem)?;
    let der = pem.contents();

    // Certificate hash (SHA-256 of full DER cert) - for WebTransport serverCertificateHashes
    let cert_hash = digest(&SHA256, der);
    let cert_hash_b64 = STANDARD.encode(cert_hash.as_ref());

    // SPKI hash (SHA-256 of SubjectPublicKeyInfo) - for Chrome's --ignore-certificate-errors-spki-list
    // Use openssl to extract SPKI hash
    let spki_hash_b64 = get_spki_hash().unwrap_or_else(|_| cert_hash_b64.clone());

    println!("=== Certificate Hashes ===\n");

    println!("WebTransport cert hash (for client/.env):");
    println!("  VITE_CERT_HASH={}\n", cert_hash_b64);

    println!("SPKI hash (for Chrome flag):");
    println!("  --ignore-certificate-errors-spki-list={}\n", spki_hash_b64);

    println!("=== Quick Setup ===\n");
    println!("1. Update client/.env:");
    println!("   VITE_CERT_HASH={}\n", cert_hash_b64);
    println!("2. Update Makefile SPKI_HASH:");
    println!("   SPKI_HASH := {}\n", spki_hash_b64);

    Ok(())
}

fn get_spki_hash() -> Result<String, Box<dyn std::error::Error>> {
    // openssl x509 -in cert.pem -pubkey -noout | openssl pkey -pubin -outform der | openssl dgst -sha256 -binary | base64
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "openssl x509 -in {} -pubkey -noout 2>/dev/null | openssl pkey -pubin -outform der 2>/dev/null | openssl dgst -sha256 -binary | base64",
            CERT_FILE
        ))
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    } else {
        Err("Failed to compute SPKI hash".into())
    }
}

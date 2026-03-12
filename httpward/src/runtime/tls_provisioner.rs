use httpward_core::core::server_models::site_manager::TlsPaths;
use rcgen::generate_simple_self_signed;
use std::env;
use std::fs;
use tracing::{debug, info};

/// Provisions a self-signed certificate for a list of domains in the temp directory.
/// The primary directory name is created by concatenating all domains with * and . replaced by _.
pub fn provision_self_signed(domains: &[String]) -> Result<TlsPaths, Box<dyn std::error::Error + Send + Sync>> {
    if domains.is_empty() {
        return Err("No domains provided for self-signed certificate generation".into());
    }

    // 1. Create primary domain by concatenating all domains with * and . replaced by _
    let primary_domain = domains
        .iter()
        .map(|d| d.replace('*', "_").replace('.', "_"))
        .collect::<Vec<_>>()
        .join("_");
    let mut temp_dir = env::temp_dir();
    temp_dir.push("httpward");
    temp_dir.push("certs");
    temp_dir.push(&primary_domain);

    fs::create_dir_all(&temp_dir)?;

    let cert_path = temp_dir.join("cert.pem");
    let key_path = temp_dir.join("key.pem");

    // 2. Check if certificates exist
    // Note: In production, you might want to verify if the existing cert
    // actually contains all the requested domains, but for local dev,
    // checking existence is usually enough.
    if cert_path.exists() && key_path.exists() {
        debug!("Using existing self-signed certs for {} at {:?}", primary_domain, temp_dir);
        return Ok(TlsPaths { cert: cert_path, key: key_path });
    }

    // 3. Prepare Subject Alternative Names (SANs)
    // We include localhost and 127.0.0.1 by default for convenience
    let mut subject_alt_names = domains.to_vec();
    if !subject_alt_names.contains(&"localhost".to_string()) {
        subject_alt_names.push("localhost".to_string());
    }
    if !subject_alt_names.contains(&"127.0.0.1".to_string()) {
        subject_alt_names.push("127.0.0.1".to_string());
    }

    info!("Generating new self-signed certificate for domains: {:?}", subject_alt_names);

    // 4. Generate new certificate
    let cert = generate_simple_self_signed(subject_alt_names)?;

    // 5. Write PEM files
    fs::write(&cert_path, cert.cert.pem())?;
    fs::write(&key_path, cert.signing_key.serialize_pem())?;

    Ok(TlsPaths {
        cert: cert_path,
        key: key_path,
    })
}

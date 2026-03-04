use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::{Watcher, RecursiveMode, RecommendedWatcher, Event, EventKind};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, error};

use crate::runtime::server_instance::TlsMapping;
use super::tls::{FallbackSniResolver, build_certified_key, TlsError};
use rama_tls_rustls::dep::rustls::sign::CertifiedKey;

/// File watcher for TLS certificate changes with debouncing
pub struct TlsFileWatcher {
    mappings: Vec<TlsMapping>,
    resolver: Arc<FallbackSniResolver>,
    file_to_domains: HashMap<PathBuf, Vec<String>>,
    debounce_delay: Duration,
}

impl TlsFileWatcher {
    /// Create a new TLS file watcher
    pub fn new(
        mappings: Vec<TlsMapping>, 
        resolver: Arc<FallbackSniResolver>,
    ) -> Self {
        let file_to_domains = Self::build_file_to_domains(&mappings);
        Self {
            mappings,
            resolver,
            file_to_domains,
            debounce_delay: Duration::from_millis(500), // 500ms debounce delay
        }
    }

    /// Create a new TLS file watcher with async callback
    pub fn new_with_async_callback(
        mappings: Vec<TlsMapping>, 
        resolver: Arc<FallbackSniResolver>,
    ) -> Self {
        let file_to_domains = Self::build_file_to_domains(&mappings);
        Self {
            mappings,
            resolver,
            file_to_domains,
            debounce_delay: Duration::from_millis(500), // 500ms debounce delay
        }
    }

    /// Build reverse mapping: file path -> domains
    fn build_file_to_domains(mappings: &[TlsMapping]) -> HashMap<PathBuf, Vec<String>> {
        let mut file_to_domains: HashMap<PathBuf, Vec<String>> = HashMap::new();
        
        for mapping in mappings {
            for domain in &mapping.domains {
                file_to_domains
                    .entry(mapping.paths.cert.clone())
                    .or_insert_with(Vec::new)
                    .push(domain.clone());
                file_to_domains
                    .entry(mapping.paths.key.clone())
                    .or_insert_with(Vec::new)
                    .push(domain.clone());
            }
        }
        file_to_domains
    }

    /// Set custom debounce delay
    pub fn with_debounce_delay(mut self, delay: Duration) -> Self {
        self.debounce_delay = delay;
        self
    }

    /// Run the file watcher
    pub async fn run(self) -> Result<(), TlsError> {
        let (tx, mut rx) = mpsc::channel::<PathBuf>(100);

        // Create watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Only handle modify and create events
                        if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                            for path in event.paths {
                                if let Err(e) = tx.blocking_send(path) {
                                    error!("Failed to send file event: {:?}", e);
                                }
                            }
                        }
                    }
                    Err(e) => error!("File watcher error: {:?}", e),
                }
            },
            notify::Config::default(),
        )?;

        // Watch all certificate and key files
        let mut watched_paths = std::collections::HashSet::new();
        for mapping in &self.mappings {
            if !watched_paths.contains(&mapping.paths.cert) {
                watcher.watch(&mapping.paths.cert, RecursiveMode::NonRecursive)?;
                watched_paths.insert(mapping.paths.cert.clone());
                info!("Watching certificate file: {:?}", mapping.paths.cert);
            }
            
            if !watched_paths.contains(&mapping.paths.key) {
                watcher.watch(&mapping.paths.key, RecursiveMode::NonRecursive)?;
                watched_paths.insert(mapping.paths.key.clone());
                info!("Watching private key file: {:?}", mapping.paths.key);
            }
        }

        info!("TLS file watcher started successfully with {}ms debounce delay", self.debounce_delay.as_millis());

        // Process events with debouncing
        let mut pending_changes: HashMap<PathBuf, tokio::time::Instant> = HashMap::new();
        let mut debounce_task: Option<tokio::task::JoinHandle<()>> = None;
        
        while let Some(path) = rx.recv().await {
            let now = tokio::time::Instant::now();
            pending_changes.insert(path.clone(), now);

            // Cancel previous debounce task if exists
            if let Some(task) = debounce_task.take() {
                task.abort();
            }

            // Start new debounce task
            let pending_paths = pending_changes.clone();
            let debounce_delay = self.debounce_delay;
            let file_to_domains = self.file_to_domains.clone();
            let mappings = self.mappings.clone();
            let resolver = self.resolver.clone();
            
            debounce_task = Some(tokio::spawn(async move {
                sleep(debounce_delay).await;
                
                // Process all pending changes after debounce delay
                for (path, _) in pending_paths {
                    if let Some(domains) = file_to_domains.get(&path) {
                        info!("Processing debounced file change: {:?}", path);
                        
                        for domain in domains {
                            if let Some(mapping) = mappings.iter().find(|m| m.domains.contains(domain)) {
                                match Self::reload_single_mapping_static(mapping).await {
                                    Ok(certified_key) => {
                                        resolver.update_domain_certificate(Some(&domain), certified_key);
                                        info!("Successfully reloaded TLS certificate for domain: {}", domain);
                                    }
                                    Err(e) => {
                                        error!("Failed to reload TLS certificate for domain {}: {:?}", domain, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }));
        }

        Ok(())
    }

    /// Static version of reload_single_mapping for use in async blocks
    pub async fn reload_single_mapping_static(mapping: &TlsMapping) -> Result<Arc<CertifiedKey>, TlsError> {
        build_certified_key(&mapping.paths.cert, &mapping.paths.key).await
    }
}

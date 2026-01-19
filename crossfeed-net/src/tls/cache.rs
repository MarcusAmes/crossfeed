use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use super::types::{LeafCertificate, TlsError, TlsErrorKind};

#[derive(Debug)]
pub struct CertCache {
    max_entries: usize,
    order: VecDeque<String>,
    entries: HashMap<String, LeafCertificate>,
    disk_path: Option<PathBuf>,
}

impl CertCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            order: VecDeque::new(),
            entries: HashMap::new(),
            disk_path: None,
        }
    }

    pub fn with_disk_path(max_entries: usize, path: impl AsRef<Path>) -> Self {
        Self {
            max_entries,
            order: VecDeque::new(),
            entries: HashMap::new(),
            disk_path: Some(path.as_ref().to_path_buf()),
        }
    }

    pub fn get(&mut self, host: &str) -> Option<LeafCertificate> {
        if let Some(cert) = self.entries.get(host).cloned() {
            self.touch(host);
            return Some(cert);
        }
        if let Some(path) = &self.disk_path {
            if let Ok(cert) = self.load_from_disk(path, host) {
                self.insert(host.to_string(), cert.clone());
                return Some(cert);
            }
        }
        None
    }

    pub fn insert(&mut self, host: String, cert: LeafCertificate) {
        if !self.entries.contains_key(&host) {
            self.order.push_back(host.clone());
        }
        self.entries.insert(host.clone(), cert);
        self.touch(&host);
        self.evict_if_needed();
    }

    pub fn persist(&self, host: &str, cert: &LeafCertificate) -> Result<(), TlsError> {
        let Some(path) = &self.disk_path else {
            return Ok(());
        };
        fs::create_dir_all(path).map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;

        let cert_path = path.join(format!("{host}.pem"));
        let key_path = path.join(format!("{host}.key"));
        fs::write(cert_path, &cert.cert_pem)
            .map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;
        fs::write(key_path, &cert.key_pem)
            .map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;
        Ok(())
    }

    fn load_from_disk(&self, path: &Path, host: &str) -> Result<LeafCertificate, TlsError> {
        let cert_path = path.join(format!("{host}.pem"));
        let key_path = path.join(format!("{host}.key"));
        let cert_pem =
            fs::read(cert_path).map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;
        let key_pem =
            fs::read(key_path).map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;
        Ok(LeafCertificate { cert_pem, key_pem })
    }

    fn touch(&mut self, host: &str) {
        if let Some(pos) = self.order.iter().position(|item| item == host) {
            self.order.remove(pos);
            self.order.push_back(host.to_string());
        }
    }

    fn evict_if_needed(&mut self) {
        while self.order.len() > self.max_entries {
            if let Some(host) = self.order.pop_front() {
                self.entries.remove(&host);
            }
        }
    }
}

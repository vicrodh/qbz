//! Chromecast device discovery via mDNS

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;

use crate::cast::CastError;

const SERVICE_TYPE: &str = "_googlecast._tcp.local.";

/// Discovered Chromecast device
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub port: u16,
}

#[derive(Default)]
struct DiscoveryState {
    devices: HashMap<String, DiscoveredDevice>,
    fullname_to_id: HashMap<String, String>,
}

/// Discovery manager for Chromecast devices
pub struct DeviceDiscovery {
    state: Arc<Mutex<DiscoveryState>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    daemon: Option<ServiceDaemon>,
}

impl DeviceDiscovery {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DiscoveryState::default())),
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
            daemon: None,
        }
    }

    /// Start mDNS discovery in background
    pub fn start_discovery(&mut self) -> Result<(), CastError> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let mdns = ServiceDaemon::new()
            .map_err(|e| CastError::Discovery(format!("Failed to create mDNS daemon: {}", e)))?;
        let receiver = mdns
            .browse(SERVICE_TYPE)
            .map_err(|e| CastError::Discovery(format!("Failed to browse mDNS services: {}", e)))?;

        let running = self.running.clone();
        let state = self.state.clone();

        running.store(true, Ordering::SeqCst);

        let handle = thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                match receiver.recv_timeout(Duration::from_millis(250)) {
                    Ok(Some(event)) => match event {
                        ServiceEvent::ServiceResolved(info) => {
                            let fullname = info.get_fullname().to_string();
                            let id = info
                                .get_property_val_str("id")
                                .unwrap_or_else(|| fullname.as_str())
                                .to_string();
                            let name = info
                                .get_property_val_str("fn")
                                .unwrap_or_else(|| id.as_str())
                                .to_string();
                            let model = info
                                .get_property_val_str("md")
                                .unwrap_or("Unknown")
                                .to_string();

                            let ip = pick_ip(info.get_addresses())
                                .unwrap_or_else(|| "127.0.0.1".to_string());

                            let device = DiscoveredDevice {
                                id: id.clone(),
                                name,
                                model,
                                ip,
                                port: info.get_port(),
                            };

                            if let Ok(mut state) = state.lock() {
                                state.fullname_to_id.insert(fullname, id.clone());
                                state.devices.insert(id, device);
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            if let Ok(mut state) = state.lock() {
                                if let Some(id) = state.fullname_to_id.remove(&fullname) {
                                    state.devices.remove(&id);
                                }
                            }
                        }
                        _ => {}
                    },
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        self.daemon = Some(mdns);
        self.handle = Some(handle);
        Ok(())
    }

    /// Stop discovery and release resources
    pub fn stop_discovery(&mut self) -> Result<(), CastError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(daemon) = self.daemon.take() {
            daemon
                .shutdown()
                .map_err(|e| CastError::Discovery(format!("Failed to shutdown mDNS: {}", e)))?;
        }

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }

        Ok(())
    }

    /// Return list of discovered devices
    pub fn get_discovered_devices(&self) -> Vec<DiscoveredDevice> {
        self.state
            .lock()
            .map(|state| state.devices.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get a specific device by ID
    pub fn get_device(&self, device_id: &str) -> Option<DiscoveredDevice> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.devices.get(device_id).cloned())
    }
}

impl Default for DeviceDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

fn pick_ip(addresses: &std::collections::HashSet<IpAddr>) -> Option<String> {
    let ipv4 = addresses.iter().find(|addr| addr.is_ipv4());
    let ip = ipv4.or_else(|| addresses.iter().next())?;
    Some(ip.to_string())
}

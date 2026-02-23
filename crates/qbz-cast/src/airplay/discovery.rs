//! AirPlay device discovery via mDNS

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;

use crate::AirPlayError;

const SERVICE_RAOP: &str = "_raop._tcp.local.";
const SERVICE_AIRPLAY: &str = "_airplay._tcp.local.";

/// Discovered AirPlay device
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredAirPlayDevice {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub port: u16,
    pub service: String,
    pub requires_password: bool,
}

#[derive(Default)]
struct DiscoveryState {
    devices: HashMap<String, DiscoveredAirPlayDevice>,
    fullname_to_id: HashMap<String, String>,
}

/// Discovery manager for AirPlay devices
pub struct AirPlayDiscovery {
    state: Arc<Mutex<DiscoveryState>>,
    running: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    daemon: Option<ServiceDaemon>,
}

impl AirPlayDiscovery {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DiscoveryState::default())),
            running: Arc::new(AtomicBool::new(false)),
            handles: Vec::new(),
            daemon: None,
        }
    }

    /// Start mDNS discovery in background
    pub fn start_discovery(&mut self) -> Result<(), AirPlayError> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let mdns = ServiceDaemon::new()
            .map_err(|e| AirPlayError::Discovery(format!("Failed to create mDNS daemon: {}", e)))?;

        let raop_rx = mdns
            .browse(SERVICE_RAOP)
            .map_err(|e| AirPlayError::Discovery(format!("Failed to browse RAOP services: {}", e)))?;
        let airplay_rx = mdns
            .browse(SERVICE_AIRPLAY)
            .map_err(|e| AirPlayError::Discovery(format!("Failed to browse AirPlay services: {}", e)))?;

        self.running.store(true, Ordering::SeqCst);

        let state = self.state.clone();
        let running = self.running.clone();
        self.handles.push(spawn_receiver(
            "raop",
            raop_rx,
            state.clone(),
            running.clone(),
        ));
        self.handles.push(spawn_receiver(
            "airplay",
            airplay_rx,
            state,
            running,
        ));

        self.daemon = Some(mdns);
        Ok(())
    }

    /// Stop discovery and release resources
    pub fn stop_discovery(&mut self) -> Result<(), AirPlayError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(daemon) = self.daemon.take() {
            daemon
                .shutdown()
                .map_err(|e| AirPlayError::Discovery(format!("Failed to shutdown mDNS: {}", e)))?;
        }

        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }

        Ok(())
    }

    /// Return list of discovered devices
    pub fn get_discovered_devices(&self) -> Vec<DiscoveredAirPlayDevice> {
        self.state
            .lock()
            .map(|state| state.devices.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get a specific device by ID
    pub fn get_device(&self, device_id: &str) -> Option<DiscoveredAirPlayDevice> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.devices.get(device_id).cloned())
    }
}

impl Default for AirPlayDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_receiver(
    service_label: &'static str,
    receiver: mdns_sd::Receiver<ServiceEvent>,
    state: Arc<Mutex<DiscoveryState>>,
    running: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for event in receiver.iter() {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            match event {
                ServiceEvent::ServiceResolved(info) => {
                    let fullname = info.get_fullname().to_string();
                    let id = device_id_from_info(&fullname, &info);
                    let name = device_name_from_info(&fullname, &info, &id);
                    let model = info
                        .get_property_val_str("am")
                        .or_else(|| info.get_property_val_str("model"))
                        .unwrap_or("Unknown")
                        .to_string();
                    let requires_password = parse_password_required(&info);
                    let ip = pick_ip(info.get_addresses())
                        .unwrap_or_else(|| "127.0.0.1".to_string());

                    let device = DiscoveredAirPlayDevice {
                        id: id.clone(),
                        name,
                        model,
                        ip,
                        port: info.get_port(),
                        service: service_label.to_string(),
                        requires_password,
                    };

                    let key = format!("{}|{}", service_label, fullname);
                    if let Ok(mut state) = state.lock() {
                        state.fullname_to_id.insert(key, id.clone());
                        state.devices.insert(id, device);
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    let key = format!("{}|{}", service_label, fullname);
                    if let Ok(mut state) = state.lock() {
                        if let Some(id) = state.fullname_to_id.remove(&key) {
                            state.devices.remove(&id);
                        }
                    }
                }
                _ => {}
            }
        }
    })
}

fn device_id_from_info(fullname: &str, info: &mdns_sd::ServiceInfo) -> String {
    if let Some(id) = info.get_property_val_str("deviceid") {
        return id.to_string();
    }
    if let Some(id) = info.get_property_val_str("id") {
        return id.to_string();
    }
    if let Some((prefix, _)) = fullname.split_once('@') {
        return prefix.to_string();
    }
    fullname.to_string()
}

fn device_name_from_info(fullname: &str, info: &mdns_sd::ServiceInfo, fallback: &str) -> String {
    if let Some(name) = info.get_property_val_str("fn") {
        return name.to_string();
    }
    if let Some((_, name)) = fullname.split_once('@') {
        return name.to_string();
    }
    fallback.to_string()
}

fn parse_password_required(info: &mdns_sd::ServiceInfo) -> bool {
    match info.get_property_val_str("pw") {
        Some(value) => matches!(value, "1" | "true" | "yes" | "on"),
        None => false,
    }
}

fn pick_ip(addresses: &std::collections::HashSet<IpAddr>) -> Option<String> {
    let ipv4 = addresses.iter().find(|addr| addr.is_ipv4());
    let ip = ipv4.or_else(|| addresses.iter().next())?;
    Some(ip.to_string())
}

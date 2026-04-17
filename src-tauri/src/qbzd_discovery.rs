//! mDNS discovery for qbzd daemon instances on the LAN.
//!
//! Uses the mdns-sd crate. Discovers `_qbz._tcp.local` services.

use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

const QBZD_SERVICE_TYPE: &str = "_qbz._tcp.local.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QbzdDevice {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub base_url: String,
}

pub struct QbzdDiscoveryState {
    devices: Arc<Mutex<Vec<QbzdDevice>>>,
    daemon: Arc<Mutex<Option<mdns_sd::ServiceDaemon>>>,
}

impl QbzdDiscoveryState {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(Mutex::new(Vec::new())),
            daemon: Arc::new(Mutex::new(None)),
        }
    }
}

#[tauri::command]
pub async fn v2_qbzd_start_discovery(
    state: tauri::State<'_, QbzdDiscoveryState>,
) -> Result<(), String> {
    let mut daemon_guard = state.daemon.lock().await;

    // Stop existing discovery if running
    if let Some(d) = daemon_guard.take() {
        let _ = d.shutdown();
    }

    state.devices.lock().await.clear();

    let mdns = mdns_sd::ServiceDaemon::new()
        .map_err(|e| format!("Failed to create mDNS daemon: {}", e))?;

    let receiver = mdns
        .browse(QBZD_SERVICE_TYPE)
        .map_err(|e| format!("Failed to browse qbzd services: {}", e))?;

    let devices = state.devices.clone();

    // Spawn background task to collect discovered devices
    tokio::spawn(async move {
        while let Ok(event) = receiver.recv_async().await {
            match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    let name = info
                        .get_fullname()
                        .split('.')
                        .next()
                        .unwrap_or("qbzd")
                        .to_string();
                    let host = info
                        .get_addresses()
                        .iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').to_string());
                    let port = info.get_port();
                    let base_url = format!("http://{}:{}", host, port);

                    log::info!("[qbzd-discovery] Found: {} at {}", name, base_url);

                    let device = QbzdDevice {
                        name,
                        host,
                        port,
                        base_url,
                    };

                    let mut devs = devices.lock().await;
                    // Deduplicate by base_url
                    if !devs.iter().any(|d| d.base_url == device.base_url) {
                        devs.push(device);
                    }
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                    log::info!("[qbzd-discovery] Removed: {}", fullname);
                    let name = fullname
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let mut devs = devices.lock().await;
                    devs.retain(|d| d.name != name);
                }
                _ => {}
            }
        }
    });

    *daemon_guard = Some(mdns);
    log::info!("[qbzd-discovery] Discovery started");
    Ok(())
}

#[tauri::command]
pub async fn v2_qbzd_stop_discovery(
    state: tauri::State<'_, QbzdDiscoveryState>,
) -> Result<(), String> {
    let mut daemon_guard = state.daemon.lock().await;
    if let Some(d) = daemon_guard.take() {
        let _ = d.shutdown();
        log::info!("[qbzd-discovery] Discovery stopped");
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_qbzd_get_devices(
    state: tauri::State<'_, QbzdDiscoveryState>,
) -> Result<Vec<QbzdDevice>, String> {
    let devs = state.devices.lock().await;
    Ok(devs.clone())
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub data: DataConfig,
    #[serde(default)]
    pub mpris: MprisConfig,
    #[serde(default)]
    pub qconnect: QConnectConfig,
    #[serde(default)]
    pub mdns: MdnsConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub port: u16,
    /// "auto" generates on first run
    pub token: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".to_string(),
            port: 8182,
            token: "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// pipewire | alsa | pulse | system
    pub backend: String,
    /// Empty = default device
    pub device: String,
    pub gapless: bool,
    pub normalization: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: "pipewire".to_string(),
            device: String::new(),
            gapless: true,
            normalization: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// L1 in-memory cache in MB. 0 = auto-detect from RAM.
    pub memory_mb: usize,
    /// L2 persistent disk cache in MB. 0 = disable.
    pub disk_mb: usize,
    /// Tracks to prefetch ahead
    pub prefetch_count: usize,
    /// Simultaneous track downloads
    pub prefetch_concurrent: usize,
    /// Parallel CMAF segment downloads per track
    pub cmaf_concurrent_segments: usize,
    #[serde(default)]
    pub auto: CacheAutoConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            memory_mb: 0, // 0 = auto-detect
            disk_mb: 400,
            prefetch_count: 2,
            prefetch_concurrent: 1,
            cmaf_concurrent_segments: 2,
            auto: CacheAutoConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheAutoConfig {
    /// Auto-detect RAM and adjust cache sizes
    pub enabled: bool,
}

impl Default for CacheAutoConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    pub dir: String,
}

impl Default for DataConfig {
    fn default() -> Self {
        let default_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/var/lib"))
            .join("qbz")
            .to_string_lossy()
            .to_string();
        Self { dir: default_dir }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MprisConfig {
    pub enabled: bool,
}

impl Default for MprisConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QConnectConfig {
    pub enabled: bool,
    /// Empty = hostname
    pub device_name: String,
}

impl Default for QConnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MdnsConfig {
    pub enabled: bool,
    /// Empty = hostname
    pub name: String,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub journal: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            journal: true,
        }
    }
}

impl DaemonConfig {
    /// Load config from TOML file, falling back to defaults.
    pub fn load(path: Option<&str>) -> Self {
        let config_path = path
            .map(PathBuf::from)
            .or_else(|| {
                dirs::config_dir().map(|d| d.join("qbz").join("qbzd.toml"))
            })
            .unwrap_or_else(|| PathBuf::from("qbzd.toml"));

        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(cfg) => {
                        log::info!("Loaded config from {}", config_path.display());
                        return cfg;
                    }
                    Err(e) => {
                        log::warn!("Failed to parse config {}: {}", config_path.display(), e);
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read config {}: {}", config_path.display(), e);
                }
            }
        } else {
            log::info!(
                "No config file at {}, using defaults",
                config_path.display()
            );
        }

        Self::default()
    }

    /// Resolve the token — generate if "auto".
    pub fn resolve_token(&mut self) {
        if self.token_is_auto() {
            self.server.token = generate_token();
            log::info!("Generated API token (first run)");
        }
    }

    pub fn token_is_auto(&self) -> bool {
        self.server.token == "auto" || self.server.token.is_empty()
    }

    /// Memory cache size in bytes
    pub fn memory_cache_bytes(&self) -> usize {
        self.cache.memory_mb * 1024 * 1024
    }

    /// Disk cache size in bytes
    pub fn disk_cache_bytes(&self) -> usize {
        self.cache.disk_mb * 1024 * 1024
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            audio: AudioConfig::default(),
            cache: CacheConfig::default(),
            data: DataConfig::default(),
            mpris: MprisConfig::default(),
            qconnect: QConnectConfig::default(),
            mdns: MdnsConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

/// Generate a random 32-character hex token
pub fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", seed)
}

/// Save config to the default config path (creates dir if needed).
pub fn save_default_config(config: &DaemonConfig) -> Result<(), String> {
    let config_dir = dirs::config_dir()
        .ok_or("Could not determine config directory")?
        .join("qbz");
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;

    let path = config_dir.join("qbzd.toml");
    let content = toml::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    log::info!("[qbzd] Config saved to {}", path.display());
    Ok(())
}

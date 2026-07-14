// crates/qbzd/src/config.rs — qbzd.toml: PROCESS concerns only (D14 single-source rule).
// Engine settings (audio/playback/qconnect content) live in the stores — never here.
// QConnect startup_mode/device_name/volume_mode live SOLELY in the daemon-root
// qconnect_settings.db KV (03 §3.4/§6; helpers land in T9) — no [qconnect] table.
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)] // Serialize: `qbzd config show --json` (T11)
#[serde(default)]
pub struct QbzdConfig {
    pub config_version: u32,
    pub data_root: Option<String>, // container override; cache root derived
    pub server: ServerCfg,
    pub log: LogCfg,
    pub mpris: MprisCfg, // documented now, inert in P0 (01 §11)
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerCfg {
    pub bind: String,
    pub port: u16,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct LogCfg {
    pub level: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct MprisCfg {
    pub enabled: bool,
}

impl Default for ServerCfg {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".into(),
            port: 8182,
        }
    }
}
impl Default for LogCfg {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}
impl Default for MprisCfg {
    fn default() -> Self {
        Self { enabled: true }
    }
}
impl Default for QbzdConfig {
    fn default() -> Self {
        Self {
            config_version: 1,
            data_root: None,
            server: Default::default(),
            log: Default::default(),
            mpris: Default::default(),
        }
    }
}

/// Known keys, one entry per (table, key). Kept literal so the sweep and the
/// spec table diff cleanly. Released keys are never renamed without an alias.
const KNOWN: &[(&str, &str)] = &[
    ("", "config_version"),
    ("", "data_root"),
    ("server", "bind"),
    ("server", "port"),
    ("log", "level"),
    ("mpris", "enabled"),
];

impl QbzdConfig {
    pub fn from_str(text: &str) -> Result<(Self, Vec<String>), String> {
        let value: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut warns = Vec::new();
        sweep(&value, "", &mut warns);
        let cfg: QbzdConfig = value.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        Ok((cfg, warns))
    }
    pub fn load(path: &std::path::Path) -> Result<(Self, Vec<String>), String> {
        match std::fs::read_to_string(path) {
            Ok(t) => Self::from_str(&t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok((Self::default(), Vec::new()))
            }
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }
}

fn sweep(v: &toml::Value, table: &str, warns: &mut Vec<String>) {
    if let toml::Value::Table(map) = v {
        for (k, inner) in map {
            match inner {
                toml::Value::Table(_) if table.is_empty() => sweep(inner, k, warns),
                _ if !KNOWN.contains(&(table, k.as_str())) => {
                    warns.push(if table.is_empty() {
                        k.clone()
                    } else {
                        format!("[{table}].{k}")
                    });
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_match_spec() {
        // 01-architecture.md §10.1
        let (c, warns) = QbzdConfig::from_str("").unwrap();
        assert_eq!(c.config_version, 1);
        assert_eq!(c.server.bind, "127.0.0.1");
        assert_eq!(c.server.port, 8182);
        assert_eq!(c.log.level, "info");
        assert!(c.mpris.enabled);
        assert!(warns.is_empty());
    }
    #[test]
    fn unknown_keys_warn_never_error() {
        // D14 / operator §5.4 (J5 silent-revert guard)
        let (_c, warns) = QbzdConfig::from_str("[server]\nbindd = \"0.0.0.0\"\n").unwrap();
        assert_eq!(warns, vec!["[server].bindd".to_string()]);
    }
}

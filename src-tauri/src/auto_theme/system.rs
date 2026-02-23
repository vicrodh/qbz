//! Desktop environment detection, wallpaper path retrieval, and system accent color.

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::PaletteColor;

/// Supported desktop environments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DesktopEnvironment {
    Gnome,
    KdePlasma,
    Cosmic,
    Xfce,
    Cinnamon,
    Unknown(String),
}

impl DesktopEnvironment {
    /// Human-readable name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Gnome => "GNOME",
            Self::KdePlasma => "KDE Plasma",
            Self::Cosmic => "COSMIC",
            Self::Xfce => "Xfce",
            Self::Cinnamon => "Cinnamon",
            Self::Unknown(name) => name.as_str(),
        }
    }
}

/// Detect the current desktop environment.
pub fn detect_desktop_environment() -> DesktopEnvironment {
    // Try each env var in priority order
    let candidates = [
        env::var("XDG_CURRENT_DESKTOP"),
        env::var("XDG_SESSION_DESKTOP"),
        env::var("DESKTOP_SESSION"),
    ];

    for candidate in &candidates {
        if let Ok(val) = candidate {
            let upper = val.to_uppercase();
            if upper.contains("GNOME") || upper.contains("UNITY") || upper.contains("UBUNTU") {
                return DesktopEnvironment::Gnome;
            }
            if upper.contains("KDE") || upper.contains("PLASMA") {
                return DesktopEnvironment::KdePlasma;
            }
            if upper.contains("COSMIC") {
                return DesktopEnvironment::Cosmic;
            }
            if upper.contains("XFCE") {
                return DesktopEnvironment::Xfce;
            }
            if upper.contains("CINNAMON") || upper.contains("X-CINNAMON") {
                return DesktopEnvironment::Cinnamon;
            }
        }
    }

    let name = candidates
        .iter()
        .find_map(|c| c.as_ref().ok().cloned())
        .unwrap_or_else(|| "unknown".to_string());

    DesktopEnvironment::Unknown(name)
}

/// Get the current wallpaper path for the detected DE.
pub fn get_system_wallpaper() -> Result<String, String> {
    let de = detect_desktop_environment();
    get_wallpaper_for_de(&de)
}

fn get_wallpaper_for_de(de: &DesktopEnvironment) -> Result<String, String> {
    match de {
        DesktopEnvironment::Gnome => get_gnome_wallpaper(),
        DesktopEnvironment::KdePlasma => get_kde_wallpaper(),
        DesktopEnvironment::Cosmic => get_cosmic_wallpaper(),
        DesktopEnvironment::Cinnamon => get_cinnamon_wallpaper(),
        DesktopEnvironment::Xfce => get_xfce_wallpaper(),
        DesktopEnvironment::Unknown(name) => {
            Err(format!("Unsupported desktop environment: {}", name))
        }
    }
}

/// Get the system accent color for the detected DE.
pub fn get_system_accent_color() -> Result<PaletteColor, String> {
    let de = detect_desktop_environment();
    get_accent_for_de(&de)
}

fn get_accent_for_de(de: &DesktopEnvironment) -> Result<PaletteColor, String> {
    match de {
        DesktopEnvironment::Gnome => get_gnome_accent(),
        DesktopEnvironment::KdePlasma => get_kde_accent(),
        DesktopEnvironment::Cosmic => get_cosmic_accent(),
        _ => Err(format!(
            "Accent color not supported for {}",
            de.display_name()
        )),
    }
}

// --- GNOME ---

fn get_gnome_wallpaper() -> Result<String, String> {
    // Try dark variant first, then standard
    for key in &["picture-uri-dark", "picture-uri"] {
        let output = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.background", key])
            .output()
            .map_err(|e| format!("Failed to run gsettings: {}", e))?;

        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(path) = parse_gsettings_uri(&raw) {
                if PathBuf::from(&path).exists() {
                    return Ok(path);
                }
            }
        }
    }
    Err("Could not determine GNOME wallpaper".into())
}

fn get_gnome_accent() -> Result<PaletteColor, String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "accent-color"])
        .output()
        .map_err(|e| format!("Failed to run gsettings: {}", e))?;

    if !output.status.success() {
        return Err("gsettings accent-color not available (requires GNOME 47+)".into());
    }

    let raw = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('\'')
        .to_lowercase();

    // GNOME accent color names → approximate RGB values
    let color = match raw.as_str() {
        "blue" => PaletteColor::new(53, 132, 228),
        "teal" => PaletteColor::new(38, 162, 105),
        "green" => PaletteColor::new(51, 209, 122),
        "yellow" => PaletteColor::new(246, 211, 45),
        "orange" => PaletteColor::new(255, 120, 0),
        "red" => PaletteColor::new(224, 27, 36),
        "pink" => PaletteColor::new(220, 138, 221),
        "purple" => PaletteColor::new(145, 65, 172),
        "slate" => PaletteColor::new(111, 131, 150),
        _ => return Err(format!("Unknown GNOME accent color: {}", raw)),
    };

    Ok(color)
}

// --- KDE Plasma ---

fn get_kde_wallpaper() -> Result<String, String> {
    let home = env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    let config_path = format!(
        "{}/.config/plasma-org.kde.plasma.desktop-appletsrc",
        home
    );

    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("Cannot read Plasma config: {}", e))?;

    // Look for Image= under [Wallpaper][org.kde.image][General]
    let mut in_wallpaper_section = false;
    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            // Check if we're entering a wallpaper image section
            in_wallpaper_section = trimmed.contains("Wallpaper")
                && trimmed.contains("org.kde.image")
                && trimmed.contains("General");
        }

        if in_wallpaper_section && trimmed.starts_with("Image=") {
            let value = trimmed.trim_start_matches("Image=").trim();
            if let Some(path) = parse_file_uri(value) {
                if PathBuf::from(&path).exists() {
                    return Ok(path);
                }
            }
            // Try as plain path
            if PathBuf::from(value).exists() {
                return Ok(value.to_string());
            }
        }
    }

    Err("Could not find wallpaper in Plasma config".into())
}

fn get_kde_accent() -> Result<PaletteColor, String> {
    let home = env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    let config_path = format!("{}/.config/kdeglobals", home);

    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("Cannot read kdeglobals: {}", e))?;

    let mut in_general = false;
    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[General]" {
            in_general = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_general = false;
            continue;
        }

        if in_general && trimmed.starts_with("AccentColor=") {
            let value = trimmed.trim_start_matches("AccentColor=").trim();
            return parse_rgb_csv(value);
        }
    }

    Err("AccentColor not found in kdeglobals".into())
}

// --- COSMIC ---

fn get_cosmic_wallpaper() -> Result<String, String> {
    let home = env::var("HOME").map_err(|_| "HOME not set".to_string())?;

    // COSMIC stores wallpaper config in its own config system
    let config_paths = [
        format!(
            "{}/.config/cosmic/com.system76.CosmicBackground/v1/all",
            home
        ),
        format!(
            "{}/.config/cosmic/com.system76.CosmicBackground/v1/backgrounds",
            home
        ),
    ];

    for config_path in &config_paths {
        if let Ok(content) = fs::read_to_string(config_path) {
            // COSMIC config may contain paths in RON format or plain text
            // Look for file paths
            if let Some(path) = extract_path_from_cosmic_config(&content) {
                if PathBuf::from(&path).exists() {
                    return Ok(path);
                }
            }
        }
    }

    Err("Could not find wallpaper in COSMIC config".into())
}

fn get_cosmic_accent() -> Result<PaletteColor, String> {
    let home = env::var("HOME").map_err(|_| "HOME not set".to_string())?;

    // Try dark and light theme accent
    let accent_paths = [
        format!(
            "{}/.config/cosmic/com.system76.CosmicTheme.Dark/v1/accent",
            home
        ),
        format!(
            "{}/.config/cosmic/com.system76.CosmicTheme.Light/v1/accent",
            home
        ),
    ];

    for path in &accent_paths {
        if let Ok(content) = fs::read_to_string(path) {
            if let Some(color) = parse_cosmic_color(&content) {
                return Ok(color);
            }
        }
    }

    Err("Could not read COSMIC accent color".into())
}

// --- Cinnamon ---

fn get_cinnamon_wallpaper() -> Result<String, String> {
    let output = Command::new("gsettings")
        .args(["get", "org.cinnamon.desktop.background", "picture-uri"])
        .output()
        .map_err(|e| format!("Failed to run gsettings: {}", e))?;

    if !output.status.success() {
        return Err("Could not get Cinnamon wallpaper via gsettings".into());
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if let Some(path) = parse_gsettings_uri(&raw) {
        if PathBuf::from(&path).exists() {
            return Ok(path);
        }
    }

    Err("Could not determine Cinnamon wallpaper".into())
}

// --- XFCE ---

fn get_xfce_wallpaper() -> Result<String, String> {
    let output = Command::new("xfconf-query")
        .args([
            "-c",
            "xfce4-desktop",
            "-p",
            "/backdrop/screen0/monitoreDP-1/workspace0/last-image",
        ])
        .output()
        .map_err(|e| format!("Failed to run xfconf-query: {}", e))?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if PathBuf::from(&path).exists() {
            return Ok(path);
        }
    }

    // Fallback: try generic monitor path
    let output = Command::new("xfconf-query")
        .args([
            "-c",
            "xfce4-desktop",
            "-l",
            "-v",
        ])
        .output()
        .map_err(|e| format!("Failed to list xfce4-desktop properties: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("last-image") {
                // Format: /backdrop/.../last-image    /path/to/image
                let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
                if parts.len() == 2 {
                    let path = parts[1].trim();
                    if PathBuf::from(path).exists() {
                        return Ok(path.to_string());
                    }
                }
            }
        }
    }

    Err("Could not determine XFCE wallpaper".into())
}

// --- Parsing helpers ---

/// Parse gsettings output like `'file:///path/to/wallpaper.jpg'` into a filesystem path.
fn parse_gsettings_uri(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('\'').trim_matches('"');
    parse_file_uri(trimmed).or_else(|| {
        // Some gsettings return plain paths
        if PathBuf::from(trimmed).is_absolute() {
            Some(trimmed.to_string())
        } else {
            None
        }
    })
}

/// Extract filesystem path from `file:///path` URI.
fn parse_file_uri(uri: &str) -> Option<String> {
    if let Some(path) = uri.strip_prefix("file://") {
        // URL-decode basic cases (spaces as %20)
        let decoded = path.replace("%20", " ");
        Some(decoded)
    } else {
        None
    }
}

/// Parse "r,g,b" CSV format (KDE).
fn parse_rgb_csv(value: &str) -> Result<PaletteColor, String> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() < 3 {
        return Err(format!("Invalid RGB CSV: {}", value));
    }
    let r = parts[0]
        .trim()
        .parse::<u8>()
        .map_err(|_| format!("Invalid R value: {}", parts[0]))?;
    let g = parts[1]
        .trim()
        .parse::<u8>()
        .map_err(|_| format!("Invalid G value: {}", parts[1]))?;
    let b = parts[2]
        .trim()
        .parse::<u8>()
        .map_err(|_| format!("Invalid B value: {}", parts[2]))?;
    Ok(PaletteColor::new(r, g, b))
}

/// Best-effort extraction of image path from COSMIC config content.
fn extract_path_from_cosmic_config(content: &str) -> Option<String> {
    // COSMIC config may have paths in various formats
    // Look for common image file patterns
    for line in content.lines() {
        let trimmed = line.trim().trim_matches('"').trim_matches('\'');

        // Check for file:// URI
        if let Some(path) = parse_file_uri(trimmed) {
            if is_image_path(&path) {
                return Some(path);
            }
        }

        // Check for absolute path to an image
        if trimmed.starts_with('/') && is_image_path(trimmed) {
            return Some(trimmed.to_string());
        }

        // Look for path embedded in the line (e.g., in RON format)
        if let Some(start) = trimmed.find('/') {
            let potential = &trimmed[start..];
            // Find end of path (before closing quote/paren)
            let end = potential
                .find(|c: char| c == '"' || c == '\'' || c == ')' || c == ',')
                .unwrap_or(potential.len());
            let path = &potential[..end];
            if is_image_path(path) && PathBuf::from(path).is_absolute() {
                return Some(path.to_string());
            }
        }
    }
    None
}

/// Parse COSMIC color format (may be RON-like with RGBA floats).
fn parse_cosmic_color(content: &str) -> Option<PaletteColor> {
    // COSMIC accent is typically stored as float RGBA (0.0-1.0)
    // Try to extract numeric values
    let nums: Vec<f64> = content
        .split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    if nums.len() >= 3 {
        // Check if values are in 0-1 range (float) or 0-255 range (int)
        let (r, g, b) = if nums[0] <= 1.0 && nums[1] <= 1.0 && nums[2] <= 1.0 {
            (
                (nums[0] * 255.0).round() as u8,
                (nums[1] * 255.0).round() as u8,
                (nums[2] * 255.0).round() as u8,
            )
        } else {
            (nums[0] as u8, nums[1] as u8, nums[2] as u8)
        };
        Some(PaletteColor::new(r, g, b))
    } else {
        None
    }
}

fn is_image_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".tiff")
        || lower.ends_with(".tif")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_de() {
        // Just verify it doesn't panic
        let _de = detect_desktop_environment();
    }

    #[test]
    fn test_parse_gsettings_uri() {
        assert_eq!(
            parse_gsettings_uri("'file:///home/user/wallpaper.jpg'"),
            Some("/home/user/wallpaper.jpg".to_string())
        );
        assert_eq!(
            parse_gsettings_uri("'file:///home/user/my%20wallpaper.png'"),
            Some("/home/user/my wallpaper.png".to_string())
        );
    }

    #[test]
    fn test_parse_file_uri() {
        assert_eq!(
            parse_file_uri("file:///home/user/pic.jpg"),
            Some("/home/user/pic.jpg".to_string())
        );
        assert_eq!(parse_file_uri("/just/a/path"), None);
    }

    #[test]
    fn test_parse_rgb_csv() {
        let color = parse_rgb_csv("66,133,244").unwrap();
        assert_eq!(color, PaletteColor::new(66, 133, 244));
    }

    #[test]
    fn test_parse_rgb_csv_with_spaces() {
        let color = parse_rgb_csv(" 66 , 133 , 244 ").unwrap();
        assert_eq!(color, PaletteColor::new(66, 133, 244));
    }

    #[test]
    fn test_parse_cosmic_color_float() {
        let color = parse_cosmic_color("(0.26, 0.52, 0.96, 1.0)").unwrap();
        assert_eq!(color.r, 66);
        assert_eq!(color.g, 133);
        assert_eq!(color.b, 245);
    }

    #[test]
    fn test_is_image_path() {
        assert!(is_image_path("/home/user/wall.jpg"));
        assert!(is_image_path("/home/user/wall.PNG"));
        assert!(is_image_path("/home/user/wall.webp"));
        assert!(!is_image_path("/home/user/wall.mp4"));
        assert!(!is_image_path("/home/user/wall.txt"));
    }
}

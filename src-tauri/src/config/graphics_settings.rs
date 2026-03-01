//! Graphics settings persistence
//!
//! Stores GPU/rendering preferences that take effect before WebView initialization.
//! These settings are device-level (not per-user) and persist across sessions.
//!
//! - hardware_acceleration: GPU rendering toggle (default: on). Read at startup
//!   as the default value; env var QBZ_HARDWARE_ACCEL=0|1 overrides.
//! - force_x11: force X11/XWayland backend on Wayland sessions (default: off)
//!   Env var QBZ_FORCE_X11=1|0 always overrides the stored value.
//! - gdk_scale / gdk_dpi_scale: display scaling overrides for XWayland

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// --- Global startup state (set once in main.rs, read-only thereafter) ---
static GRAPHICS_USING_FALLBACK: AtomicBool = AtomicBool::new(false);
static GRAPHICS_IS_WAYLAND: AtomicBool = AtomicBool::new(false);
static GRAPHICS_HAS_NVIDIA: AtomicBool = AtomicBool::new(false);
static GRAPHICS_HAS_AMD: AtomicBool = AtomicBool::new(false);
static GRAPHICS_HAS_INTEL: AtomicBool = AtomicBool::new(false);
static GRAPHICS_IS_VM: AtomicBool = AtomicBool::new(false);
static GRAPHICS_HW_ACCEL: AtomicBool = AtomicBool::new(true);
static GRAPHICS_FORCE_X11: AtomicBool = AtomicBool::new(false);

/// Set startup graphics state (called once from main.rs)
pub fn set_startup_graphics_state(
    using_fallback: bool,
    is_wayland: bool,
    has_nvidia: bool,
    has_amd: bool,
    has_intel: bool,
    is_vm: bool,
    hw_accel: bool,
    force_x11: bool,
) {
    GRAPHICS_USING_FALLBACK.store(using_fallback, Ordering::SeqCst);
    GRAPHICS_IS_WAYLAND.store(is_wayland, Ordering::SeqCst);
    GRAPHICS_HAS_NVIDIA.store(has_nvidia, Ordering::SeqCst);
    GRAPHICS_HAS_AMD.store(has_amd, Ordering::SeqCst);
    GRAPHICS_HAS_INTEL.store(has_intel, Ordering::SeqCst);
    GRAPHICS_IS_VM.store(is_vm, Ordering::SeqCst);
    GRAPHICS_HW_ACCEL.store(hw_accel, Ordering::SeqCst);
    GRAPHICS_FORCE_X11.store(force_x11, Ordering::SeqCst);
}

/// Check if graphics is using fallback defaults (for UI warning)
pub fn is_using_graphics_fallback() -> bool {
    GRAPHICS_USING_FALLBACK.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsSettings {
    /// GPU rendering toggle. Read at startup as default; env var QBZ_HARDWARE_ACCEL overrides.
    pub hardware_acceleration: bool,
    /// Force X11 (XWayland) backend on Wayland sessions (requires restart)
    pub force_x11: bool,
    /// GDK_SCALE override for XWayland (None = auto). Integer values: "1", "2"
    pub gdk_scale: Option<String>,
    /// GDK_DPI_SCALE override for XWayland (None = auto). Float values: "0.5", "1", "1.5"
    pub gdk_dpi_scale: Option<String>,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            hardware_acceleration: true,
            force_x11: false,
            gdk_scale: None,
            gdk_dpi_scale: None,
        }
    }
}

pub struct GraphicsSettingsStore {
    conn: Connection,
}

impl GraphicsSettingsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("graphics_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open graphics settings database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for graphics settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS graphics_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                hardware_acceleration INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO graphics_settings (id, hardware_acceleration) VALUES (1, 0);",
        )
        .map_err(|e| format!("Failed to create graphics settings table: {}", e))?;

        // Migrations: add columns (no-op if already present)
        let _ = conn.execute_batch(
            "ALTER TABLE graphics_settings ADD COLUMN force_x11 INTEGER NOT NULL DEFAULT 0;",
        );
        let _ = conn.execute_batch("ALTER TABLE graphics_settings ADD COLUMN gdk_scale TEXT;");
        let _ = conn.execute_batch("ALTER TABLE graphics_settings ADD COLUMN gdk_dpi_scale TEXT;");

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<GraphicsSettings, String> {
        self.conn
            .query_row(
                "SELECT hardware_acceleration, force_x11, gdk_scale, gdk_dpi_scale FROM graphics_settings WHERE id = 1",
                [],
                |row| {
                    Ok(GraphicsSettings {
                        hardware_acceleration: row.get::<_, i64>(0)? != 0,
                        force_x11: row.get::<_, i64>(1)? != 0,
                        gdk_scale: row.get::<_, Option<String>>(2)?,
                        gdk_dpi_scale: row.get::<_, Option<String>>(3)?,
                    })
                },
            )
            .map_err(|e| format!("Failed to get graphics settings: {}", e))
    }

    pub fn set_hardware_acceleration(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE graphics_settings SET hardware_acceleration = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set hardware_acceleration: {}", e))?;
        Ok(())
    }

    pub fn set_force_x11(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE graphics_settings SET force_x11 = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set force_x11: {}", e))?;
        Ok(())
    }

    pub fn set_gdk_scale(&self, value: Option<String>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE graphics_settings SET gdk_scale = ?1 WHERE id = 1",
                params![value],
            )
            .map_err(|e| format!("Failed to set gdk_scale: {}", e))?;
        Ok(())
    }

    pub fn set_gdk_dpi_scale(&self, value: Option<String>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE graphics_settings SET gdk_dpi_scale = ?1 WHERE id = 1",
                params![value],
            )
            .map_err(|e| format!("Failed to set gdk_dpi_scale: {}", e))?;
        Ok(())
    }
}

/// Thread-safe wrapper for Tauri state management
pub struct GraphicsSettingsState {
    pub store: Arc<Mutex<Option<GraphicsSettingsStore>>>,
}

impl GraphicsSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(GraphicsSettingsStore::new()?))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }
}

// Tauri commands

#[tauri::command]
pub fn get_graphics_settings(
    state: tauri::State<'_, GraphicsSettingsState>,
) -> Result<GraphicsSettings, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn set_hardware_acceleration(
    state: tauri::State<'_, GraphicsSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[GraphicsSettings] Setting hardware_acceleration to {} (restart required)",
        enabled
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_hardware_acceleration(enabled)
}

#[tauri::command]
pub fn set_force_x11(
    state: tauri::State<'_, GraphicsSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[GraphicsSettings] Setting force_x11 to {} (restart required)",
        enabled
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_force_x11(enabled)
}

#[tauri::command]
pub fn set_gdk_scale(
    state: tauri::State<'_, GraphicsSettingsState>,
    value: Option<String>,
) -> Result<(), String> {
    log::info!(
        "[GraphicsSettings] Setting gdk_scale to {:?} (restart required)",
        value
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gdk_scale(value)
}

#[tauri::command]
pub fn set_gdk_dpi_scale(
    state: tauri::State<'_, GraphicsSettingsState>,
    value: Option<String>,
) -> Result<(), String> {
    log::info!(
        "[GraphicsSettings] Setting gdk_dpi_scale to {:?} (restart required)",
        value
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gdk_dpi_scale(value)
}

// --- Startup Status (set once at app launch, read-only thereafter) ---

/// Tracks graphics configuration status from startup
/// Used to show warnings in UI when settings failed to load
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsStartupStatus {
    /// True if graphics settings failed to load from DB (using safe defaults)
    pub using_fallback: bool,
    /// True if running on Wayland
    pub is_wayland: bool,
    /// True if NVIDIA GPU was detected
    pub has_nvidia: bool,
    /// True if AMD GPU was detected
    pub has_amd: bool,
    /// True if Intel GPU was detected
    pub has_intel: bool,
    /// True if running in a virtual machine
    pub is_vm: bool,
    /// True if hardware acceleration is enabled
    pub hardware_accel_enabled: bool,
    /// True if force_x11 is active
    pub force_x11_active: bool,
}

/// Get graphics startup status (reads from static atomics set in main.rs)
#[tauri::command]
pub fn get_graphics_startup_status() -> GraphicsStartupStatus {
    GraphicsStartupStatus {
        using_fallback: GRAPHICS_USING_FALLBACK.load(Ordering::SeqCst),
        is_wayland: GRAPHICS_IS_WAYLAND.load(Ordering::SeqCst),
        has_nvidia: GRAPHICS_HAS_NVIDIA.load(Ordering::SeqCst),
        has_amd: GRAPHICS_HAS_AMD.load(Ordering::SeqCst),
        has_intel: GRAPHICS_HAS_INTEL.load(Ordering::SeqCst),
        is_vm: GRAPHICS_IS_VM.load(Ordering::SeqCst),
        hardware_accel_enabled: GRAPHICS_HW_ACCEL.load(Ordering::SeqCst),
        force_x11_active: GRAPHICS_FORCE_X11.load(Ordering::SeqCst),
    }
}

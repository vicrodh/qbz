//! Audio diagnostics commands for detecting actual hardware sample rate

use std::fs;

/// Hardware audio status
#[derive(Debug, Clone, serde::Serialize)]
pub struct HardwareAudioStatus {
    /// Actual sample rate being used by hardware (Hz)
    pub hardware_sample_rate: Option<u32>,
    /// Audio format (e.g., "S32_LE")
    pub hardware_format: Option<String>,
    /// Whether hardware is currently active
    pub is_active: bool,
}

/// Get actual hardware audio status by reading /proc/asound
#[tauri::command]
pub fn get_hardware_audio_status() -> Result<HardwareAudioStatus, String> {
    // Read all hw_params files from /proc/asound
    let proc_pattern = "/proc/asound/card*/pcm*/sub*/hw_params";
    
    // Use glob to find all matching files
    let paths = glob::glob(proc_pattern)
        .map_err(|e| format!("Failed to glob hw_params: {}", e))?;
    
    for path in paths.flatten() {
        if let Ok(contents) = fs::read_to_string(&path) {
            // Parse the hw_params file
            let mut sample_rate: Option<u32> = None;
            let mut format: Option<String> = None;
            
            for line in contents.lines() {
                let line = line.trim();
                
                // Parse rate line: "rate: 192000 (192000/1)"
                if line.starts_with("rate:") {
                    if let Some(rate_str) = line.split_whitespace().nth(1) {
                        sample_rate = rate_str.parse().ok();
                    }
                }
                
                // Parse format line: "format: S32_LE"
                if line.starts_with("format:") {
                    if let Some(fmt) = line.split_whitespace().nth(1) {
                        format = Some(fmt.to_string());
                    }
                }
            }
            
            // If we found both, return it
            if sample_rate.is_some() {
                log::debug!(
                    "Hardware audio status: {}Hz, format: {:?}",
                    sample_rate.unwrap_or(0),
                    format
                );

                return Ok(HardwareAudioStatus {
                    hardware_sample_rate: sample_rate,
                    hardware_format: format,
                    is_active: true,
                });
            }
        }
    }
    
    // No active hardware found
    Ok(HardwareAudioStatus {
        hardware_sample_rate: None,
        hardware_format: None,
        is_active: false,
    })
}

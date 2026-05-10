use anyhow::{anyhow, Result};
use std::process::Command;

/// Information about a monitor/screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorInfo {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl MonitorInfo {
    /// Create a new MonitorInfo.
    pub fn new(left: i32, top: i32, width: i32, height: i32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    /// Check if a point (x, y) is within this monitor.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left
            && x < self.left + self.width
            && y >= self.top
            && y < self.top + self.height
    }
}

/// Capture a screenshot of the monitor that contains the current mouse cursor.
///
/// Returns the raw PNG bytes and the monitor info.
///
/// # Platform-specific behavior
/// - **macOS**: Uses `screencapture` command-line tool.
/// - **Windows**: Uses GDI API (not yet implemented).
/// - **Linux**: Uses X11 (not yet implemented).
///
/// # Errors
/// Returns an error if the platform is unsupported or the capture command fails.
pub fn screenshot_current_monitor() -> Result<(Vec<u8>, MonitorInfo)> {
    #[cfg(target_os = "macos")]
    {
        screenshot_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow!(
            "screenshot_current_monitor is only implemented for macOS in this phase"
        ))
    }
}

/// Encode raw image bytes to a base64 string.
///
/// # Arguments
/// * `bytes` - Raw image bytes (e.g., PNG).
///
/// # Returns
/// Base64-encoded string.
pub fn encode_image_to_base64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(bytes)
}

#[cfg(target_os = "macos")]
fn screenshot_macos() -> Result<(Vec<u8>, MonitorInfo)> {
    // Get current mouse position using AppleScript
    let mouse_pos = get_mouse_position_macos()?;

    // Get monitor info using system_profiler
    let monitor = get_monitor_at_position_macos(mouse_pos.0, mouse_pos.1)?;

    // Use screencapture to capture the specific monitor
    let output = Command::new("screencapture")
        .args([
            "-x", // no sound
            "-D",
            &get_display_index_macos(mouse_pos.0, mouse_pos.1)?.to_string(),
            "-",
        ]) // output to stdout
        .output()
        .map_err(|e| anyhow!("Failed to run screencapture: {}", e))?;

    if !output.status.success() {
        return Err(anyhow!(
            "screencapture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok((output.stdout, monitor))
}

#[cfg(target_os = "macos")]
fn get_mouse_position_macos() -> Result<(i32, i32)> {
    let script = r#"
        tell application "System Events"
            return {mouse location}
        end tell
    "#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| anyhow!("Failed to get mouse position: {}", e))?;

    if !output.status.success() {
        return Err(anyhow!(
            "AppleScript failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse output like "{123, 456}"
    let trimmed = stdout.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<&str> = trimmed.split(',').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Unexpected mouse position format: {}", stdout));
    }

    let x: i32 = parts[0]
        .trim()
        .parse()
        .map_err(|e| anyhow!("Failed to parse mouse x: {}", e))?;
    let y: i32 = parts[1]
        .trim()
        .parse()
        .map_err(|e| anyhow!("Failed to parse mouse y: {}", e))?;

    Ok((x, y))
}

#[cfg(target_os = "macos")]
fn get_monitor_at_position_macos(_x: i32, _y: i32) -> Result<MonitorInfo> {
    // For now, return a default monitor info
    // In a full implementation, this would query actual display info
    // This is a simplified version for Phase 1
    let script = r#"
        tell application "Finder"
            return bounds of window of desktop
        end tell
    "#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| anyhow!("Failed to get monitor bounds: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim().trim_start_matches('{').trim_end_matches('}');
        let parts: Vec<&str> = trimmed.split(',').collect();
        if parts.len() == 4 {
            if let (Ok(_), Ok(_), Ok(w), Ok(h)) = (
                parts[0].trim().parse::<i32>(),
                parts[1].trim().parse::<i32>(),
                parts[2].trim().parse::<i32>(),
                parts[3].trim().parse::<i32>(),
            ) {
                return Ok(MonitorInfo::new(0, 0, w, h));
            }
        }
    }

    // Fallback to common resolution
    Ok(MonitorInfo::new(0, 0, 1920, 1080))
}

#[cfg(target_os = "macos")]
fn get_display_index_macos(_x: i32, _y: i32) -> Result<i32> {
    // Simplified: always return 1 (primary display)
    // Full implementation would query CGGetDisplaysWithPoint
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_info_contains() {
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        assert!(monitor.contains(100, 100));
        assert!(monitor.contains(1919, 1079));
        assert!(!monitor.contains(1920, 1080));
        assert!(!monitor.contains(-1, -1));
    }

    #[test]
    fn test_encode_image_to_base64() {
        let data = b"fake_png_data";
        let encoded = encode_image_to_base64(data);
        assert!(!encoded.is_empty());
        // Verify it's valid base64
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let decoded = STANDARD.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}

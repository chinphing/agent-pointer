use crate::tools::computer::screen::MonitorInfo;

/// Supported coordinate systems for model output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoordinateSystem {
    /// Qwen coordinate system: 0-1000 normalized.
    #[default]
    Qwen,
    /// Kimi coordinate system: 0-1000 normalized (may have different mapping).
    Kimi,
    /// Direct pixel coordinates.
    Pixel,
}

impl CoordinateSystem {
    /// Parse a coordinate system from a string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "qwen" => Some(Self::Qwen),
            "kimi" => Some(Self::Kimi),
            "pixel" => Some(Self::Pixel),
            _ => None,
        }
    }
}

/// Convert normalized coordinates to screen pixel coordinates.
///
/// # Arguments
/// * `normalized` - Normalized coordinates (x, y) in the range [0, 1000] for Qwen/Kimi.
/// * `monitor` - The monitor info defining the screen bounds.
/// * `system` - The coordinate system to use.
///
/// # Returns
/// Screen pixel coordinates (x, y).
///
/// # Notes
/// - For Qwen/Kimi: maps 0-1000 to monitor width/height.
/// - For Pixel: returns the input directly.
/// - Result is offset by monitor's left/top position.
pub fn normalized_to_screen(
    normalized: (f32, f32),
    monitor: &MonitorInfo,
    system: CoordinateSystem,
) -> (i32, i32) {
    match system {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let x = (normalized.0 / 1000.0) * monitor.width as f32;
            let y = (normalized.1 / 1000.0) * monitor.height as f32;
            (
                monitor.left + x as i32,
                monitor.top + y as i32,
            )
        }
        CoordinateSystem::Pixel => (normalized.0 as i32, normalized.1 as i32),
    }
}

/// Convert screen pixel coordinates to normalized coordinates.
///
/// # Arguments
/// * `screen` - Screen pixel coordinates (x, y).
/// * `monitor` - The monitor info defining the screen bounds.
/// * `system` - The coordinate system to use.
///
/// # Returns
/// Normalized coordinates (x, y) in the range [0, 1000] for Qwen/Kimi.
pub fn screen_to_normalized(
    screen: (i32, i32),
    monitor: &MonitorInfo,
    system: CoordinateSystem,
) -> (f32, f32) {
    match system {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let x = ((screen.0 - monitor.left) as f32 / monitor.width as f32) * 1000.0;
            let y = ((screen.1 - monitor.top) as f32 / monitor.height as f32) * 1000.0;
            (x, y)
        }
        CoordinateSystem::Pixel => (screen.0 as f32, screen.1 as f32),
    }
}

/// A converter that holds monitor info and coordinate system for repeated conversions.
#[derive(Debug, Clone)]
pub struct CoordConverter {
    monitor: MonitorInfo,
    system: CoordinateSystem,
}

impl CoordConverter {
    /// Create a new CoordConverter.
    pub fn new(monitor: MonitorInfo, system: CoordinateSystem) -> Self {
        Self { monitor, system }
    }

    /// Convert normalized to screen coordinates.
    pub fn to_screen(&self, normalized: (f32, f32)) -> (i32, i32) {
        normalized_to_screen(normalized, &self.monitor, self.system)
    }

    /// Convert screen to normalized coordinates.
    pub fn to_normalized(&self, screen: (i32, i32)) -> (f32, f32) {
        screen_to_normalized(screen, &self.monitor, self.system)
    }

    /// Get the monitor info.
    pub fn monitor(&self) -> &MonitorInfo {
        &self.monitor
    }

    /// Get the coordinate system.
    pub fn system(&self) -> CoordinateSystem {
        self.system
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_conversion() {
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        let (x, y) = normalized_to_screen((500.0, 500.0), &monitor, CoordinateSystem::Qwen);
        assert_eq!(x, 960);
        assert_eq!(y, 540);
    }

    #[test]
    fn test_qwen_with_offset() {
        let monitor = MonitorInfo::new(100, 100, 1920, 1080);
        let (x, y) = normalized_to_screen((500.0, 500.0), &monitor, CoordinateSystem::Qwen);
        assert_eq!(x, 1060);
        assert_eq!(y, 640);
    }

    #[test]
    fn test_pixel_conversion() {
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        let (x, y) = normalized_to_screen((100.0, 200.0), &monitor, CoordinateSystem::Pixel);
        assert_eq!(x, 100);
        assert_eq!(y, 200);
    }

    #[test]
    fn test_screen_to_normalized() {
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        let (x, y) = screen_to_normalized((960, 540), &monitor, CoordinateSystem::Qwen);
        assert!((x - 500.0).abs() < 0.1);
        assert!((y - 500.0).abs() < 0.1);
    }

    #[test]
    fn test_converter() {
        let monitor = MonitorInfo::new(0, 0, 1000, 1000);
        let converter = CoordConverter::new(monitor, CoordinateSystem::Qwen);
        let (x, y) = converter.to_screen((250.0, 750.0));
        assert_eq!(x, 250);
        assert_eq!(y, 750);
    }

    #[test]
    fn test_coordinate_system_from_str() {
        assert_eq!(
            CoordinateSystem::from_str("qwen"),
            Some(CoordinateSystem::Qwen)
        );
        assert_eq!(
            CoordinateSystem::from_str("kimi"),
            Some(CoordinateSystem::Kimi)
        );
        assert_eq!(
            CoordinateSystem::from_str("pixel"),
            Some(CoordinateSystem::Pixel)
        );
        assert_eq!(CoordinateSystem::from_str("unknown"), None);
    }
}

//! §3.3 style loop tests: mock annotation HTTP + in-memory action backend (no real display input).

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{DynamicImage, RgbImage};
use pointer_core::agents::computer::actions::{
    ActionBackend, ActionExecutor, ActionResult, KeyPhase, MouseButton, MouseMoveProfile,
};
use pointer_core::agents::computer::screen::MonitorInfo;
use pointer_core::agents::computer::vision_state::VisionState;
use pointer_core::agents::computer::ComputerState;
use serde_json::json;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn solid_jpeg(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut buf = Vec::new();
    let img = RgbImage::from_pixel(width, height, image::Rgb(rgb));
    DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    buf
}

fn tiny_png_bytes() -> Vec<u8> {
    let mut buf = Vec::new();
    let img = RgbImage::from_pixel(1, 1, image::Rgb([0u8, 0u8, 0u8]));
    DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

struct SharedRecordingBackend {
    moves: Arc<Mutex<Vec<(i32, i32)>>>,
}

impl ActionBackend for SharedRecordingBackend {
    fn click(&self) -> Result<ActionResult> {
        Ok(ActionResult::success("click"))
    }
    fn double_click(&self) -> Result<ActionResult> {
        Ok(ActionResult::success("double"))
    }
    fn right_click(&self) -> Result<ActionResult> {
        Ok(ActionResult::success("right"))
    }
    fn move_to_with_profile(
        &self,
        x: i32,
        y: i32,
        _profile: MouseMoveProfile,
    ) -> Result<ActionResult> {
        self.moves.lock().unwrap().push((x, y));
        Ok(ActionResult::success("move"))
    }
    fn scroll(&self, _lines: i32) -> Result<ActionResult> {
        Ok(ActionResult::success("scroll"))
    }
    fn type_text(&self, _text: &str) -> Result<ActionResult> {
        Ok(ActionResult::success("type"))
    }
    fn hotkey(&self, _keys: &[&str]) -> Result<ActionResult> {
        Ok(ActionResult::success("hotkey"))
    }
    fn get_position(&self) -> Result<(i32, i32)> {
        Ok((0, 0))
    }
    fn key_phase(&self, _name: &str, _phase: KeyPhase) -> Result<ActionResult> {
        Ok(ActionResult::success("key"))
    }
    fn mouse_phase(&self, _button: MouseButton, _phase: KeyPhase) -> Result<ActionResult> {
        Ok(ActionResult::success("mouse_phase"))
    }
}

#[tokio::test]
async fn computer_state_apply_screen_capture_reuses_prior_jpeg() {
    let state = ComputerState::with_annotate_url("");
    let jpeg_a = solid_jpeg(64, 64, [40, 80, 120]);
    let jpeg_b = solid_jpeg(64, 64, [200, 10, 90]);
    let monitor = MonitorInfo::new(0, 0, 64, 64);

    let first = state
        .apply_screen_capture("test", &jpeg_a, monitor, (64, 64), (10, 10), None)
        .await
        .expect("first pipeline");
    assert!(first.inject_before_action.is_none());

    let second = state
        .apply_screen_capture("test", &jpeg_b, monitor, (64, 64), (20, 20), None)
        .await
        .expect("second pipeline");
    let before = second
        .inject_before_action
        .as_ref()
        .expect("prior jpeg reused as before frame");
    assert_eq!(
        before.screen_jpeg.as_slice(),
        first.screenshot_jpeg.as_slice()
    );
    assert!(before.zoom_pointer_png.is_empty());
}

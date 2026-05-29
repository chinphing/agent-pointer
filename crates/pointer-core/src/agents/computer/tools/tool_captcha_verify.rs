use super::args_util::{json_bool_loose, require_non_empty_str, resolve_index_pixels};
use super::dati_client::{query_until_ready, upload, DatiConfig};
use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::state::ComputerState;
use crate::agents::computer::vision::screen::MonitorInfo;
use crate::agents::computer::vision_state::{ElementInfo, VisionState};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::ImageFormat;
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::Value;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const QUERY_TIMEOUT_SECS: u64 = 60;
const POLL_INTERVAL_SECS: u64 = 1;

pub struct CaptchaVerifyTool {
    executor: Arc<Mutex<ActionExecutor>>,
    computer_state: Arc<ComputerState>,
    conversation_id: String,
    vision_state: Arc<Mutex<VisionState>>,
}

impl CaptchaVerifyTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        computer_state: Arc<ComputerState>,
        conversation_id: String,
        vision_state: Arc<Mutex<VisionState>>,
    ) -> Self {
        Self {
            executor,
            computer_state,
            conversation_id,
            vision_state,
        }
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "type" => self.do_type(args),
            "click" => self.do_click(args),
            "drag" => self.do_drag(args),
            _ => Err(anyhow!("Use method: type, click, or drag.")),
        }
    }

    fn do_type(&self, args: &Value) -> Result<String> {
        let goal = require_non_empty_str(args, "goal")?;
        let index_captcha_area = required_u32(args, "index_captcha_area")?;
        let index_input_area = required_u32(args, "index_input_area")?;
        let remark = arg_text(args, "remark").unwrap_or_default();
        let answer = self.extract_and_solve(index_captcha_area, &remark)?;
        let input_pos = {
            let vision = self.vision_state.lock().unwrap();
            resolve_index_pixels(&vision, args, index_input_area)?
        };
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(input_pos.0, input_pos.1, &answer, true, false, true)?;
        Ok(format!(
            "Goal: {goal}. Type action executed, cleared first. Please verify result on next screenshot."
        ))
    }

    fn do_click(&self, args: &Value) -> Result<String> {
        let goal = require_non_empty_str(args, "goal")?;
        let index_captcha_area = required_u32(args, "index_captcha_area")?;
        let remark = arg_text(args, "remark").unwrap_or_default();
        let answer = self.extract_and_solve(index_captcha_area, &remark)?;
        let points = self.answer_to_screen_points(index_captcha_area, &answer)?;
        if points.is_empty() {
            anyhow::bail!("No coordinates in answer.");
        }
        let executor = self.executor.lock().unwrap();
        for &(x, y) in &points {
            executor.click_at(x, y, true)?;
        }
        Ok(format!(
            "Goal: {goal}. Clicked {} point(s). Verify on next screenshot.",
            points.len()
        ))
    }

    fn do_drag(&self, args: &Value) -> Result<String> {
        let goal = require_non_empty_str(args, "goal")?;
        let index_captcha_area = required_u32(args, "index_captcha_area")?;
        let remark = arg_text(args, "remark").unwrap_or_default();
        let answer = self.extract_and_solve(index_captcha_area, &remark)?;
        let mut points = self.answer_to_screen_points(index_captcha_area, &answer)?;
        if points.len() < 2 {
            anyhow::bail!("Drag requires at least 2 coordinates in answer.");
        }

        let is_slider = json_bool_loose(args.get("is_slider"));
        let slider_handle = args
            .get("index_slider_arrow")
            .or_else(|| args.get("index_slider_handle"));
        let mut used_handle: Option<u32> = None;
        if let Some(v) = slider_handle {
            if !is_slider {
                anyhow::bail!(
                    "index_slider_arrow / index_slider_handle is only valid when is_slider=true."
                );
            }
            let handle_index = value_to_u32(v, "index_slider_arrow")?;
            let handle_pos = {
                let vision = self.vision_state.lock().unwrap();
                resolve_index_pixels(&vision, args, handle_index)?
            };
            translate_points(&mut points, handle_pos);
            used_handle = Some(handle_index);
        }

        if is_slider {
            let offset = crate::platform_config::effective_settings_global()
                .captcha_slider_offset_px;
            if let Some(last) = points.last_mut() {
                last.0 += offset;
            }
        }

        let executor = self.executor.lock().unwrap();
        executor.drag_left_through_points(&points, true)?;
        let handle_note = used_handle
            .map(|i| format!(" using slider arrow index {i}"))
            .unwrap_or_default();
        Ok(format!(
            "Goal: {goal}. Drag along {} point(s){handle_note}. Verify on next screenshot.",
            points.len()
        ))
    }

    fn extract_and_solve(&self, index_captcha_area: u32, remark: &str) -> Result<String> {
        let image = self.crop_captcha_as_data_png(index_captcha_area)?;
        let cfg = DatiConfig::from_settings_and_env();
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .build()?;
        let subjectno = upload(&client, &cfg, &image, remark)?;
        query_until_ready(
            &client,
            &cfg,
            &subjectno,
            Duration::from_secs(QUERY_TIMEOUT_SECS),
            Duration::from_secs(POLL_INTERVAL_SECS),
        )
    }

    fn crop_captcha_as_data_png(&self, index_captcha_area: u32) -> Result<String> {
        let (jpeg, monitor, capture_px) = self
            .computer_state
            .current_turn_raw_capture_for_conversation(&self.conversation_id)?;
        let elem = {
            let vision = self.vision_state.lock().unwrap();
            element_from_vision(&vision, index_captcha_area)?
        };
        crop_element_from_jpeg(&jpeg, &monitor, capture_px, &elem)
    }

    fn answer_to_screen_points(
        &self,
        index_captcha_area: u32,
        answer: &str,
    ) -> Result<Vec<(i32, i32)>> {
        let rel = parse_coords_result(answer)?;
        let elem = {
            let vision = self.vision_state.lock().unwrap();
            element_from_vision(&vision, index_captcha_area)?
        };
        let left = (elem.center_x as f32 - elem.width / 2.0).round() as i32;
        let top = (elem.center_y as f32 - elem.height / 2.0).round() as i32;
        Ok(rel.into_iter().map(|(x, y)| (left + x, top + y)).collect())
    }
}

fn crop_element_from_jpeg(
    jpeg: &[u8],
    monitor: &MonitorInfo,
    capture_px: (u32, u32),
    elem: &ElementInfo,
) -> Result<String> {
    let img = image::load_from_memory(jpeg)?;
    let left = ((elem.center_x as f32 - elem.width / 2.0).round() as i32 - monitor.left)
        .clamp(0, capture_px.0.saturating_sub(1) as i32) as u32;
    let top = ((elem.center_y as f32 - elem.height / 2.0).round() as i32 - monitor.top)
        .clamp(0, capture_px.1.saturating_sub(1) as i32) as u32;
    let right = ((elem.center_x as f32 + elem.width / 2.0).round() as i32 - monitor.left)
        .clamp(left as i32 + 1, capture_px.0 as i32) as u32;
    let bottom = ((elem.center_y as f32 + elem.height / 2.0).round() as i32 - monitor.top)
        .clamp(top as i32 + 1, capture_px.1 as i32) as u32;

    let crop = img.crop_imm(left, top, right - left, bottom - top);
    let mut buf = Vec::new();
    crop.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(buf)))
}

fn element_from_vision(vision: &VisionState, index: u32) -> Result<ElementInfo> {
    vision
        .element(index)
        .cloned()
        .ok_or_else(|| anyhow!("Index {index} not found in current annotation"))
}

fn parse_coords_result(answer: &str) -> Result<Vec<(i32, i32)>> {
    let re = Regex::new(r"(-?\d+)\s*,\s*(-?\d+)")?;
    Ok(re
        .captures_iter(answer)
        .filter_map(|cap| {
            let x = cap.get(1)?.as_str().parse::<i32>().ok()?;
            let y = cap.get(2)?.as_str().parse::<i32>().ok()?;
            Some((x, y))
        })
        .collect())
}

fn translate_points(points: &mut [(i32, i32)], new_start: (i32, i32)) {
    if let Some((x, y)) = points.first().copied() {
        let dx = new_start.0 - x;
        let dy = new_start.1 - y;
        for p in points {
            p.0 += dx;
            p.1 += dy;
        }
    }
}

fn required_u32(args: &Value, key: &str) -> Result<u32> {
    let value = args
        .get(key)
        .ok_or_else(|| anyhow!("Missing {key}."))?;
    value_to_u32(value, key)
}

fn value_to_u32(value: &Value, key: &str) -> Result<u32> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|i| u64::try_from(i).ok()))
        .map(|n| n as u32)
        .ok_or_else(|| anyhow!("{key} must be integer."))
}

fn arg_text(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_coords_extracts_vendor_answer_points() {
        let points = parse_coords_result("2,143|64,82|160,44|228,52").unwrap();
        assert_eq!(points, vec![(2, 143), (64, 82), (160, 44), (228, 52)]);
    }

    #[test]
    fn translate_points_keeps_relative_offsets() {
        let mut points = vec![(10, 20), (30, 40)];
        translate_points(&mut points, (100, 200));
        assert_eq!(points, vec![(100, 200), (120, 220)]);
    }

    #[test]
    fn crop_element_from_jpeg_uses_monitor_relative_coords() {
        let mut rgba = image::RgbaImage::new(100, 80);
        for x in 20..40 {
            for y in 10..30 {
                rgba.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();

        let monitor = MonitorInfo::new(0, 0, 100, 80);
        let elem = ElementInfo {
            index: 1,
            center_x: 30,
            center_y: 20,
            width: 20.0,
            height: 20.0,
            norm_left: 0,
            norm_top: 0,
            norm_right: 0,
            norm_bottom: 0,
        };
        let out = crop_element_from_jpeg(&jpeg, &monitor, (100, 80), &elem).unwrap();
        assert!(out.starts_with("data:image/png;base64,"));
    }
}

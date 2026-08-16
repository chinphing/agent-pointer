//! Volcengine Ark (Doubao Seedream / Seedance) image and video generation.

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::billing::{record_generation_usage, GenerationUsage};
use super::dashscope::{GenerationArtifact, ImageGenerateRequest, VideoGenerateRequest};
use super::models::{
    is_seedance_v2_model, provider_is_volcengine, resolve_volcengine_video_model,
    volcengine_image_url, volcengine_video_task_url, volcengine_video_tasks_url, GenerationKind,
    ResolvedGenerationConfig,
};
use super::reference_image::resolve_reference_image_for_api;
use super::save::{download_url_to_file, save_generated_bytes};

const TIMEOUT_SECS: u64 = 600;
const POLL_INTERVAL_MS: u64 = 5000;
const MAX_POLL_ATTEMPTS: u32 = 120;

async fn poll_volcengine_video(
    client: &reqwest::Client,
    cfg: &ResolvedGenerationConfig,
    task_id: &str,
    cancel: &CancellationToken,
) -> Result<Value> {
    let url = volcengine_video_task_url(&cfg.base_url, task_id);
    for _ in 0..MAX_POLL_ATTEMPTS {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", cfg.api_key.trim()))
            .send()
            .await
            .context("poll volcengine video")?;
        let text = resp.text().await.context("read poll body")?;
        let payload: Value = serde_json::from_str(&text).context("parse poll json")?;
        let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
        match status {
            "succeeded" => return Ok(payload),
            "failed" | "cancelled" => {
                let msg = payload
                    .pointer("/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Volcengine video task failed");
                return Err(anyhow!("{msg}"));
            }
            _ => tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await,
        }
    }
    Err(anyhow!("Volcengine video task {task_id} timed out"))
}

pub async fn generate_image_volcengine(
    cfg: &ResolvedGenerationConfig,
    conversation_id: &str,
    run_id: &str,
    req: &ImageGenerateRequest,
    cancel: CancellationToken,
) -> Result<GenerationArtifact> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()
        .context("build HTTP client")?;
    let mut body = json!({
        "model": cfg.model,
        "prompt": req.prompt,
        "size": req.size.clone().unwrap_or_else(|| "2K".into()),
        "response_format": "url",
        "stream": false,
        "watermark": false,
    });
    if let Some(raw) = req.image_url.as_deref().filter(|s| !s.trim().is_empty()) {
        body["image"] = json!(resolve_reference_image_for_api(raw)?);
    }
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }
    let url = volcengine_image_url(&cfg.base_url);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("volcengine image request")?;
    let status = resp.status();
    let text = resp.text().await.context("read image body")?;
    if !status.is_success() {
        return Err(anyhow!("Volcengine image HTTP {status}: {text}"));
    }
    let payload: Value = serde_json::from_str(&text).context("parse image json")?;
    let mut urls = Vec::new();
    if let Some(data) = payload.get("data").and_then(|v| v.as_array()) {
        for item in data {
            if let Some(url) = item.get("url").and_then(|v| v.as_str()) {
                urls.push(url.to_string());
            } else if let Some(b64) = item.get("b64_json").and_then(|v| v.as_str()) {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .context("decode b64 image")?;
                let path = save_generated_bytes(conversation_id, &bytes, "gen.png")?;
                urls.push(format!("file://{}", path.display()));
            }
        }
    }
    if urls.is_empty() {
        return Err(anyhow!("Volcengine image response contained no images"));
    }
    let count = urls.len() as u32;
    let usage = GenerationUsage::per_image(count);
    let mut local_paths = Vec::new();
    for image_url in urls {
        if image_url.starts_with("file://") {
            local_paths.push(image_url.trim_start_matches("file://").to_string());
            continue;
        }
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        let path = save_generated_bytes(conversation_id, &[], "gen.png")?;
        download_url_to_file(&client, &image_url, &path).await?;
        local_paths.push(path.to_string_lossy().into_owned());
    }
    record_generation_usage(
        run_id,
        conversation_id,
        GenerationKind::Image,
        &cfg.model,
        &usage,
        cfg.source,
    );
    Ok(GenerationArtifact {
        local_paths,
        model: cfg.model.clone(),
        provider: cfg.provider_id.clone(),
        usage,
    })
}

pub async fn generate_video_volcengine(
    cfg: &ResolvedGenerationConfig,
    conversation_id: &str,
    run_id: &str,
    req: &VideoGenerateRequest,
    cancel: CancellationToken,
) -> Result<GenerationArtifact> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()
        .context("build HTTP client")?;
    let mut req = req.clone();
    if let Some(raw) = req.image_url.as_deref().filter(|s| !s.trim().is_empty()) {
        req.image_url = Some(resolve_reference_image_for_api(raw)?);
    }
    let has_image = req
        .image_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some();
    let model = resolve_volcengine_video_model(&cfg.model, has_image);
    let seedance_v2 = is_seedance_v2_model(&model);
    let mut content = vec![json!({"type": "text", "text": req.prompt})];
    if let Some(url) = req.image_url.as_deref().filter(|s| !s.trim().is_empty()) {
        content.push(json!({
            "type": "image_url",
            "image_url": {"url": url},
            "role": "first_frame",
        }));
    }
    let mut body = json!({
        "model": model,
        "content": content,
    });
    if let Some(res) = req.size.as_deref() {
        body["resolution"] = json!(res.to_ascii_lowercase());
    } else if seedance_v2 {
        body["resolution"] = json!("1080p");
    } else {
        body["resolution"] = json!("720p");
    }
    if seedance_v2 {
        body["ratio"] = json!(if has_image { "adaptive" } else { "16:9" });
        let duration = req.duration_seconds.unwrap_or(5).clamp(4, 15);
        body["duration"] = json!(duration);
    } else if let Some(d) = req.duration_seconds {
        body["duration"] = json!(d);
    }
    if let Some(audio) = req.audio {
        body["generate_audio"] = json!(audio);
    } else if seedance_v2 {
        body["generate_audio"] = json!(true);
    }
    body["watermark"] = json!(false);
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }
    let submit_url = volcengine_video_tasks_url(&cfg.base_url);
    let resp = client
        .post(&submit_url)
        .header("Authorization", format!("Bearer {}", cfg.api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("volcengine video submit")?;
    let status = resp.status();
    let text = resp.text().await.context("read video submit")?;
    if !status.is_success() {
        return Err(anyhow!("Volcengine video HTTP {status}: {text}"));
    }
    let submitted: Value = serde_json::from_str(&text).context("parse submit json")?;
    let task_id = submitted
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Volcengine video missing task id"))?;
    let completed = poll_volcengine_video(&client, cfg, task_id, &cancel).await?;
    let video_url = completed
        .pointer("/content/video_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Volcengine video missing video_url"))?;
    let fallback = req.duration_seconds.unwrap_or(5);
    let duration = completed
        .get("duration")
        .or_else(|| completed.pointer("/usage/duration"))
        .or_else(|| completed.pointer("/usage/output_video_duration"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(fallback)
        .max(1);
    let usage = GenerationUsage::per_video_seconds(duration);
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }
    let path = save_generated_bytes(conversation_id, &[], "gen.mp4")?;
    download_url_to_file(&client, video_url, &path).await?;
    record_generation_usage(
        run_id,
        conversation_id,
        GenerationKind::Video,
        &model,
        &usage,
        cfg.source,
    );
    Ok(GenerationArtifact {
        local_paths: vec![path.to_string_lossy().into_owned()],
        model,
        provider: cfg.provider_id.clone(),
        usage,
    })
}

pub fn route_volcengine(cfg: &ResolvedGenerationConfig) -> bool {
    provider_is_volcengine(&cfg.provider_id, &cfg.base_url)
}

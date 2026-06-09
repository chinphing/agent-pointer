//! DashScope (Qwen / Wan) image and video generation.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::billing::{GenerationUsage, record_generation_usage};
use super::models::{
    dashscope_multimodal_image_url, dashscope_tasks_url, dashscope_video_synthesis_url,
    GenerationKind, ResolvedGenerationConfig,
};
use super::save::{download_url_to_file, save_generated_bytes};

const IMAGE_TIMEOUT_SECS: u64 = 180;
const VIDEO_TIMEOUT_SECS: u64 = 600;
const POLL_INTERVAL_MS: u64 = 2500;
const MAX_POLL_ATTEMPTS: u32 = 120;

#[derive(Debug, Clone)]
pub struct ImageGenerateRequest {
    pub prompt: String,
    pub size: Option<String>,
    pub n: u32,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoGenerateRequest {
    pub prompt: String,
    pub size: Option<String>,
    pub duration_seconds: Option<u32>,
    pub image_url: Option<String>,
    pub audio: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct GenerationArtifact {
    pub local_paths: Vec<String>,
    pub model: String,
    pub provider: String,
    pub usage: GenerationUsage,
}

fn auth_headers(api_key: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {}", api_key.trim()).parse().expect("auth header"),
    );
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().expect("content-type"),
    );
    headers
}

fn extract_image_urls(payload: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(choices) = payload
        .pointer("/output/choices")
        .and_then(|v| v.as_array())
    {
        for choice in choices {
            if let Some(content) = choice.pointer("/message/content").and_then(|v| v.as_array()) {
                for item in content {
                    if let Some(url) = item.get("image").and_then(|v| v.as_str()) {
                        urls.push(url.to_string());
                    }
                }
            }
        }
    }
    if urls.is_empty() {
        if let Some(results) = payload.pointer("/output/results").and_then(|v| v.as_array()) {
            for entry in results {
                if let Some(url) = entry.get("url").and_then(|v| v.as_str()) {
                    urls.push(url.to_string());
                }
            }
        }
    }
    urls
}

fn extract_video_urls(payload: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(results) = payload.pointer("/output/results").and_then(|v| v.as_array()) {
        for entry in results {
            if let Some(url) = entry.get("video_url").and_then(|v| v.as_str()) {
                urls.push(url.to_string());
            }
        }
    }
    if let Some(url) = payload.pointer("/output/video_url").and_then(|v| v.as_str()) {
        urls.push(url.to_string());
    }
    urls
}

async fn poll_dashscope_task(
    client: &reqwest::Client,
    cfg: &ResolvedGenerationConfig,
    task_id: &str,
    cancel: &CancellationToken,
) -> Result<Value> {
    let url = dashscope_tasks_url(&cfg.base_url, task_id);
    let headers = auth_headers(&cfg.api_key);
    for _ in 0..MAX_POLL_ATTEMPTS {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        let resp = client
            .get(&url)
            .headers(headers.clone())
            .send()
            .await
            .context("poll dashscope task")?;
        let text = resp.text().await.context("read poll body")?;
        let payload: Value = serde_json::from_str(&text).context("parse poll json")?;
        let status = payload
            .pointer("/output/task_status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        match status.as_str() {
            "SUCCEEDED" => return Ok(payload),
            "FAILED" | "CANCELED" | "UNKNOWN" => {
                let msg = payload
                    .pointer("/output/message")
                    .or_else(|| payload.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("DashScope task failed");
                return Err(anyhow!("{msg}"));
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
            }
        }
    }
    Err(anyhow!("DashScope task {task_id} timed out"))
}

async fn download_to_conversation(
    client: &reqwest::Client,
    conversation_id: &str,
    url: &str,
    default_ext: &str,
) -> Result<String> {
    let tmp_name = format!("gen{default_ext}");
    let path = save_generated_bytes(conversation_id, &[], &tmp_name)?;
    download_url_to_file(client, url, &path).await?;
    Ok(path.to_string_lossy().into_owned())
}

pub async fn generate_image_dashscope(
    cfg: &ResolvedGenerationConfig,
    conversation_id: &str,
    run_id: &str,
    req: &ImageGenerateRequest,
    cancel: CancellationToken,
) -> Result<GenerationArtifact> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(IMAGE_TIMEOUT_SECS))
        .build()
        .context("build HTTP client")?;
    let mut content = vec![json!({"text": req.prompt})];
    if let Some(url) = req.image_url.as_deref().filter(|s| !s.trim().is_empty()) {
        content.insert(0, json!({"image": url}));
    }
    let mut parameters = json!({
        "n": req.n.max(1).min(4),
        "watermark": false,
        "thinking_mode": true,
    });
    if let Some(size) = req.size.as_deref().filter(|s| !s.trim().is_empty()) {
        parameters["size"] = json!(size);
    }
    let body = json!({
        "model": cfg.model,
        "input": {
            "messages": [{
                "role": "user",
                "content": content,
            }]
        },
        "parameters": parameters,
    });
    let url = dashscope_multimodal_image_url(&cfg.base_url);
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }
    let resp = client
        .post(&url)
        .headers(auth_headers(&cfg.api_key))
        .json(&body)
        .send()
        .await
        .context("dashscope image request")?;
    let status = resp.status();
    let text = resp.text().await.context("read image response")?;
    if !status.is_success() {
        return Err(anyhow!("DashScope image HTTP {status}: {text}"));
    }
    let payload: Value = serde_json::from_str(&text).context("parse image json")?;
    let urls = extract_image_urls(&payload);
    if urls.is_empty() {
        return Err(anyhow!("DashScope image response contained no image URLs"));
    }
    let usage = payload
        .get("usage")
        .map(|u| GenerationUsage::from_dashscope_usage(u, GenerationKind::Image))
        .unwrap_or_else(|| GenerationUsage::per_image(urls.len() as u32));
    let mut local_paths = Vec::new();
    for (i, image_url) in urls.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        let path = download_to_conversation(&client, conversation_id, image_url, ".png").await?;
        local_paths.push(path);
        log::info!("dashscope image saved index={i} model={}", cfg.model);
    }
    record_generation_usage(run_id, conversation_id, GenerationKind::Image, &cfg.model, &usage);
    Ok(GenerationArtifact {
        local_paths,
        model: cfg.model.clone(),
        provider: cfg.provider_id.clone(),
        usage,
    })
}

pub async fn generate_video_dashscope(
    cfg: &ResolvedGenerationConfig,
    conversation_id: &str,
    run_id: &str,
    req: &VideoGenerateRequest,
    cancel: CancellationToken,
) -> Result<GenerationArtifact> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(VIDEO_TIMEOUT_SECS))
        .build()
        .context("build HTTP client")?;
    let mut input = json!({"prompt": req.prompt});
    if let Some(url) = req.image_url.as_deref().filter(|s| !s.trim().is_empty()) {
        input["img_url"] = json!(url);
    }
    let mut parameters = json!({
        "prompt_extend": true,
        "watermark": false,
    });
    if let Some(size) = req.size.as_deref().filter(|s| !s.trim().is_empty()) {
        parameters["size"] = json!(size);
    } else {
        parameters["size"] = json!("1280*720");
    }
    if let Some(d) = req.duration_seconds {
        parameters["duration"] = json!(d);
    }
    if let Some(audio) = req.audio {
        parameters["enable_audio"] = json!(audio);
    }
    if cfg.model.contains("wan2.6") || cfg.model.contains("wan2.7") {
        parameters["shot_type"] = json!("multi");
    }
    let body = json!({
        "model": cfg.model,
        "input": input,
        "parameters": parameters,
    });
    let url = dashscope_video_synthesis_url(&cfg.base_url);
    if cancel.is_cancelled() {
        return Err(anyhow!("已停止生成"));
    }
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.api_key.trim()))
        .header("Content-Type", "application/json")
        .header("X-DashScope-Async", "enable")
        .json(&body)
        .send()
        .await
        .context("dashscope video submit")?;
    let status = resp.status();
    let text = resp.text().await.context("read video submit body")?;
    if !status.is_success() {
        return Err(anyhow!("DashScope video HTTP {status}: {text}"));
    }
    let submitted: Value = serde_json::from_str(&text).context("parse video submit json")?;
    let task_id = submitted
        .pointer("/output/task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("DashScope video missing task_id"))?;
    let completed = poll_dashscope_task(&client, cfg, task_id, &cancel).await?;
    let urls = extract_video_urls(&completed);
    if urls.is_empty() {
        return Err(anyhow!("DashScope video completed without URLs"));
    }
    let usage = completed
        .get("usage")
        .map(|u| GenerationUsage::from_dashscope_usage(u, GenerationKind::Video))
        .unwrap_or_else(|| {
            GenerationUsage::per_video_seconds(req.duration_seconds.unwrap_or(5))
        });
    let mut local_paths = Vec::new();
    for video_url in &urls {
        if cancel.is_cancelled() {
            return Err(anyhow!("已停止生成"));
        }
        let path = download_to_conversation(&client, conversation_id, video_url, ".mp4").await?;
        local_paths.push(path);
    }
    record_generation_usage(run_id, conversation_id, GenerationKind::Video, &cfg.model, &usage);
    Ok(GenerationArtifact {
        local_paths,
        model: cfg.model.clone(),
        provider: cfg.provider_id.clone(),
        usage,
    })
}

//! Platform experience (SOP) list/detail/home for new-session welcome panel.
//!
//! Standalone deployments never load experiences (no official catalog).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::platform_endpoints;

const DEFAULT_TIMEOUT_SEC: u64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceListItem {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub excerpt: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub category_name_zh: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub cover_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceDetail {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub prompt_text: String,
    #[serde(default)]
    pub narrative_text: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub cover_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceHomeCategoryBlock {
    pub id: String,
    pub name_zh: String,
    pub items: Vec<ExperienceListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceHomeResponse {
    pub featured: Vec<ExperienceListItem>,
    pub categories: Vec<ExperienceHomeCategoryBlock>,
}

fn experiences_enabled() -> bool {
    if crate::deployment_mode::is_standalone() {
        return false;
    }
    !platform_endpoints::api_base().trim().is_empty()
}

fn experiences_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SEC))
        .build()
        .context("build experiences http client")
}

pub async fn fetch_pinned_experiences(limit: usize) -> Result<Vec<ExperienceListItem>> {
    if !experiences_enabled() {
        log::info!("experiences: skipped (standalone or no POINTER_API_BASE)");
        return Ok(Vec::new());
    }
    let limit = limit.max(1).min(20);
    let url = format!(
        "{}/api/experiences?pinned_only=true&page_size={limit}",
        platform_endpoints::api_base().trim_end_matches('/')
    );
    let client = experiences_client()?;
    let rows: Vec<ExperienceListItem> = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("experiences list bad status for {url}"))?
        .json()
        .await
        .context("parse experiences list json")?;
    Ok(rows.into_iter().take(limit).collect())
}

pub async fn fetch_experience_home() -> Result<ExperienceHomeResponse> {
    if !experiences_enabled() {
        log::info!("experiences: home skipped (standalone or no POINTER_API_BASE)");
        return Ok(ExperienceHomeResponse {
            featured: Vec::new(),
            categories: Vec::new(),
        });
    }
    let url = format!(
        "{}/api/experiences/home",
        platform_endpoints::api_base().trim_end_matches('/')
    );
    let client = experiences_client()?;
    client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("experiences home bad status for {url}"))?
        .json()
        .await
        .context("parse experiences home json")
}

pub async fn fetch_experience_search(query: &str, limit: usize) -> Result<Vec<ExperienceListItem>> {
    if !experiences_enabled() {
        return Ok(Vec::new());
    }
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.max(1).min(50);
    let base = platform_endpoints::api_base();
    let base = base.trim_end_matches('/');
    let client = experiences_client()?;
    let rows: Vec<ExperienceListItem> = client
        .get(format!("{base}/api/experiences"))
        .query(&[("q", q), ("page_size", &limit.to_string())])
        .send()
        .await
        .with_context(|| format!("GET {base}/api/experiences?q=..."))?
        .error_for_status()
        .with_context(|| "experiences search bad status")?
        .json()
        .await
        .context("parse experiences search json")?;
    Ok(rows)
}

pub async fn fetch_experience_detail(slug: &str) -> Result<ExperienceDetail> {
    if !experiences_enabled() {
        anyhow::bail!("experiences disabled in standalone mode");
    }
    let slug = slug.trim();
    if slug.is_empty() {
        anyhow::bail!("experience slug is empty");
    }
    let url = format!(
        "{}/api/experiences/{slug}",
        platform_endpoints::api_base().trim_end_matches('/')
    );
    let client = experiences_client()?;
    client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("experience detail bad status for {url}"))?
        .json()
        .await
        .context("parse experience detail json")
}

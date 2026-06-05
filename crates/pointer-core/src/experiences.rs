//! Platform experience (SOP) list/detail for new-session home suggestions.

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceDetail {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub prompt_text: String,
    #[serde(default)]
    pub narrative_text: Option<String>,
}

fn experiences_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SEC))
        .build()
        .context("build experiences http client")
}

pub async fn fetch_pinned_experiences(limit: usize) -> Result<Vec<ExperienceListItem>> {
    let limit = limit.max(1).min(20);
    let url = format!(
        "{}/api/experiences?pinned_only=true&limit={limit}",
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

pub async fn fetch_experience_detail(slug: &str) -> Result<ExperienceDetail> {
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

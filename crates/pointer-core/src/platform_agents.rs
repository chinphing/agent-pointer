//! Pointer platform cloud agent / shop API client (desktop Bearer auth).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::platform_auth::PlatformAuthManager;

const HTTP_TIMEOUT_SEC: u64 = 60;

fn api_base() -> String {
    crate::platform_endpoints::api_base()
        .trim_end_matches('/')
        .to_string()
}

async fn bearer(auth: &PlatformAuthManager) -> Result<String> {
    auth.ensure_access_token().await
}

async fn parse_error(resp: reqwest::Response) -> anyhow::Error {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("detail")
                .and_then(|d| d.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| text.trim().to_string());
    anyhow!("HTTP {status}: {detail}")
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SEC))
        .build()
        .context("build platform agents http client")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlatformMe {
    pub balance_yuan: String,
    pub nickname: Option<String>,
    #[serde(default)]
    pub token_quota_exhausted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ShopRegion {
    pub id: String,
    pub code: String,
    pub name_zh: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MachineTierPublic {
    pub id: String,
    pub label_zh: String,
    #[serde(default)]
    pub description_zh: String,
    pub vcpu: Option<i32>,
    pub memory_gb: Option<i32>,
    pub disk_gb: Option<i32>,
    pub price_multiplier: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ShopPricing {
    pub trial_hourly_rate_yuan: String,
    pub hourly_rate_yuan: String,
    pub monthly_yuan: String,
    pub yearly_yuan: String,
    #[serde(default)]
    pub trial_hourly_public_note: String,
    #[serde(default)]
    pub hourly_public_note: String,
    #[serde(default)]
    pub machine_tiers: Vec<MachineTierPublic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ShopPreviewResult {
    pub gross_yuan: String,
    pub discount_yuan: String,
    pub paid_yuan: String,
    pub discount_percent_applied: i32,
    pub pricing: ShopPricing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServerRegionBrief {
    pub id: String,
    pub code: String,
    pub name_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CloudAgent {
    pub id: String,
    pub machine_tier_id: Option<String>,
    pub package_kind: Option<String>,
    pub hours_purchased: Option<i32>,
    pub prepaid_months: Option<i32>,
    pub prepaid_years: Option<i32>,
    pub expires_at: Option<String>,
    pub instance_status: String,
    pub app_status: String,
    pub ecs_instance_id: Option<String>,
    pub public_ip: Option<String>,
    pub private_ip: Option<String>,
    pub last_probe_error: Option<String>,
    pub console_url: Option<String>,
    pub created_at: String,
    pub server_region: Option<ServerRegionBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentPhaseCounts {
    pub running: i32,
    pub starting: i32,
    #[serde(default)]
    pub releasing: i32,
    #[serde(default)]
    pub other: i32,
    pub released: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentListPage {
    pub items: Vec<CloudAgent>,
    pub total: i32,
    pub page: i32,
    pub page_size: i32,
    pub phase_counts: AgentPhaseCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ShopPurchaseResult {
    pub agent: CloudAgent,
    pub order_id: String,
    pub balance_yuan: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentOauthCodeResult {
    pub code: String,
    pub state: String,
    pub expires_in: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ShopPreviewRequest {
    pub region_id: String,
    pub package_kind: String,
    pub hours: Option<i32>,
    pub quantity: Option<i32>,
    pub machine_tier_id: Option<String>,
}

pub async fn fetch_platform_me(auth: &PlatformAuthManager) -> Result<PlatformMe> {
    let url = format!("{}/api/me", api_base());
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse /api/me")
}

pub async fn list_shop_regions(auth: &PlatformAuthManager) -> Result<Vec<ShopRegion>> {
    let url = format!("{}/api/shop/regions", api_base());
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse shop regions")
}

pub async fn fetch_shop_pricing(
    auth: &PlatformAuthManager,
    region_id: &str,
) -> Result<ShopPricing> {
    let url = format!(
        "{}/api/shop/pricing?region_id={}",
        api_base(),
        urlencoding_encode(region_id)
    );
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse shop pricing")
}

pub async fn preview_shop(
    auth: &PlatformAuthManager,
    body: &ShopPreviewRequest,
) -> Result<ShopPreviewResult> {
    let url = format!("{}/api/shop/preview", api_base());
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse shop preview")
}

pub async fn purchase_agent(
    auth: &PlatformAuthManager,
    body: &ShopPreviewRequest,
) -> Result<ShopPurchaseResult> {
    let url = format!("{}/api/shop/purchase", api_base());
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse shop purchase")
}

pub async fn list_agents(
    auth: &PlatformAuthManager,
    page: i32,
    page_size: i32,
    status: &str,
) -> Result<AgentListPage> {
    let url = format!(
        "{}/api/agents?page={page}&page_size={page_size}&status={}",
        api_base(),
        urlencoding_encode(status)
    );
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse agents list")
}

pub async fn get_agent(auth: &PlatformAuthManager, agent_id: &str) -> Result<CloudAgent> {
    let url = format!(
        "{}/api/agents/{}",
        api_base(),
        urlencoding_encode(agent_id)
    );
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse agent detail")
}

pub async fn renew_agent(
    auth: &PlatformAuthManager,
    agent_id: &str,
    body: &ShopPreviewRequest,
) -> Result<ShopPurchaseResult> {
    let url = format!(
        "{}/api/agents/{}/renew",
        api_base(),
        urlencoding_encode(agent_id)
    );
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse renew")
}

pub async fn release_agent(auth: &PlatformAuthManager, agent_id: &str) -> Result<CloudAgent> {
    let url = format!(
        "{}/api/agents/{}/release",
        api_base(),
        urlencoding_encode(agent_id)
    );
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse release")
}

pub async fn create_agent_oauth_code(
    auth: &PlatformAuthManager,
    agent_id: &str,
) -> Result<AgentOauthCodeResult> {
    let url = format!(
        "{}/api/agents/{}/oauth-code",
        api_base(),
        urlencoding_encode(agent_id)
    );
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "state": "default" }))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse oauth code")
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

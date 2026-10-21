//! Provider-backed [`LocateVision`]: the only place in the locator that talks
//! to a model.
//!
//! Both phases use the **computer agent's current tier model** (the provider
//! the tool pass already resolved from `agentModeLlm.computer[mode]`); there is
//! no separate locator configuration.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agents::computer::vision::screen;
use crate::provider::{ChatOnceOutput, OpenAIProvider};

use super::engine::LocateVision;
use super::geometry::NormBox;
use super::prompt::{
    locate_tool, parse_box_from_text, parse_locate_arguments, parse_verify_arguments, report_tool,
    LOCATE_TEXT_FALLBACK,
};
use super::render::LocateImage;
use super::types::VerifyVerdict;

/// Tool-call attempts per model phase before the text fallback / default verdict.
const MAX_CALL_ATTEMPTS: usize = 3;

/// Locator backed by the chat provider (B5 `locator` / `verifier`).
pub struct ChatLocateVision<'a> {
    provider: &'a OpenAIProvider,
    cancel: CancellationToken,
}

impl<'a> ChatLocateVision<'a> {
    pub fn new(provider: &'a OpenAIProvider, cancel: CancellationToken) -> Self {
        Self { provider, cancel }
    }

    fn wire_messages(image: &LocateImage, prompt: &str, suffix: Option<&str>) -> Vec<Value> {
        let mut content = vec![
            json!({"type": "text", "text": "【图】"}),
            json!({
                "type": "image_url",
                "image_url": {
                    "url": format!(
                        "data:image/jpeg;base64,{}",
                        screen::encode_image_to_base64(&image.jpeg)
                    )
                }
            }),
            json!({"type": "text", "text": prompt}),
        ];
        if let Some(suffix) = suffix {
            content.push(json!({"type": "text", "text": suffix}));
        }
        vec![json!({"role": "user", "content": content})]
    }

    async fn chat(
        &self,
        image: &LocateImage,
        prompt: &str,
        tools: Vec<Value>,
        suffix: Option<&str>,
    ) -> Result<ChatOnceOutput> {
        self.provider
            .stream_pipeline_module_with_tools(
                Self::wire_messages(image, prompt, suffix),
                None,
                tools,
                self.cancel.clone(),
                None,
                Some("locate_element"),
            )
            .await
    }
}

#[async_trait]
impl LocateVision for ChatLocateVision<'_> {
    async fn locate(&self, image: &LocateImage, prompt: &str) -> Result<Option<NormBox>> {
        for attempt in 0..MAX_CALL_ATTEMPTS {
            match self.chat(image, prompt, vec![locate_tool()], None).await {
                Ok(out) => {
                    let parsed = out.tool_calls.iter().find_map(|tc| {
                        tc.name
                            .trim()
                            .eq_ignore_ascii_case("locate")
                            .then(|| parse_locate_arguments(&tc.arguments))
                            .flatten()
                    });
                    if let Some(b) = parsed {
                        return Ok(Some(b));
                    }
                    log::debug!(
                        "locate_element: locate attempt {} returned no usable bbox",
                        attempt + 1
                    );
                }
                Err(e) => log::warn!(
                    "locate_element: locate attempt {} failed: {e}",
                    attempt + 1
                ),
            }
        }

        // B5 fallback: no tool call — ask for a bare JSON array.
        match self
            .chat(image, prompt, Vec::new(), Some(LOCATE_TEXT_FALLBACK))
            .await
        {
            Ok(out) => Ok(parse_box_from_text(&out.text)),
            Err(e) => {
                log::warn!("locate_element: locate text fallback failed: {e}");
                Ok(None)
            }
        }
    }

    async fn verify(&self, image: &LocateImage, prompt: &str) -> Result<VerifyVerdict> {
        for attempt in 0..MAX_CALL_ATTEMPTS {
            match self.chat(image, prompt, vec![report_tool()], None).await {
                Ok(out) => {
                    let parsed = out.tool_calls.iter().find_map(|tc| {
                        tc.name
                            .trim()
                            .eq_ignore_ascii_case("report")
                            .then(|| parse_verify_arguments(&tc.arguments))
                            .flatten()
                    });
                    if let Some(v) = parsed {
                        return Ok(v);
                    }
                    log::debug!(
                        "locate_element: verify attempt {} returned no usable verdict",
                        attempt + 1
                    );
                }
                Err(e) => log::warn!(
                    "locate_element: verify attempt {} failed: {e}",
                    attempt + 1
                ),
            }
        }
        // Never silently pass: `VerifyVerdict::default()` is unclear/too_blurry.
        Ok(VerifyVerdict::default())
    }
}

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
use super::prompt::{
    locate_tool, parse_box_from_text, parse_locate_arguments, parse_verify_arguments,
    report_tool_views, LocateAnswer, LOCATE_TEXT_FALLBACK,
};
use super::render::LocateImage;
use super::types::VerifyVerdict;

/// Tool-call attempts per model phase before the text fallback / default verdict.
const MAX_CALL_ATTEMPTS: usize = 3;

/// Locator backed by the chat provider (B5 `locator` / `verifier`).
pub struct ChatLocateVision<'a> {
    provider: &'a OpenAIProvider,
    /// Verifier provider; equals `provider` unless locate verify is configured.
    verify_provider: &'a OpenAIProvider,
    cancel: CancellationToken,
}

impl<'a> ChatLocateVision<'a> {
    /// Locate and verify share one provider (fixtures / default config).
    pub fn new(provider: &'a OpenAIProvider, cancel: CancellationToken) -> Self {
        Self {
            provider,
            verify_provider: provider,
            cancel,
        }
    }

    /// Independent verifier provider (locate verify model configured).
    pub fn with_verify_provider(
        provider: &'a OpenAIProvider,
        verify_provider: &'a OpenAIProvider,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            provider,
            verify_provider,
            cancel,
        }
    }

    fn wire_messages(image: &LocateImage, prompt: &str, suffix: Option<&str>) -> Vec<Value> {
        Self::wire_messages_pair(image, None, prompt, suffix)
    }

    /// Same, with an optional second image labelled 【图二】.
    ///
    /// The label matters: the prompt refers to 「图一」/「图二」, and a model that
    /// cannot tell which picture is which will happily judge the box from the
    /// wide one — which is exactly the image that is too coarse for it.
    fn wire_messages_pair(
        close: &LocateImage,
        wide: Option<&LocateImage>,
        prompt: &str,
        suffix: Option<&str>,
    ) -> Vec<Value> {
        let mut content = vec![json!({"type": "text", "text": "【图一】"})];
        content.push(json!({
            "type": "image_url",
            "image_url": {
                "url": format!(
                    "data:image/jpeg;base64,{}",
                    screen::encode_image_to_base64(&close.jpeg)
                )
            }
        }));
        if let Some(wide) = wide {
            content.push(json!({"type": "text", "text": "【图二】"}));
            content.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!(
                        "data:image/jpeg;base64,{}",
                        screen::encode_image_to_base64(&wide.jpeg)
                    )
                }
            }));
        }
        content.push(json!({"type": "text", "text": prompt}));
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
    async fn locate(&self, image: &LocateImage, prompt: &str) -> Result<Option<LocateAnswer>> {
        for attempt in 0..MAX_CALL_ATTEMPTS {
            match self.chat(image, prompt, vec![locate_tool()], None).await {
                Ok(out) => {
                    // Raw arguments, every call. Without this a box that never
                    // shrinks is indistinguishable from a hint that never
                    // arrived — both look like 「locate ignored the correction」.
                    for tc in &out.tool_calls {
                        log::info!("locate raw args: {}", tc.arguments);
                    }
                    let parsed = out.tool_calls.iter().find_map(|tc| {
                        tc.name
                            .trim()
                            .eq_ignore_ascii_case("locate")
                            .then(|| parse_locate_arguments(&tc.arguments))
                            .flatten()
                    });
                    if let Some(answer) = parsed {
                        // `NotFound` is a complete answer — do not retry it.
                        // Retrying would ask the same question of the same
                        // image and invite the model to invent a box.
                        return Ok(Some(answer));
                    }
                    log::debug!(
                        "locate_element: locate attempt {} returned no usable answer",
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
            Ok(out) => Ok(parse_box_from_text(&out.text).map(LocateAnswer::Found)),
            Err(e) => {
                log::warn!("locate_element: locate text fallback failed: {e}");
                Ok(None)
            }
        }
    }

    async fn verify(&self, image: &LocateImage, prompt: &str) -> Result<VerifyVerdict> {
        self.verify_pair(image, None, prompt).await
    }

    async fn verify_pair(
        &self,
        close: &LocateImage,
        wide: Option<&LocateImage>,
        prompt: &str,
    ) -> Result<VerifyVerdict> {
        for attempt in 0..MAX_CALL_ATTEMPTS {
            let messages = Self::wire_messages_pair(close, wide, prompt, None);
            let call = self
                .verify_provider
                .stream_pipeline_module_with_tools(
                    messages,
                    None,
                    vec![report_tool_views(wide.is_some())],
                    self.cancel.clone(),
                    None,
                    Some("locate_element"),
                )
                .await;
            match call {
                Ok(out) => {
                    let parsed = out.tool_calls.iter().find_map(|tc| {
                        tc.name
                            .trim()
                            .eq_ignore_ascii_case("report")
                            .then(|| parse_verify_arguments(&tc.arguments))
                            .flatten()
                    });
                    if let Some(mut v) = parsed {
                        v.reasoning = out.reasoning_content.clone().unwrap_or_default();
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

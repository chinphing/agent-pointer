//! Production-shaped main-LLM call for the e2e benchmark.
//!
//! Goal: feed the model what production feeds it, so a hint measured here says
//! something about shipped behaviour rather than about a test-only prompt.
//!
//! Reused verbatim from production:
//!   · `AGENT.md`            — read from `src/agents/computer/AGENT.md`
//!   · `[CUR_SCREEN]` preamble — `screen_inject::build_cur_screen_preamble()`
//!   · slot label + base64    — `screen_inject::assemble_cur_screen_payload()`
//!   · `mouse.schema.yaml`    — the same file the tool registry parses
//!
//! **Known divergence (deliberate, documented):** the tool list is the click
//! tool only, not the full 31-tool computer set. Building the full set needs
//! `ComputerState::new(AgentRegistry, SharedPlatformAuth, SharedPlatformConfig)`,
//! which drags in the whole app runtime. For a "produce a bbox" measurement the
//! extra tools are not callable anyway; if the hint rate ever looks suspiciously
//! good, this is the first thing to re-check.

use anyhow::{Context, Result};
use pointer_core::agents::computer::extension_hooks::screen_inject::{
    assemble_cur_screen_payload, build_cur_screen_preamble,
};
use pointer_core::agents::computer::state::ScreenCaptureResult;
use pointer_core::agents::computer::vision::screen::MonitorInfo;
use pointer_core::models::{ChatMessage, SystemPromptSections};
use pointer_core::provider::OpenAIProvider;
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// The click tool as an OpenAI wire entry.
///
/// `mouse.schema.yaml` maps tool name → *parameters* schema; the wire format
/// needs the `{type, function:{name, description, parameters}}` wrapper. Same
/// shape `tools::openai_tool_entry` builds, which is private — reproduced here
/// rather than widened, since the wrapper is three literal keys.
fn click_tool_schema() -> Result<serde_json::Value> {
    const YAML: &str = include_str!("../../src/agents/computer/tools/prompts/mouse.schema.yaml");
    let parsed: serde_json::Value =
        serde_yaml::from_str(YAML).context("parse mouse.schema.yaml")?;
    let params = parsed
        .get("mouse_click_at")
        .cloned()
        .context("mouse.schema.yaml has no mouse_click_at entry")?;
    Ok(json!({
        "type": "function",
        "function": {
            "name": "mouse_click_at",
            "description": "Click a point on screen. Supply the target description and the box the element occupies, in 0-1000 normalized coordinates over the whole screen.",
            "parameters": params
        }
    }))
}

/// System prompt slices, in production order.
///
/// Production (`chat_service/single_agent_prompt.rs:76-100` →
/// `prompts.rs:23-56`) builds:
///
/// ```text
/// COMMUNICATION_PUBLIC
/// <agent system prompts>        ← computer: AGENT.md body + communication slice
/// tool appendix                 ← one section per tool doc
/// memory / user rules / AGENTS.md
/// [Environment]
/// ```
///
/// Everything here comes from the same `pub` accessors production calls, so the
/// text cannot drift. Memory / user rules / workspace `AGENTS.md` are skipped:
/// the benchmark has no user store, and their absence is recorded in the README.
/// Coordinate contract under test.
///
/// `POINTER_E2E_BBOX_UNIT=norm` (default) — the production contract: `bbox` is
/// 0-1000 normalized over the screen.
/// `POINTER_E2E_BBOX_UNIT=pixel` — `bbox` is raw screenshot pixels.
///
/// The two arms exist because normalization forces the model to divide by the
/// image width before answering, and the traces show that step being skipped
/// (`norm=[410,408,455,442]` for a target the model itself placed at pixel 437).
/// One switch, two prompts, so the arms stay comparable.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BboxUnit {
    Norm,
    Pixel,
}

impl BboxUnit {
    fn from_env() -> Self {
        // `POINTER_E2E_PIXEL_BBOX=1` is the older, narrower spelling of the same
        // switch; keep honouring it so the commands recorded in the README keep
        // working.
        if std::env::var("POINTER_E2E_PIXEL_BBOX").as_deref() == Ok("1") {
            return BboxUnit::Pixel;
        }
        match std::env::var("POINTER_E2E_BBOX_UNIT")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "pixel" | "px" => BboxUnit::Pixel,
            _ => BboxUnit::Norm,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            BboxUnit::Norm => "norm",
            BboxUnit::Pixel => "pixel",
        }
    }
}

fn system_prompt_slices() -> Result<Vec<String>> {
    // Minimal arm: a deliberately tiny prompt, to separate "the model cannot
    // read coordinates" from "our prompt is confusing it". If the minimal arm
    // scores better, the prompt is the problem; if worse, the task needs the
    // scaffolding.
    if std::env::var("POINTER_E2E_MINIMAL_PROMPT").as_deref() == Ok("1") {
        return Ok(vec![match BboxUnit::from_env() {
            BboxUnit::Norm => "You control a macOS desktop through screenshots.\n\
             Given the screenshot and a goal, find the element the goal names and \
             report the box it occupies.\n\
             \n\
             Coordinates are 0-1000 normalized over the whole screen:\
             (0,0) is the top-left corner, (1000,1000) the bottom-right.\
             x scales with image width, y with image height.\n\
             \n\
             Call `mouse_click_at` with `target_desc` and `bbox`."
                .to_string(),
            BboxUnit::Pixel => "You control a macOS desktop through screenshots.\n\
                 Given the screenshot and a goal, find the element the goal names and \
                 report the box it occupies.\n\
                 \n\
                 Locate it on a 0-1000 normalized grid first: (0,0) is the top-left \
                 corner, (1000,1000) the bottom-right, x scales with image width, \
                 y with image height.\n\
                 Then report `bbox` in raw pixels of the screenshot: \
                 [x1, y1, x2, y2].\n\
                 \n\
                 Call `mouse_click_at` with `target_desc` and `bbox`."
                .to_string(),
        }]);
    }

    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/agents/computer");
    let mut out = Vec::new();

    // 1. COMMUNICATION_PUBLIC — same accessor the main loop uses.
    let public = pointer_core::agents::communication_public_md();
    if !public.trim().is_empty() {
        out.push(public.to_string());
    }

    // 2. Computer agent role prompts. `computer_agent_body()` strips AGENT.md's
    //    frontmatter.
    out.push(pointer_core::agents::computer_agent_body());
    // `computer_communication()` now only describes the single `[Current screen]`
    // slot the injector actually emits; the retired multi-frame names are gone,
    // so it no longer sends the model looking for frames that do not exist.
    out.push(pointer_core::agents::computer_communication());

    // 3. Tool appendix. Production concatenates every allowed tool's `.md`; the
    //    harness only exposes `mouse_click_at`, so only that doc applies. See
    //    the module header for why the other 30 are absent.
    let mouse_doc = std::fs::read_to_string(base.join("tools/prompts/mouse.md"))
        .with_context(|| "read mouse.md".to_string())?;
    out.push(mouse_doc);

    // 4. [Environment] — OS / locale / date. No screen size in it; the model
    //    learns the frame from the image itself.
    out.push(format!(
        "[Environment]\n{}",
        pointer_core::env_prompt::build_environment_system_prompt_slice()
    ));

    out.retain(|s| !s.trim().is_empty());
    Ok(out)
}

/// One production-shaped turn: `(frame, goal)` in, the model's submitted bbox out.
///
/// Returns the bbox in **frame pixels**. The model answers in 0–1000 (the same
/// unit production asks for), and the conversion here is the one production
/// performs before handing the box to `locate`.
pub struct AgentHint {
    /// Submitted box, frame pixels.
    pub box_px: [f64; 4],
    /// Submitted box, model's own 0–1000 units — kept for diagnostics, because
    /// unit confusion is the failure this benchmark exists to catch.
    pub box_norm: [f64; 4],
    /// Tool name the model chose (empty when it answered in text).
    pub tool: String,
    /// The model's reasoning channel, when the provider returned one.
    pub reasoning: String,
    pub usage_prompt_tokens: Option<u32>,
}

/// Optional conversation history taken from a production request dump.
///
/// `POINTER_E2E_HISTORY_DUMP=<path to logs/llm_prompts/<conv>/<file>.json>`
/// with `POINTER_E2E_HISTORY_LIMIT=N` prepends the first N rows of that dump
/// (system row excluded) as `base_messages`.
///
/// Why: production runs with ~680 rows of prior turns, and its hints are
/// accurate (median 18 px). The cold-start harness is not. Whether the gap is
/// "history gives the model a coordinate anchor" or "history is what makes the
/// thinking channel fire at all" is exactly what this switch is for.
fn history_from_dump() -> Result<Vec<ChatMessage>> {
    let Ok(path) = std::env::var("POINTER_E2E_HISTORY_DUMP") else {
        return Ok(Vec::new());
    };
    let limit: usize = std::env::var("POINTER_E2E_HISTORY_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let rows = v
        .get("messages")
        .and_then(|m| m.as_array())
        .context("dump has no messages[]")?;

    let mut out = Vec::new();
    for row in rows.iter().skip(1).take(limit) {
        let role = match row.get("role").and_then(|r| r.as_str()) {
            Some("user") => "user",
            Some("assistant") => "assistant",
            Some("tool") => "tool",
            _ => continue,
        };
        // Dump content is either a plain string or an array of typed parts.
        let content = match row.get("content") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        // `tool_calls` in the dump uses snake_case; `ChatMessage` renames it.
        let tool_calls = row.get("tool_calls").and_then(|tc| tc.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let f = c.get("function")?;
                    Some(serde_json::json!({
                        "id": c.get("id").and_then(|x| x.as_str()).unwrap_or("call"),
                        "name": f.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                        "arguments": f.get("arguments").and_then(|x| x.as_str()).unwrap_or("{}"),
                        "status": "done",
                    }))
                })
                .collect::<Vec<_>>()
        });

        let mut obj = serde_json::json!({
            "id": format!("hist-{}", out.len()),
            "role": role,
            "content": content,
            "status": "done",
            "createdAt": 0,
        });
        if let Some(tc) = tool_calls {
            if !tc.is_empty() {
                obj["toolCalls"] = serde_json::Value::Array(tc);
            }
        }
        if let Some(id) = row.get("tool_call_id").and_then(|x| x.as_str()) {
            obj["toolCallId"] = serde_json::Value::String(id.to_string());
        }
        out.push(serde_json::from_value(obj).context("history row -> ChatMessage")?);
    }
    eprintln!("[harness] history rows injected: {}", out.len());
    Ok(out)
}

/// One production-shaped turn, driven through the same wire builder and the
/// same streaming entry point the main loop uses.
///
/// `chat_once` also streams internally, but it hands the provider a single
/// string system message and bypasses `build_stream_chat_wire`. That path never
/// produced a reasoning channel in our runs (`reasoning_len=0` on every attempt,
/// while production dumps show `enable_thinking=true` with reasoning present).
/// Driving the real path removes the last unknown.
async fn hint_via_production_wire(
    provider: &OpenAIProvider,
    msgs: &[ChatMessage],
    system: &SystemPromptSections,
    tools: Vec<serde_json::Value>,
    dump_label: &str,
) -> Result<(String, Vec<pointer_core::models::ToolCall>, Option<String>, Option<u32>)> {
    use pointer_core::provider::ProviderEvent;
    let wire = provider.build_stream_chat_wire(
        &[],
        msgs,
        system,
        tools,
        Some(dump_label),
        pointer_core::message_context::LlmHistoryScope::Lead,
    )?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ProviderEvent>(256);
    let cancel = CancellationToken::new();

    // `join!` instead of `spawn`: the provider is borrowed, and neither future
    // needs to outlive this call.
    let send = provider.stream_chat_wired(wire, tx, cancel, Some(dump_label));
    let recv = async {
        let mut reasoning = String::new();
        let mut text = String::new();
        let mut calls = Vec::new();
        let mut usage = None;
        let mut served_model = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                ProviderEvent::ReasoningDelta(s) => reasoning.push_str(&s),
                ProviderEvent::ContentDelta(s) => text.push_str(&s),
                ProviderEvent::Finish {
                    tool_calls, usage: u, model, ..
                } => {
                    calls = tool_calls;
                    usage = u;
                    served_model = model;
                }
                _ => {}
            }
        }
        (text, calls, reasoning, usage, served_model)
    };
    let (sent, (text, calls, reasoning, usage, served_model)) = tokio::join!(send, recv);
    sent?;
    if !served_model.is_empty() {
        eprintln!("[model] served={served_model}");
    }
    Ok((
        text,
        calls,
        (!reasoning.is_empty()).then_some(reasoning),
        usage.map(|u| u.prompt_tokens),
    ))
}

pub async fn agent_hint(
    provider: &OpenAIProvider,
    frame_jpeg: &[u8],
    frame_px: (u32, u32),
    goal: &str,
) -> Result<AgentHint> {
    let (w, h) = (frame_px.0.max(1) as f64, frame_px.1.max(1) as f64);

    // Same capture shape production stores; only `screenshot_jpeg` is read by
    // the payload assembler.
    let cap = ScreenCaptureResult {
        raw_unmarked_jpeg: frame_jpeg.to_vec(),
        screenshot_jpeg: frame_jpeg.to_vec(),
        monitor: MonitorInfo::new(0, 0, w as i32, h as i32),
    };
    let (slot_labels, images_base64) = assemble_cur_screen_payload(&cap);
    // Production preamble now carries the screenshot's pixel dimensions (see
    // `build_cur_screen_preamble`), so the harness must not append its own copy:
    // two sources of truth is exactly the bug this change removes.
    let preamble = build_cur_screen_preamble(&cap);
    // The minimal arm has no `AGENT.md` behind it, so it states the contract
    // itself. The full arm relies on `AGENT.md` → **Locating targets**.
    let preamble = if std::env::var("POINTER_E2E_MINIMAL_PROMPT").as_deref() == Ok("1") {
        format!(
            "{preamble}\n\nDecide the position on a 0-1000 normalized grid first, then \
             convert for `bbox`: x_pixel = x_norm / 1000 × {w:.0}, \
             y_pixel = y_norm / 1000 × {h:.0}."
        )
    } else {
        preamble
    };
    // Guard against the silent failure this benchmark exists to catch: if the
    // image never reaches the model it will guess the screen centre, which
    // looks like a coordinate bug rather than a plumbing bug.
    let img_bytes: usize = images_base64.iter().map(|s| s.len()).sum();
    anyhow::ensure!(
        !images_base64.is_empty() && img_bytes > 10_000,
        "screenshot payload looks empty: {} slot(s), {} base64 chars",
        images_base64.len(),
        img_bytes
    );

    // The `[CUR_SCREEN]` message production injects, then the user's goal as its
    // own row — same split production uses (inject tail, then the real turn).
    // Verify what actually reaches the model. The pixel arm is only meaningful
    // if the dimensions are really in the prompt; a silent miss would look like
    // "the model ignores pixel coordinates" when it never saw the scale.
    eprintln!(
        "[preamble] unit={} frame={w:.0}x{h:.0} len={} tail={:?}",
        BboxUnit::from_env().as_str(),
        preamble.len(),
        preamble.chars().rev().take(320).collect::<String>().chars().rev().collect::<String>(),
    );

    let screen_msg: ChatMessage = serde_json::from_value(json!({
        "id": "e2e-screen",
        "role": "user",
        "content": preamble,
        "status": "complete",
        "createdAt": 0,
        "imagesBase64": images_base64,
        "imageSlotLabels": slot_labels,
    }))
    .context("build [CUR_SCREEN] message")?;

    let goal_msg: ChatMessage = serde_json::from_value(json!({
        "id": "e2e-goal",
        "role": "user",
        "content": goal,
        "status": "complete",
        "createdAt": 0,
    }))
    .context("build goal message")?;

    let slices = system_prompt_slices()?;
    eprintln!(
        "[system] {} block(s), total {} chars",
        slices.len(),
        slices.iter().map(|s| s.len()).sum::<usize>()
    );
    for (i, s) in slices.iter().enumerate() {
        let head: String = s.lines().next().unwrap_or("").chars().take(70).collect();
        eprintln!("[system]   [{i}] len={:6} {head}", s.len());
    }
    let system = SystemPromptSections {
        cacheable: slices,
        dynamic: Vec::new(),
    };

    let mut msgs = history_from_dump()?;
    msgs.push(screen_msg);
    msgs.push(goal_msg);

    let (text, calls, reasoning, prompt_tokens) = hint_via_production_wire(
        provider,
        &msgs,
        &system,
        vec![click_tool_schema()?],
        "computer_e2e_agent",
    )
    .await
    .context("stream_chat_wired")?;

    let call = calls.first().context("model returned no tool call")?;
    // Diagnostic: a thinking model that returns no reasoning is either not
    // thinking or thinking into a channel we do not parse. Both matter when
    // comparing two prompt arms, so surface the raw shape.
    eprintln!(
        "[harness] bbox_unit={} text_len={} reasoning_len={} tool_calls={} prompt_tokens={:?}",
        BboxUnit::from_env().as_str(),
        text.len(),
        reasoning.as_deref().map(str::len).unwrap_or(0),
        calls.len(),
        prompt_tokens,
    );
    let args: serde_json::Value = serde_json::from_str(&call.arguments)
        .context("tool call arguments are not JSON")?;
    let raw = args
        .get("bbox")
        .and_then(|v| v.as_array())
        .context("tool call has no bbox")?;
    anyhow::ensure!(raw.len() == 4, "bbox needs 4 numbers, got {}", raw.len());
    let mut b = [0.0f64; 4];
    for (i, v) in raw.iter().enumerate() {
        b[i] = v.as_f64().context("bbox entry is not a number")?;
    }

    // `BboxUnit::Pixel` asks the model for raw pixel coordinates instead of the
    // 0-1000 normalized contract production uses. Comparing the two tells us
    // whether the normalization step is itself a source of error: in normalized
    // mode the model must divide by the image width before answering, and the
    // traces show it getting that step wrong.
    let pixel_mode = BboxUnit::from_env() == BboxUnit::Pixel;
    let (box_px, box_norm) = if pixel_mode {
        // Model already answered in pixels; only derive the normalized view for
        // the shared scoring path.
        (
            b,
            [b[0] / w * 1000.0, b[1] / h * 1000.0, b[2] / w * 1000.0, b[3] / h * 1000.0],
        )
    } else {
        // 0–1000 → frame pixels, exactly as production converts before locate.
        (
            [
                b[0] / 1000.0 * w,
                b[1] / 1000.0 * h,
                b[2] / 1000.0 * w,
                b[3] / 1000.0 * h,
            ],
            b,
        )
    };

    Ok(AgentHint {
        box_px,
        box_norm,
        tool: call.name.clone(),
        reasoning: reasoning.unwrap_or_default(),
        usage_prompt_tokens: prompt_tokens,
    })
}

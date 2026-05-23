//! Computer agent tier runtime: Primary / Intermediate / Advanced profiles.

use crate::agents::computer::vision::coord::{screen_to_normalized, CoordinateSystem};
use crate::agents::computer::vision::vision_state::{CornerAnchor, VisionState};
use crate::agents::AgentRegistry;
use serde_json::Value;
use std::collections::HashMap;

pub const CONFIG_KEY_AUTO_UPGRADE: &str = "computerAutoUpgrade";
pub const CONFIG_KEY_INITIAL_TIER: &str = "computerInitialTier";
pub const CONFIG_KEY_MODEL_PRIMARY: &str = "computerModelPrimary";
pub const CONFIG_KEY_MODEL_ADVANCED: &str = "computerModelAdvanced";

pub const DEFAULT_MODEL_PRIMARY: &str = "qwen3.5-plus";
pub const DEFAULT_MODEL_ADVANCED: &str = "qwen3.6-plus";
/// Qwen `thinking_budget` for Primary / Intermediate (`qwen3.5-plus`).
pub const PRIMARY_INTERMEDIATE_THINKING_BUDGET: u32 = 2048;
pub const ADVANCED_THINKING_BUDGET: u32 = 8192;

const MAX_TIER_HISTORY: usize = 10;
/// Consecutive **`Step result: fail`** before auto-upgrade (`>` this value → bump tier).
pub const TIER_ERROR_THRESHOLD: u32 = 3;
/// Same-**goal** **`verify: fail`** or **`verify: pending`** rows before **Repetition** **`STUCK: yes`** (`>` this value).
pub const REPETITION_STUCK_COUNT_THRESHOLD: u32 = 3;

/// Back-compat alias for [`REPETITION_STUCK_COUNT_THRESHOLD`].
pub const REPETITION_FAIL_COUNT_STUCK_THRESHOLD: u32 = REPETITION_STUCK_COUNT_THRESHOLD;
/// Same-goal verify fails before **`[LOCKED GOAL]`** engages (`>` this value).
pub const TASK_ERROR_THRESHOLD: u32 = 3;

/// Vision / reasoning profile for one conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComputerTier {
    Primary,
    Intermediate,
    Advanced,
}

impl ComputerTier {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "intermediate" | "mid" | "medium" => Self::Intermediate,
            "advanced" | "high" => Self::Advanced,
            _ => Self::Primary,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Intermediate => "intermediate",
            Self::Advanced => "advanced",
        }
    }

    pub fn bump(self) -> Self {
        match self {
            Self::Primary => Self::Intermediate,
            Self::Intermediate => Self::Advanced,
            Self::Advanced => Self::Advanced,
        }
    }

    pub fn includes_cause_in_history(self) -> bool {
        !matches!(self, ComputerTier::Primary)
    }
}

/// Static tier options from agent manifest.
#[derive(Debug, Clone)]
pub struct ComputerTierConfig {
    pub auto_upgrade: bool,
    pub initial_tier: ComputerTier,
    pub model_primary: String,
    pub model_advanced: String,
}

impl Default for ComputerTierConfig {
    fn default() -> Self {
        Self {
            auto_upgrade: true,
            initial_tier: ComputerTier::Primary,
            model_primary: DEFAULT_MODEL_PRIMARY.into(),
            model_advanced: DEFAULT_MODEL_ADVANCED.into(),
        }
    }
}

impl ComputerTierConfig {
    pub fn from_agent_registry(agents: &AgentRegistry) -> Self {
        let mut cfg = Self::default();
        let Some(def) = agents.get("computer").map(|a| a.def()) else {
            return cfg;
        };
        for (k, v) in &def.config {
            match k.as_str() {
                CONFIG_KEY_AUTO_UPGRADE => {
                    cfg.auto_upgrade = v.eq_ignore_ascii_case("true") || v == "1";
                }
                CONFIG_KEY_INITIAL_TIER => {
                    cfg.initial_tier = ComputerTier::parse(v);
                }
                CONFIG_KEY_MODEL_PRIMARY if !v.trim().is_empty() => {
                    cfg.model_primary = v.trim().to_string();
                }
                CONFIG_KEY_MODEL_ADVANCED if !v.trim().is_empty() => {
                    cfg.model_advanced = v.trim().to_string();
                }
                _ => {}
            }
        }
        cfg
    }

    /// App settings override agent manifest when `computerInitialTier` is non-empty.
    pub fn apply_app_settings(&mut self, settings: &crate::models::ModelSettings) {
        let tier = settings.computer_initial_tier.trim();
        if !tier.is_empty() {
            self.initial_tier = ComputerTier::parse(tier);
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub step_result: String,
    pub cause: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TierActionRecord {
    pub tool_name: String,
    pub goal: String,
    pub action: Option<String>,
    pub coords: Option<(i32, i32)>,
    pub verify_result: Option<VerifyOutcome>,
    pub extra_args_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LockedGoal {
    pub fingerprint: String,
    pub label: String,
}

/// Per-conversation tier state (histories isolated per tier).
#[derive(Debug, Clone)]
pub struct ComputerTierRuntime {
    pub current_tier: ComputerTier,
    pub tier_error_streak: u32,
    pub task_error_streak: u32,
    pub locked_goal: Option<LockedGoal>,
    histories: HashMap<ComputerTier, Vec<TierActionRecord>>,
    /// Goal of the last desktop tool executed (for lock counting).
    pub last_executed_goal: Option<String>,
    /// Fail streak for the current goal before lock engages.
    goal_fail_fingerprint: Option<String>,
    goal_fail_streak: u32,
}

impl ComputerTierRuntime {
    pub fn new(initial: ComputerTier) -> Self {
        Self {
            current_tier: initial,
            tier_error_streak: 0,
            task_error_streak: 0,
            locked_goal: None,
            histories: HashMap::new(),
            last_executed_goal: None,
            goal_fail_fingerprint: None,
            goal_fail_streak: 0,
        }
    }

    pub fn history_for(&self, tier: ComputerTier) -> &[TierActionRecord] {
        self.histories.get(&tier).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn push_action(&mut self, tier: ComputerTier, record: TierActionRecord) {
        let v = self.histories.entry(tier).or_default();
        v.push(record);
        if v.len() > MAX_TIER_HISTORY {
            v.remove(0);
        }
    }

    pub fn backfill_last_verify(&mut self, tier: ComputerTier, outcome: VerifyOutcome) {
        let Some(v) = self.histories.get_mut(&tier) else {
            return;
        };
        if let Some(last) = v.last_mut() {
            last.verify_result = Some(outcome);
        }
    }

    /// Normalize `tool_args.goal` for fingerprinting.
    pub fn fingerprint_goal(goal: &str) -> String {
        let normalized = normalize_goal_text(goal);
        stable_hash_hex16(&normalized)
    }

    pub fn locked_goal_label(&self) -> Option<&str> {
        self.locked_goal.as_ref().map(|l| l.label.as_str())
    }

    pub fn locked_goal_dynamic_block(&self) -> Option<String> {
        let lock = self.locked_goal.as_ref()?;
        Some(format!(
            "[LOCKED GOAL]\nYou must complete this goal before starting any other goal:\n\"{}\"\nEvery desktop tool this turn must use the same goal string until Verify reports pass for that goal.",
            lock.label.replace('"', "\\\"")
        ))
    }

    pub fn on_round_complete(
        &mut self,
        config: &ComputerTierConfig,
        parsed: Option<&ParsedVerify>,
        last_tool_goal: Option<&str>,
    ) {
        let Some(pv) = parsed else {
            return;
        };
        let step = pv.step_result.as_str();
        let is_fail = step == "fail";
        let is_pass = step == "pass";

        if is_fail {
            self.tier_error_streak = self.tier_error_streak.saturating_add(1);
            if let Some(goal) = last_tool_goal.map(str::trim).filter(|s| !s.is_empty()) {
                let fp = Self::fingerprint_goal(goal);
                if self.locked_goal.as_ref().is_some_and(|l| l.fingerprint == fp) {
                    self.task_error_streak = self.task_error_streak.saturating_add(1);
                } else if self.locked_goal.is_none() {
                    if self.goal_fail_fingerprint.as_deref() == Some(fp.as_str()) {
                        self.goal_fail_streak = self.goal_fail_streak.saturating_add(1);
                    } else {
                        self.goal_fail_fingerprint = Some(fp.clone());
                        self.goal_fail_streak = 1;
                    }
                    if self.goal_fail_streak > TASK_ERROR_THRESHOLD {
                        log::info!("computer tier: lock goal \"{}\" (fp={fp})", goal);
                        self.locked_goal = Some(LockedGoal {
                            fingerprint: fp,
                            label: goal.to_string(),
                        });
                        self.task_error_streak = self.goal_fail_streak;
                    }
                }
            }
        } else if is_pass {
            self.tier_error_streak = 0;
            let unlock = self.locked_goal.is_none()
                || goal_matches_lock(last_tool_goal, self.locked_goal.as_ref());
            if unlock {
                self.locked_goal = None;
                self.task_error_streak = 0;
                self.goal_fail_fingerprint = None;
                self.goal_fail_streak = 0;
                self.current_tier = ComputerTier::Primary;
                log::info!(
                    "computer tier: goal verify pass — reset to {:?}",
                    self.current_tier
                );
            }
        }

        if !is_pass {
            if self.tier_error_streak > TIER_ERROR_THRESHOLD && config.auto_upgrade {
                let prev = self.current_tier;
                self.current_tier = prev.bump();
                self.tier_error_streak = 0;
                log::info!("computer tier: auto_upgrade {:?} -> {:?}", prev, self.current_tier);
            }
        }
    }
}

pub fn goal_matches_lock(goal: Option<&str>, lock: Option<&LockedGoal>) -> bool {
    let Some(g) = goal.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(lock) = lock else {
        return false;
    };
    ComputerTierRuntime::fingerprint_goal(g) == lock.fingerprint
}

fn normalize_goal_text(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = false;
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_space = false;
        } else if c.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        }
    }
    out.trim().to_string()
}

fn stable_hash_hex16(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn verify_counts_toward_repetition_stuck(step: &str) -> bool {
    step == "fail" || step == "pending"
}

/// Count **`verify: fail`** and **`verify: pending`** rows for the **same `goal`** as the newest history row.
pub fn same_goal_repetition_count_in_history(records: &[TierActionRecord]) -> u32 {
    let Some(last) = records.last() else {
        return 0;
    };
    let goal = last.goal.as_str();
    records
        .iter()
        .filter(|r| r.goal == goal)
        .filter(|r| {
            r.verify_result
                .as_ref()
                .is_some_and(|v| verify_counts_toward_repetition_stuck(v.step_result.as_str()))
        })
        .count() as u32
}

/// Back-compat alias for [`same_goal_repetition_count_in_history`].
pub fn same_goal_fail_count_in_history(records: &[TierActionRecord]) -> u32 {
    same_goal_repetition_count_in_history(records)
}

/// Host-maintained counters for **`[Computer tier runtime]`** under **`[CUR_SCREEN]`**.
pub fn format_tier_runtime_block(
    rt: &ComputerTierRuntime,
    config: &ComputerTierConfig,
    tier: ComputerTier,
    records: &[TierActionRecord],
) -> String {
    let mut lines = vec![
        "[Computer tier runtime — host-maintained; not screenshots.]".to_string(),
        format!("Current tier: {}", tier.label()),
    ];
    if config.auto_upgrade && tier != ComputerTier::Advanced {
        let next = tier.bump().label();
        lines.push(format!(
            "Verify-fail streak: {} (auto-upgrade after >{} consecutive verify fail → {next})",
            rt.tier_error_streak, TIER_ERROR_THRESHOLD
        ));
    } else if config.auto_upgrade {
        lines.push(format!(
            "Verify-fail streak: {} (already advanced; no further tier bump)",
            rt.tier_error_streak
        ));
    } else {
        lines.push(format!(
            "Verify-fail streak: {} (computerAutoUpgrade=false — tier unchanged)",
            rt.tier_error_streak
        ));
    }
    let rep_count = same_goal_repetition_count_in_history(records);
    if records.is_empty() {
        lines.push(
            "Repetition count: none — no prior desktop tool rows in this tier history.".to_string(),
        );
    } else if let Some(last) = records.last() {
        let stuck = rep_count > REPETITION_STUCK_COUNT_THRESHOLD;
        let stuck_label = if stuck { "yes" } else { "no" };
        lines.push(format!(
            "Repetition count: {rep_count} verify fail/pending for goal=\"{}\" (>{REPETITION_STUCK_COUNT_THRESHOLD} → STUCK: yes) — STUCK: {stuck_label}",
            escape_goal(&last.goal)
        ));
    }
    if let Some(lock) = rt.locked_goal.as_ref() {
        lines.push(format!(
            "Locked goal active: \"{}\" (task-fail streak {} on this goal)",
            lock.label.replace('"', "\\\""),
            rt.task_error_streak
        ));
    } else if rt.goal_fail_streak > 0 {
        lines.push(format!(
            "Goal-fail streak: {} (>{} locks goal before other goals)",
            rt.goal_fail_streak, TASK_ERROR_THRESHOLD
        ));
    }
    lines.push(
        "On verify pass for the active goal: tier resets to primary and streaks clear.".to_string(),
    );
    lines.join("\n")
}

/// Build a tier history block for `[CUR_SCREEN]`.
pub fn format_tier_history_block(tier: ComputerTier, records: &[TierActionRecord]) -> Option<String> {
    if records.is_empty() {
        return None;
    }
    let mut lines = vec![
        "[Recent desktop tool calls — ordered oldest to newest; repetition uses goal; coordinates are session 0-1000; overlay indices are not comparable across turns.]".to_string(),
    ];
    for (i, r) in records.iter().enumerate() {
        lines.push(format!("  {}: {}", i + 1, format_history_line(tier, r)));
    }
    Some(lines.join("\n"))
}

fn format_history_line(tier: ComputerTier, r: &TierActionRecord) -> String {
    let mut base = format!("{} goal=\"{}\"", r.tool_name, escape_goal(&r.goal));
    if let Some(ref a) = r.action {
        if !a.is_empty() {
            base.push_str(&format!(" action=\"{}\"", escape_goal(a)));
        }
    }
    if let Some((x, y)) = r.coords {
        base.push_str(&format!(" at ({x}, {y})"));
    } else if let Some(ref hint) = r.extra_args_hint {
        if !hint.is_empty() {
            base.push_str(&format!(" {hint}"));
        }
    }
    base.push_str(" | ");
    base.push_str(&format_verify_suffix(tier, r.verify_result.as_ref()));
    base
}

fn escape_goal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_verify_suffix(tier: ComputerTier, v: Option<&VerifyOutcome>) -> String {
    let Some(v) = v else {
        return "verify: —".into();
    };
    let step = v.step_result.as_str();
    if tier.includes_cause_in_history() {
        if let Some(ref c) = v.cause {
            if !c.is_empty() && step != "pass" {
                return format!("verify: {step} ({c})");
            }
        }
    }
    format!("verify: {step}")
}

/// Parsed verify block from assistant `thoughts`.
#[derive(Debug, Clone)]
pub struct ParsedVerify {
    pub step_result: String,
    pub cause: Option<String>,
}

/// Extract `Step result:` and optional `Cause:` from thoughts text.
pub fn parse_verify_from_thoughts(thoughts: &str) -> Option<ParsedVerify> {
    let step = extract_field_line(thoughts, "Step result:")?;
    let step_result = step
        .split_whitespace()
        .next()
        .unwrap_or("n/a")
        .trim_matches(|c: char| c == '.' || c == ';')
        .to_string();
    if step_result.is_empty() {
        return None;
    }
    let cause = extract_field_line(thoughts, "Cause:")
        .map(|s| s.trim().trim_matches(|c: char| c == '.' || c == ';').to_string())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("—") && !s.eq_ignore_ascii_case("-"));
    Some(ParsedVerify {
        step_result,
        cause,
    })
}

fn extract_field_line(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix(key) {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Build a [`TierActionRecord`] from tool invocation (session-normalized coords).
pub fn normalize_tool_record(
    tool_name: &str,
    args: &Value,
    vision: &VisionState,
) -> Option<TierActionRecord> {
    let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or("").trim();
    if goal.is_empty() {
        log::warn!("computer tier history: skip record — missing goal on {tool_name}");
        return None;
    }
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let coords = extract_normalized_coords(args, vision);
    let extra_args_hint = if coords.is_none() {
        Some(compact_non_coord_args(args))
    } else {
        None
    };
    Some(TierActionRecord {
        tool_name: tool_name.to_string(),
        goal: goal.to_string(),
        action,
        coords,
        verify_result: None,
        extra_args_hint,
    })
}

fn extract_normalized_coords(args: &Value, vision: &VisionState) -> Option<(i32, i32)> {
    if let (Some(x), Some(y)) = (
        args.get("x").and_then(|v| v.as_f64()),
        args.get("y").and_then(|v| v.as_f64()),
    ) {
        return normalized_pair(vision, x as f32, y as f32);
    }
    let index = args.get("index").and_then(|v| v.as_u64()).map(|n| n as u32)?;
    let anchor = args
        .get("anchor")
        .and_then(|v| v.as_str())
        .and_then(CornerAnchor::parse);
    let dx = args.get("dx").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let dy = args.get("dy").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let (px, py) = if let Some(a) = anchor {
        vision.resolve_index_anchor(index, a, dx, dy)?
    } else {
        vision.resolve_index(index)?
    };
    let monitor = vision.screen_bbox()?;
    let (nx, ny) = screen_to_normalized((px, py), &monitor, CoordinateSystem::Qwen);
    Some((nx.round() as i32, ny.round() as i32))
}

fn normalized_pair(_vision: &VisionState, x: f32, y: f32) -> Option<(i32, i32)> {
    Some((x.round() as i32, y.round() as i32))
}

fn compact_non_coord_args(args: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(keys) = args.get("keys").and_then(|v| v.as_str()) {
        parts.push(format!("keys=\"{keys}\""));
    }
    if let Some(sec) = args.get("seconds").and_then(|v| v.as_f64()) {
        parts.push(format!("seconds={sec}"));
    }
    if let Some(text) = args.get("text").and_then(|v| v.as_str()) {
        let t = if text.chars().count() > 40 {
            format!("{}…", text.chars().take(40).collect::<String>())
        } else {
            text.to_string()
        };
        parts.push(format!("text=\"{t}\""));
    }
    if parts.is_empty() {
        if let Some(obj) = args.as_object() {
            let keys: Vec<_> = obj
                .keys()
                .filter(|k| *k != "goal" && *k != "action" && *k != "method" && *k != "human_like")
                .take(4)
                .collect();
            if !keys.is_empty() {
                return format!("({})", keys.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","));
            }
        }
    }
    parts.join(" ")
}

/// Per-round LLM overrides for computer tier.
#[derive(Debug, Clone)]
pub struct ComputerRoundLlmOverrides {
    pub model: String,
    pub enable_thinking: bool,
    pub thinking_budget: Option<u32>,
}

impl ComputerRoundLlmOverrides {
    pub fn for_tier(tier: ComputerTier, config: &ComputerTierConfig) -> Self {
        match tier {
            ComputerTier::Primary | ComputerTier::Intermediate => Self {
                model: config.model_primary.clone(),
                enable_thinking: true,
                thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
            },
            ComputerTier::Advanced => Self {
                model: config.model_advanced.clone(),
                enable_thinking: true,
                thinking_budget: Some(ADVANCED_THINKING_BUDGET),
            },
        }
    }
}

// Thread-local active tier for tool handlers.
std::thread_local! {
    static ACTIVE_COMPUTER_TIER: std::cell::RefCell<Option<ComputerTier>> =
        const { std::cell::RefCell::new(None) };
}

pub struct ComputerTierGuard {
    previous: Option<ComputerTier>,
}

impl ComputerTierGuard {
    pub fn enter(tier: ComputerTier) -> Self {
        let previous = ACTIVE_COMPUTER_TIER.with(|c| {
            let mut g = c.borrow_mut();
            std::mem::replace(&mut *g, Some(tier))
        });
        Self { previous }
    }
}

impl Drop for ComputerTierGuard {
    fn drop(&mut self) {
        ACTIVE_COMPUTER_TIER.with(|c| {
            *c.borrow_mut() = self.previous;
        });
    }
}

pub fn current_computer_tier() -> Option<ComputerTier> {
    ACTIVE_COMPUTER_TIER.with(|c| *c.borrow())
}

pub fn tier_allows_index_tools(tier: ComputerTier) -> bool {
    use crate::agents::computer::tools::tool_prompts::{positioning_mode_for_tier, ComputerPositioningMode};
    positioning_mode_for_tier(tier) == ComputerPositioningMode::Index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_normalizes_case_and_space() {
        let a = ComputerTierRuntime::fingerprint_goal("Open  Settings");
        let b = ComputerTierRuntime::fingerprint_goal("open settings");
        assert_eq!(a, b);
    }

    #[test]
    fn parse_verify_extracts_step_and_cause() {
        let t = "Verify:\nStep result: fail\nCause: precision_miss\n";
        let p = parse_verify_from_thoughts(t).unwrap();
        assert_eq!(p.step_result, "fail");
        assert_eq!(p.cause.as_deref(), Some("precision_miss"));
    }

    #[test]
    fn format_history_includes_goal_and_coords() {
        let r = TierActionRecord {
            tool_name: "mouse:click_at".into(),
            goal: "Open Settings".into(),
            action: Some("click".into()),
            coords: Some((412, 680)),
            verify_result: Some(VerifyOutcome {
                step_result: "fail".into(),
                cause: Some("precision_miss".into()),
            }),
            extra_args_hint: None,
        };
        let line = format_history_line(ComputerTier::Intermediate, &r);
        assert!(line.contains("goal=\"Open Settings\""));
        assert!(line.contains("at (412, 680)"));
        assert!(line.contains("verify: fail (precision_miss)"));
    }

    #[test]
    fn tier_bumps_after_four_fails_when_auto_upgrade() {
        let config = ComputerTierConfig {
            auto_upgrade: true,
            initial_tier: ComputerTier::Primary,
            ..ComputerTierConfig::default()
        };
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        for _ in 0..4 {
            rt.on_round_complete(
                &config,
                Some(&ParsedVerify {
                    step_result: "fail".into(),
                    cause: Some("precision_miss".into()),
                }),
                Some("open settings"),
            );
        }
        assert_eq!(rt.current_tier, ComputerTier::Intermediate);
    }

    #[test]
    fn primary_history_omits_cause() {
        let r = TierActionRecord {
            tool_name: "mouse:click_at".into(),
            goal: "g".into(),
            action: None,
            coords: None,
            verify_result: Some(VerifyOutcome {
                step_result: "fail".into(),
                cause: Some("precision_miss".into()),
            }),
            extra_args_hint: None,
        };
        let line = format_history_line(ComputerTier::Primary, &r);
        assert!(line.contains("verify: fail"));
        assert!(!line.contains("precision_miss"));
    }

    #[test]
    fn same_goal_repetition_count_sums_fail_and_pending_rows() {
        let mk = |goal: &str, step: &str| TierActionRecord {
            tool_name: "mouse:click_index".into(),
            goal: goal.into(),
            action: Some("click".into()),
            coords: None,
            verify_result: Some(VerifyOutcome {
                step_result: step.into(),
                cause: None,
            }),
            extra_args_hint: None,
        };
        let records = vec![
            mk("open settings", "pass"),
            mk("open settings", "fail"),
            mk("open settings", "pending"),
        ];
        assert_eq!(same_goal_repetition_count_in_history(&records), 2);
        let switched = vec![
            mk("open settings", "fail"),
            mk("open settings", "pending"),
            mk("other", "fail"),
        ];
        assert_eq!(same_goal_repetition_count_in_history(&switched), 1);
    }

    #[test]
    fn tier_runtime_block_shows_stuck_and_upgrade_hint() {
        let config = ComputerTierConfig::default();
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        rt.tier_error_streak = 2;
        let records = vec![TierActionRecord {
            tool_name: "t".into(),
            goal: "g".into(),
            action: Some("a".into()),
            coords: None,
            verify_result: None,
            extra_args_hint: None,
        }];
        let block = format_tier_runtime_block(&rt, &config, ComputerTier::Primary, &records);
        assert!(block.contains("Verify-fail streak: 2"));
        assert!(block.contains("intermediate"));
        assert!(block.contains("Repetition count: 0"));
        assert!(block.contains("STUCK: no"));
    }

    #[test]
    fn tier_runtime_block_stuck_when_four_same_goal_fails() {
        let config = ComputerTierConfig::default();
        let rt = ComputerTierRuntime::new(ComputerTier::Primary);
        let records: Vec<_> = (0..4)
            .map(|_| TierActionRecord {
                tool_name: "t".into(),
                goal: "same".into(),
                action: Some("click".into()),
                coords: None,
                verify_result: Some(VerifyOutcome {
                    step_result: "fail".into(),
                    cause: None,
                }),
                extra_args_hint: None,
            })
            .collect();
        let block = format_tier_runtime_block(&rt, &config, ComputerTier::Primary, &records);
        assert!(block.contains("Repetition count: 4"));
        assert!(block.contains("STUCK: yes"));
    }

    #[test]
    fn primary_intermediate_thinking_budget() {
        let cfg = ComputerTierConfig::default();
        let o = ComputerRoundLlmOverrides::for_tier(ComputerTier::Primary, &cfg);
        assert!(o.enable_thinking);
        assert_eq!(
            o.thinking_budget,
            Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET)
        );
        let o2 = ComputerRoundLlmOverrides::for_tier(ComputerTier::Intermediate, &cfg);
        assert_eq!(o2.thinking_budget, Some(2048));
    }

    #[test]
    fn apply_app_settings_overrides_initial_tier() {
        let mut cfg = ComputerTierConfig::default();
        assert_eq!(cfg.initial_tier, ComputerTier::Primary);
        let settings = crate::models::ModelSettings {
            computer_initial_tier: "advanced".into(),
            ..crate::models::ModelSettings::default()
        };
        cfg.apply_app_settings(&settings);
        assert_eq!(cfg.initial_tier, ComputerTier::Advanced);
    }

    #[test]
    fn locked_goal_label_returns_goal_text_not_footer() {
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        rt.locked_goal = Some(LockedGoal {
            fingerprint: "abc".into(),
            label: "Open WeChat".into(),
        });
        assert_eq!(rt.locked_goal_label(), Some("Open WeChat"));
        let block = rt.locked_goal_dynamic_block().unwrap();
        assert!(block.contains("Open WeChat"));
        assert_ne!(rt.locked_goal_label(), block.lines().nth(3));
    }

    #[test]
    fn advanced_tier_disallows_index_tools() {
        assert!(tier_allows_index_tools(ComputerTier::Primary));
        assert!(tier_allows_index_tools(ComputerTier::Intermediate));
        assert!(!tier_allows_index_tools(ComputerTier::Advanced));
    }
}

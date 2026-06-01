//! Computer agent tier runtime: Primary / Intermediate / Advanced profiles.

use crate::agents::computer::vision::coord::{screen_to_normalized, CoordinateSystem};
use crate::agents::computer::vision::vision_state::VisionState;
use crate::models::ComputerTierLlmConfig;
use crate::models::ToolCall;
use crate::agents::AgentRegistry;
use serde_json::Value;
use std::collections::HashMap;

pub const CONFIG_KEY_AUTO_UPGRADE: &str = "computerAutoUpgrade";
pub const CONFIG_KEY_INITIAL_TIER: &str = "computerInitialTier";
pub const CONFIG_KEY_MODEL_PRIMARY: &str = "computerModelPrimary";
pub const CONFIG_KEY_MODEL_INTERMEDIATE: &str = "computerModelIntermediate";
pub const CONFIG_KEY_MODEL_ADVANCED: &str = "computerModelAdvanced";

pub const DEFAULT_MODEL_PRIMARY: &str = "qwen3.5-flash";
pub const DEFAULT_MODEL_INTERMEDIATE: &str = "qwen3.5-plus";
pub const DEFAULT_MODEL_ADVANCED: &str = "qwen3.6-plus";
/// Qwen `thinking_budget` for Primary / Intermediate (`qwen3.5-plus`).
pub const PRIMARY_INTERMEDIATE_THINKING_BUDGET: u32 = 2048;
pub const ADVANCED_THINKING_BUDGET: u32 = 8192;

const MAX_TIER_HISTORY: usize = 10;

/// Triggers tier auto-upgrade (Primary → Intermediate → Advanced) when
/// the model-reported `repetition_count` or consecutive verify-fail streak
/// exceeds this value.
pub const TIER_UPGRADE_THRESHOLD: u32 = 3;

/// Back-compat alias for [`TIER_UPGRADE_THRESHOLD`].
pub const REPETITION_STUCK_COUNT_THRESHOLD: u32 = TIER_UPGRADE_THRESHOLD;
/// Back-compat alias for [`TIER_UPGRADE_THRESHOLD`].
pub const REPETITION_FAIL_COUNT_STUCK_THRESHOLD: u32 = TIER_UPGRADE_THRESHOLD;
/// Back-compat alias for [`TIER_UPGRADE_THRESHOLD`].
pub const TIER_ERROR_THRESHOLD: u32 = TIER_UPGRADE_THRESHOLD;

/// Same-goal verify fails before **`[LOCKED GOAL]`** engages (`>` this value).
pub const GOAL_LOCK_THRESHOLD: u32 = 3;
/// Back-compat alias for [`GOAL_LOCK_THRESHOLD`].
pub const TASK_ERROR_THRESHOLD: u32 = GOAL_LOCK_THRESHOLD;

/// When the model-reported `repetition_count` reaches this value,
/// the conversation loop exits with a "cannot complete" message
/// requesting user guidance.
pub const GIVE_UP_THRESHOLD: u32 = 5;

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

/// Static tier options from agent manifest + platform overrides.
#[derive(Debug, Clone)]
pub struct ComputerTierConfig {
    pub auto_upgrade: bool,
    pub initial_tier: ComputerTier,
    pub model_primary: String,
    pub model_intermediate: String,
    pub model_advanced: String,
    pub tier_llm: HashMap<String, ComputerTierLlmConfig>,
}

impl Default for ComputerTierConfig {
    fn default() -> Self {
        Self {
            auto_upgrade: true,
            initial_tier: ComputerTier::Intermediate,
            model_primary: DEFAULT_MODEL_PRIMARY.into(),
            model_intermediate: DEFAULT_MODEL_INTERMEDIATE.into(),
            model_advanced: DEFAULT_MODEL_ADVANCED.into(),
            tier_llm: HashMap::new(),
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
                CONFIG_KEY_MODEL_INTERMEDIATE if !v.trim().is_empty() => {
                    cfg.model_intermediate = v.trim().to_string();
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
        if settings.computer_human_like {
            // human_like is read from platform settings at tool layer; no field here.
        }
    }

    /// Override tier LLM profiles from platform `computerTierLlm`.
    pub fn apply_platform_tier_llm(&mut self, m: &HashMap<String, ComputerTierLlmConfig>) {
        if m.is_empty() {
            return;
        }
        self.tier_llm = m.clone();
        if let Some(p) = m.get("primary") {
            if !p.model.trim().is_empty() {
                self.model_primary = p.model.clone();
            }
        }
        if let Some(i) = m.get("intermediate") {
            if !i.model.trim().is_empty() {
                self.model_intermediate = i.model.clone();
            }
        }
        if let Some(a) = m.get("advanced") {
            if !a.model.trim().is_empty() {
                self.model_advanced = a.model.clone();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub step_result: String,
    pub cause: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedTierSignal {
    pub action_result: String,
    pub repetition_count: u32,
    pub failure_cause: Option<String>,
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
    /// Set to true when repetition_count reaches GIVE_UP_THRESHOLD;
    /// the outer loop reads this to exit with a user-guidance request.
    pub should_give_up: bool,
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
            should_give_up: false,
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

    fn effective_repetition_count(
        sidecar_repetition_count: Option<u32>,
        host_repetition_count: u32,
    ) -> u32 {
        match sidecar_repetition_count {
            Some(sidecar) => sidecar.max(host_repetition_count),
            None => host_repetition_count,
        }
    }

    pub fn on_round_complete(
        &mut self,
        config: &ComputerTierConfig,
        parsed: Option<&ParsedVerify>,
        last_tool_goal: Option<&str>,
        sidecar_repetition_count: Option<u32>,
        host_repetition_count: u32,
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
                    if self.goal_fail_streak > GOAL_LOCK_THRESHOLD {
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
            self.should_give_up = false;
            let unlock = self.locked_goal.is_none()
                || goal_matches_lock(last_tool_goal, self.locked_goal.as_ref());
            if unlock {
                self.locked_goal = None;
                self.task_error_streak = 0;
                self.goal_fail_fingerprint = None;
                self.goal_fail_streak = 0;
                self.current_tier = config.initial_tier;
                log::info!(
                    "computer tier: goal verify pass — reset to {:?}",
                    self.current_tier
                );
            }
        }

        if !is_pass {
            let effective_rep = Self::effective_repetition_count(
                sidecar_repetition_count,
                host_repetition_count,
            );
            let should_upgrade = if effective_rep > 0 {
                effective_rep > TIER_UPGRADE_THRESHOLD
            } else {
                self.tier_error_streak > TIER_UPGRADE_THRESHOLD
            };
            if should_upgrade && config.auto_upgrade {
                let prev = self.current_tier;
                self.current_tier = prev.bump();
                self.tier_error_streak = 0;
                log::info!(
                    "computer tier: auto_upgrade {:?} -> {:?} (sidecar={:?} host={host_repetition_count} effective={effective_rep})",
                    prev,
                    self.current_tier,
                    sidecar_repetition_count,
                );
            }

            if effective_rep >= GIVE_UP_THRESHOLD {
                log::info!(
                    "computer tier: give up — effective repetition={effective_rep} >= {GIVE_UP_THRESHOLD} (sidecar={:?} host={host_repetition_count})",
                    sidecar_repetition_count,
                );
                self.should_give_up = true;
            }
        }
    }

    pub fn reset_give_up_for_new_turn(&mut self) {
        self.should_give_up = false;
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

fn verify_counts_toward_repetition_count(step: &str) -> bool {
    step == "fail"
}

/// Count verify-fail rows for the same goal as the newest history row.
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
                .is_some_and(|v| verify_counts_toward_repetition_count(v.step_result.as_str()))
        })
        .count() as u32
}

/// Back-compat alias for [`same_goal_repetition_count_in_history`].
pub fn same_goal_fail_count_in_history(records: &[TierActionRecord]) -> u32 {
    same_goal_repetition_count_in_history(records)
}

/// Host-maintained counters for internal runtime diagnostics.
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
            rt.tier_error_streak, TIER_UPGRADE_THRESHOLD
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
        lines.push(format!(
            "Repetition count: {rep_count} verify fail for goal=\"{}\"",
            escape_goal(&last.goal)
        ));
        let rep_policy = if rep_count == 0 {
            "repeat-policy: count=0 continue current tactic"
        } else if rep_count <= TIER_UPGRADE_THRESHOLD {
            "repeat-policy: count=1..3 switch tactic/method"
        } else if rep_count < GIVE_UP_THRESHOLD {
            "repeat-policy: count=4 escalate tier/reasoning"
        } else {
            "repeat-policy: count>=5 exhausted — request user guidance"
        };
        lines.push(rep_policy.to_string());
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
            rt.goal_fail_streak, GOAL_LOCK_THRESHOLD
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
        return "action_result: —".into();
    };
    let step = v.step_result.as_str();
    if tier.includes_cause_in_history() {
        if let Some(ref c) = v.cause {
            if !c.is_empty() && step != "pass" {
                return format!("action_result: {step} ({c})");
            }
        }
    }
    format!("action_result: {step}")
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

/// Sidecar tool id used to carry verify/repetition signal for tier runtime.
pub const COMPUTER_TIER_SIGNAL_TOOL_NAME: &str = "verify";

/// Extract repetition signal from sidecar tool calls in one assistant round.
///
/// Expected sidecar payload:
/// `{ "action_result": "pass|fail|pending|n/a", "repetition_count": <u32>, "failure_cause"?: "wrong_operation|precision_miss" }`.
pub fn parse_tier_signal_from_sidecar_tool_calls(tool_calls: &[ToolCall]) -> Option<ParsedTierSignal> {
    let mut parsed: Option<ParsedTierSignal> = None;
    for tc in tool_calls {
        if registry_tool_base_name(tc.name.as_str()) != COMPUTER_TIER_SIGNAL_TOOL_NAME {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&tc.arguments) else {
            continue;
        };
        let Some(obj) = v.as_object() else {
            continue;
        };
        let Some(action_result) = obj
            .get("action_result")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
        else {
            continue;
        };
        if !matches!(action_result.as_str(), "pass" | "fail" | "pending" | "n/a") {
            continue;
        }
        let Some(repetition_count) = obj
            .get("repetition_count")
            .and_then(|x| x.as_u64())
            .and_then(|n| u32::try_from(n).ok())
        else {
            continue;
        };
        let failure_cause = obj
            .get("failure_cause")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase());
        let failure_cause = match action_result.as_str() {
            "fail" => {
                let Some(cause) = failure_cause else {
                    continue;
                };
                if !matches!(cause.as_str(), "wrong_operation" | "precision_miss") {
                    continue;
                }
                Some(cause)
            }
            _ => {
                if failure_cause.is_some() {
                    continue;
                }
                None
            }
        };
        parsed = Some(ParsedTierSignal {
            action_result,
            repetition_count,
            failure_cause,
        });
    }
    parsed
}

fn registry_tool_base_name(raw: &str) -> &str {
    match raw.trim().split_once(':') {
        Some((base, rest)) if !base.is_empty() && !rest.trim().is_empty() => base.trim(),
        _ => raw.trim(),
    }
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
    let (px, py) = vision.resolve_index(index)?;
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
        let key = tier.label();
        if let Some(t) = config.tier_llm.get(key) {
            return Self {
                model: t.model.clone(),
                enable_thinking: t.enable_thinking,
                thinking_budget: t.thinking_budget,
            };
        }
        match tier {
            ComputerTier::Primary => Self {
                model: config.model_primary.clone(),
                enable_thinking: true,
                thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
            },
            ComputerTier::Intermediate => Self {
                model: config.model_intermediate.clone(),
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

/// Index-targeting mouse/composite/modified_click methods are allowed on every tier (including Advanced).
pub fn tier_allows_index_tools(_tier: ComputerTier) -> bool {
    true
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
            tool_name: "mouse_click_at".into(),
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
        assert!(line.contains("action_result: fail (precision_miss)"));
    }

    #[test]
    fn tier_bumps_after_four_fails_when_auto_upgrade() {
        let config = ComputerTierConfig {
            auto_upgrade: true,
            initial_tier: ComputerTier::Intermediate,
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
                None,
                0,
            );
        }
        assert_eq!(rt.current_tier, ComputerTier::Intermediate);
    }

    #[test]
    fn primary_history_omits_cause() {
        let r = TierActionRecord {
            tool_name: "mouse_click_at".into(),
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
        assert!(line.contains("action_result: fail"));
        assert!(!line.contains("precision_miss"));
    }

    #[test]
    fn same_goal_repetition_count_sums_only_fail_rows() {
        let mk = |goal: &str, step: &str| TierActionRecord {
            tool_name: "mouse_click_index".into(),
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
            mk("open settings", "n/a"),
        ];
        assert_eq!(same_goal_repetition_count_in_history(&records), 1);
        let switched = vec![
            mk("open settings", "fail"),
            mk("open settings", "n/a"),
            mk("other", "fail"),
        ];
        assert_eq!(same_goal_repetition_count_in_history(&switched), 1);
    }

    #[test]
    fn tier_runtime_block_shows_repetition_policy_and_upgrade_hint() {
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
        assert!(block.contains("repeat-policy: count=0"));
    }

    #[test]
    fn history_block_keeps_verify_only_in_history_rows() {
        let records = vec![TierActionRecord {
            tool_name: "mouse_click_index".into(),
            goal: "Open Settings".into(),
            action: Some("click settings icon".into()),
            coords: Some((400, 300)),
            verify_result: Some(VerifyOutcome {
                step_result: "fail".into(),
                cause: Some("unexpected_change".into()),
            }),
            extra_args_hint: None,
        }];
        let block = format_tier_history_block(ComputerTier::Intermediate, &records).unwrap();
        assert!(block.contains("action_result: fail (unexpected_change)"));
        assert!(!block.contains("Last verify result (previous round):"));
    }

    #[test]
    fn tier_runtime_block_escalates_policy_when_four_same_goal_fails() {
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
        assert!(block.contains("repeat-policy: count=4 escalate tier/reasoning"));
    }

    #[test]
    fn on_round_complete_sets_give_up_when_effective_repetition_reaches_threshold() {
        let config = ComputerTierConfig::default();
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        rt.on_round_complete(
            &config,
            Some(&ParsedVerify {
                step_result: "fail".into(),
                cause: None,
            }),
            Some("same goal"),
            Some(5),
            0,
        );
        assert!(rt.should_give_up);
    }

    #[test]
    fn on_round_complete_give_up_uses_host_count_when_sidecar_missing() {
        let config = ComputerTierConfig::default();
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        rt.on_round_complete(
            &config,
            Some(&ParsedVerify {
                step_result: "fail".into(),
                cause: None,
            }),
            Some("same goal"),
            None,
            5,
        );
        assert!(rt.should_give_up);
    }

    #[test]
    fn on_round_complete_clears_give_up_on_verify_pass() {
        let config = ComputerTierConfig::default();
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        rt.should_give_up = true;
        rt.on_round_complete(
            &config,
            Some(&ParsedVerify {
                step_result: "pass".into(),
                cause: None,
            }),
            Some("same goal"),
            Some(5),
            5,
        );
        assert!(!rt.should_give_up);
    }

    #[test]
    fn reset_give_up_for_new_turn_clears_flag() {
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        rt.should_give_up = true;
        rt.reset_give_up_for_new_turn();
        assert!(!rt.should_give_up);
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
        assert_eq!(cfg.initial_tier, ComputerTier::Intermediate);
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
        assert!(tier_allows_index_tools(ComputerTier::Advanced));
    }

    #[test]
    fn parse_tier_signal_from_sidecar_tool_calls_prefers_latest_signal() {
        let calls = vec![
            ToolCall {
                id: "a".into(),
                name: "task_board_patch".into(),
                arguments: "{}".into(),
                status: "pending".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            },
            ToolCall {
                id: "b".into(),
                name: "verify:report".into(),
                arguments: r#"{"action_result":"fail","repetition_count":2,"failure_cause":"precision_miss"}"#.into(),
                status: "pending".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            },
            ToolCall {
                id: "c".into(),
                name: "verify:report".into(),
                arguments: r#"{"action_result":"pass","repetition_count":4}"#.into(),
                status: "pending".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            },
        ];
        let p = parse_tier_signal_from_sidecar_tool_calls(&calls).unwrap();
        assert_eq!(p.action_result, "pass");
        assert_eq!(p.repetition_count, 4);
        assert!(p.failure_cause.is_none());
    }
}

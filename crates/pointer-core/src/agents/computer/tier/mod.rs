//! Computer agent tier runtime: Primary / Intermediate / Advanced profiles.

use crate::agents::computer::vision::coord::{screen_to_normalized, CoordinateSystem};
use crate::agents::computer::vision::vision_state::VisionState;
use crate::agents::AgentRegistry;
use crate::models::ComputerTierLlmConfig;
use serde_json::Value;
use std::collections::HashMap;

pub const CONFIG_KEY_AUTO_UPGRADE: &str = "computerAutoUpgrade";
pub const CONFIG_KEY_INITIAL_TIER: &str = "computerInitialTier";
pub const CONFIG_KEY_MODEL_PRIMARY: &str = "computerModelPrimary";
pub const CONFIG_KEY_MODEL_INTERMEDIATE: &str = "computerModelIntermediate";
pub const CONFIG_KEY_MODEL_ADVANCED: &str = "computerModelAdvanced";
pub const CONFIG_KEY_ADVANCED_PIPELINE: &str = "computerAdvancedPipeline";
pub const CONFIG_KEY_PIPELINE_MODEL_DECISION: &str = "computerPipelineModelDecision";
pub const CONFIG_KEY_PIPELINE_MODEL_POSITION: &str = "computerPipelineModelPosition";
pub const CONFIG_KEY_PIPELINE_MODEL_VERIFY: &str = "computerPipelineModelVerify";
pub const CONFIG_KEY_PIPELINE_THINKING_BUDGET_POSITION: &str =
    "computerPipelineThinkingBudgetPosition";
pub const CONFIG_KEY_PIPELINE_THINKING_BUDGET_VERIFY: &str = "computerPipelineThinkingBudgetVerify";
pub const CONFIG_KEY_PIPELINE_VERIFY_HOST: &str = "computerPipelineVerifyHost";

// 平台模型配置全部由平台下发；本地不内置 computer tier 默认模型（登录后由
// tierDefaults.computerTierLlm / computerPipelineLlm 注入，未配置时为空）。
pub const DEFAULT_MODEL_PRIMARY: &str = "";
pub const DEFAULT_MODEL_INTERMEDIATE: &str = "";
pub const DEFAULT_MODEL_ADVANCED: &str = "";
pub const DEFAULT_MODEL_PIPELINE_DECISION: &str = "";
pub const DEFAULT_MODEL_PIPELINE_POSITION: &str = "";
pub const DEFAULT_MODEL_PIPELINE_VERIFY: &str = "";
/// Default LLM provider id for computer tier / pipeline overrides (empty = none local).
pub const DEFAULT_COMPUTER_LLM_PROVIDER: &str = "";
/// `thinking_budget` for Advanced pipeline Position phase.
pub const DEFAULT_PIPELINE_POSITION_THINKING_BUDGET: u32 = 1024;
/// `thinking_budget` for Advanced pipeline Verify phase.
pub const DEFAULT_PIPELINE_VERIFY_THINKING_BUDGET: u32 = 256;
/// `thinking_budget` for Primary / Intermediate.
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

/// Per-phase LLM model ids and thinking budgets for Advanced modular pipeline.
#[derive(Debug, Clone)]
pub struct ComputerPipelineLlmConfig {
    pub model_decision: String,
    pub model_position: String,
    pub model_verify: String,
    pub provider_decision: String,
    pub provider_position: String,
    pub provider_verify: String,
    pub thinking_budget_position: u32,
    pub thinking_budget_verify: u32,
}

impl Default for ComputerPipelineLlmConfig {
    fn default() -> Self {
        Self {
            model_decision: DEFAULT_MODEL_PIPELINE_DECISION.into(),
            model_position: DEFAULT_MODEL_PIPELINE_POSITION.into(),
            model_verify: DEFAULT_MODEL_PIPELINE_VERIFY.into(),
            provider_decision: DEFAULT_COMPUTER_LLM_PROVIDER.into(),
            provider_position: DEFAULT_COMPUTER_LLM_PROVIDER.into(),
            provider_verify: DEFAULT_COMPUTER_LLM_PROVIDER.into(),
            thinking_budget_position: DEFAULT_PIPELINE_POSITION_THINKING_BUDGET,
            thinking_budget_verify: DEFAULT_PIPELINE_VERIFY_THINKING_BUDGET,
        }
    }
}

/// Advanced pipeline LLM phase for per-stage model overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineLlmPhase {
    Decision,
    Position,
    Verify,
}

impl PipelineLlmPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Position => "position",
            Self::Verify => "verify",
        }
    }
}

impl ComputerPipelineLlmConfig {
    pub fn model_for_phase(&self, phase: PipelineLlmPhase) -> &str {
        match phase {
            PipelineLlmPhase::Decision => &self.model_decision,
            PipelineLlmPhase::Position => &self.model_position,
            PipelineLlmPhase::Verify => &self.model_verify,
        }
    }

    pub fn provider_for_phase(&self, phase: PipelineLlmPhase) -> &str {
        match phase {
            PipelineLlmPhase::Decision => &self.provider_decision,
            PipelineLlmPhase::Position => &self.provider_position,
            PipelineLlmPhase::Verify => &self.provider_verify,
        }
    }

    pub fn thinking_for_phase(&self, phase: PipelineLlmPhase) -> (bool, u32) {
        match phase {
            PipelineLlmPhase::Decision => (true, PRIMARY_INTERMEDIATE_THINKING_BUDGET),
            PipelineLlmPhase::Position => (true, self.thinking_budget_position),
            PipelineLlmPhase::Verify => (true, self.thinking_budget_verify),
        }
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
    pub advanced_pipeline: bool,
    pub verify_host_enabled: bool,
    pub pipeline_llm: ComputerPipelineLlmConfig,
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
            advanced_pipeline: true,
            verify_host_enabled: true,
            pipeline_llm: ComputerPipelineLlmConfig::default(),
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
                CONFIG_KEY_ADVANCED_PIPELINE => {
                    cfg.advanced_pipeline = v.eq_ignore_ascii_case("true") || v == "1";
                }
                CONFIG_KEY_PIPELINE_VERIFY_HOST => {
                    cfg.verify_host_enabled = v.eq_ignore_ascii_case("true") || v == "1";
                }
                CONFIG_KEY_PIPELINE_MODEL_DECISION if !v.trim().is_empty() => {
                    cfg.pipeline_llm.model_decision = v.trim().to_string();
                }
                CONFIG_KEY_PIPELINE_MODEL_POSITION if !v.trim().is_empty() => {
                    cfg.pipeline_llm.model_position = v.trim().to_string();
                }
                CONFIG_KEY_PIPELINE_MODEL_VERIFY if !v.trim().is_empty() => {
                    cfg.pipeline_llm.model_verify = v.trim().to_string();
                }
                CONFIG_KEY_PIPELINE_THINKING_BUDGET_POSITION => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        if n > 0 {
                            cfg.pipeline_llm.thinking_budget_position = n;
                        }
                    }
                }
                CONFIG_KEY_PIPELINE_THINKING_BUDGET_VERIFY => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        if n > 0 {
                            cfg.pipeline_llm.thinking_budget_verify = n;
                        }
                    }
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

    /// Override Advanced pipeline phase models from platform `computerPipelineLlm`.
    pub fn apply_platform_pipeline_llm(&mut self, p: &crate::models::ComputerPipelineLlmSettings) {
        if !p.decision.trim().is_empty() {
            self.pipeline_llm.model_decision = p.decision.trim().to_string();
        }
        if !p.position.trim().is_empty() {
            self.pipeline_llm.model_position = p.position.trim().to_string();
        }
        if !p.verify.trim().is_empty() {
            self.pipeline_llm.model_verify = p.verify.trim().to_string();
        }
        if !p.decision_provider_id.trim().is_empty() {
            self.pipeline_llm.provider_decision = p.decision_provider_id.trim().to_string();
        }
        if !p.position_provider_id.trim().is_empty() {
            self.pipeline_llm.provider_position = p.position_provider_id.trim().to_string();
        }
        if !p.verify_provider_id.trim().is_empty() {
            self.pipeline_llm.provider_verify = p.verify_provider_id.trim().to_string();
        }
        if p.position_thinking_budget > 0 {
            self.pipeline_llm.thinking_budget_position = p.position_thinking_budget;
        }
        if p.verify_thinking_budget > 0 {
            self.pipeline_llm.thinking_budget_verify = p.verify_thinking_budget;
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub step_result: String,
    pub cause: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedVerify {
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
    /// Set to true when repetition_count reaches GIVE_UP_THRESHOLD;
    /// the outer loop reads this to exit with a user-guidance request.
    pub should_give_up: bool,
    /// Archived failed-attempt summary after user guidance reset; shown in
    /// `[Recent desktop tool calls]` but excluded from repetition counting.
    give_up_reference: Option<String>,
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
            give_up_reference: None,
        }
    }

    pub fn give_up_reference(&self) -> Option<&str> {
        self.give_up_reference.as_deref()
    }

    pub fn history_for(&self, tier: ComputerTier) -> &[TierActionRecord] {
        self.histories
            .get(&tier)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn push_action(&mut self, tier: ComputerTier, record: TierActionRecord) {
        self.auto_close_all_open_rows_before_push(tier);
        let v = self.histories.entry(tier).or_default();
        v.push(record);
        if v.len() > MAX_TIER_HISTORY {
            v.remove(0);
        }
    }

    /// Close every open row before a new desktop action (never verified — `skipped`).
    pub fn auto_close_all_open_rows_before_push(&mut self, tier: ComputerTier) {
        let Some(v) = self.histories.get_mut(&tier) else {
            return;
        };
        let n = auto_close_all_open_rows_slice(v);
        if n > 0 {
            log::warn!(
                "computer tier: auto-closed {n} open history row(s) as verify skipped before new desktop action"
            );
        }
    }

    /// Close stale open rows, keeping the newest open row for verify this turn.
    pub fn auto_close_stale_open_rows(&mut self, tier: ComputerTier) {
        let Some(v) = self.histories.get_mut(&tier) else {
            return;
        };
        let n = auto_close_stale_open_rows_slice(v);
        if n > 0 {
            log::warn!(
                "computer tier: auto-closed {n} stale open history row(s) as verify skipped (kept newest open)"
            );
        }
    }

    /// Backfill verify outcome into the newest open history row. Returns true when stored.
    pub fn backfill_newest_open_verify(
        &mut self,
        tier: ComputerTier,
        outcome: VerifyOutcome,
    ) -> bool {
        if outcome.step_result == "pending" {
            return false;
        }
        let Some(v) = self.histories.get_mut(&tier) else {
            return false;
        };
        let Some(idx) = v.iter().rposition(|r| r.verify_result.is_none()) else {
            log::warn!("computer tier: host verify ignored — no open verifying row to close");
            return false;
        };
        if v[idx].verify_result.is_some() {
            return false;
        }
        v[idx].verify_result = Some(outcome);
        true
    }

    /// Back-compat alias — prefer [`backfill_newest_open_verify`].
    pub fn backfill_last_verify(&mut self, tier: ComputerTier, outcome: VerifyOutcome) {
        let _ = self.backfill_newest_open_verify(tier, outcome);
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
            // Sidecar reads prompt text and may count archived give-up rows; host history wins when higher.
            Some(sidecar) if sidecar > host_repetition_count => host_repetition_count,
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
                if self
                    .locked_goal
                    .as_ref()
                    .is_some_and(|l| l.fingerprint == fp)
                {
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
            let effective_rep =
                Self::effective_repetition_count(sidecar_repetition_count, host_repetition_count);
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

    /// Full runtime reset when the user sends new guidance after give-up or a stuck loop.
    /// Clears streaks and tier history; archives prior failed ops into `give_up_reference`.
    pub fn reset_for_new_user_guidance(&mut self, initial_tier: ComputerTier) {
        let reference = self.build_give_up_reference_block();
        self.histories.clear();
        self.current_tier = initial_tier;
        self.tier_error_streak = 0;
        self.task_error_streak = 0;
        self.locked_goal = None;
        self.goal_fail_fingerprint = None;
        self.goal_fail_streak = 0;
        self.should_give_up = false;
        self.last_executed_goal = None;
        self.give_up_reference = reference;
        log::info!(
            "computer tier: reset for new user guidance (tier={}, give_up_reference={})",
            initial_tier.label(),
            self.give_up_reference.is_some()
        );
    }

    fn build_give_up_reference_block(&self) -> Option<String> {
        let mut all_rows: Vec<(ComputerTier, &TierActionRecord)> = Vec::new();
        for tier in [
            ComputerTier::Primary,
            ComputerTier::Intermediate,
            ComputerTier::Advanced,
        ] {
            if let Some(records) = self.histories.get(&tier) {
                for r in records {
                    all_rows.push((tier, r));
                }
            }
        }
        if all_rows.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        if self.should_give_up {
            lines.push("Stop reason: repetition exhausted — user guidance requested.".to_string());
        }
        if let Some(lock) = &self.locked_goal {
            lines.push(format!(
                "Locked goal at reset: \"{}\"",
                escape_goal(&lock.label)
            ));
        }

        let fail_rows: Vec<_> = all_rows
            .iter()
            .filter(|(_, r)| {
                r.verify_result
                    .as_ref()
                    .is_some_and(|v| verify_counts_toward_repetition_count(v.step_result.as_str()))
            })
            .collect();

        if fail_rows.is_empty() {
            lines.push("Recent operations before reset (reference only):".to_string());
            for (tier, r) in &all_rows {
                lines.push(format!("  - {}", format_history_line(*tier, r)));
            }
        } else {
            lines.push(
                "Failed operations (do not repeat — change tool or target element in Next):"
                    .to_string(),
            );
            for (tier, r) in fail_rows {
                lines.push(format!("  - {}", format_history_line(*tier, r)));
            }
        }
        Some(lines.join("\n"))
    }

    /// Test helper: append a history row without auto-closing open rows.
    #[cfg(test)]
    pub fn test_push_open_row(&mut self, tier: ComputerTier, record: TierActionRecord) {
        let v = self.histories.entry(tier).or_default();
        v.push(record);
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

fn verify_outcome_skipped() -> VerifyOutcome {
    VerifyOutcome {
        step_result: "skipped".into(),
        cause: None,
    }
}

/// Mark every open row as skipped (before pushing a new desktop action).
fn auto_close_all_open_rows_slice(records: &mut [TierActionRecord]) -> u32 {
    let mut n = 0u32;
    for r in records.iter_mut() {
        if r.verify_result.is_none() {
            r.verify_result = Some(verify_outcome_skipped());
            n = n.saturating_add(1);
        }
    }
    n
}

/// Mark older open rows as skipped; keep the newest open row for verify this turn.
fn auto_close_stale_open_rows_slice(records: &mut [TierActionRecord]) -> u32 {
    let open_indices: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.verify_result.is_none())
        .map(|(i, _)| i)
        .collect();
    if open_indices.len() <= 1 {
        return 0;
    }
    let keep = *open_indices.last().unwrap();
    let mut n = 0u32;
    for (i, r) in records.iter_mut().enumerate() {
        if r.verify_result.is_none() && i != keep {
            r.verify_result = Some(verify_outcome_skipped());
            n = n.saturating_add(1);
        }
    }
    n
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
pub fn format_tier_history_block(
    tier: ComputerTier,
    records: &[TierActionRecord],
    give_up_reference: Option<&str>,
) -> Option<String> {
    if records.is_empty() && give_up_reference.is_none() {
        return None;
    }
    let mut lines = vec![
        "[Recent desktop tool calls — ordered oldest to newest; repetition uses goal; coordinates are session 0-1000; overlay indices are not comparable across turns.]".to_string(),
        "Verify suffix: only the newest row may show verify: verifying; verify: skipped = never verified; verify: verified - * = closed — do not re-verify or re-report.".to_string(),
    ];
    if let Some(ref_block) = give_up_reference {
        lines.push(
            "[Prior attempt — give up reference; host-maintained after user guidance; do NOT re-verify or re-report these rows. Use in Next to avoid repeating failed tool+target combinations.]".to_string(),
        );
        for line in ref_block.lines() {
            if !line.trim().is_empty() {
                lines.push(format!("  {line}"));
            }
        }
    }
    if records.is_empty() {
        if give_up_reference.is_some() {
            lines.push(
                "[Current session — no desktop tool calls yet after user guidance.]".to_string(),
            );
        }
    } else {
        if give_up_reference.is_some() {
            lines.push(
                "[Current session — fresh after user guidance; verify only rows below.]"
                    .to_string(),
            );
        }
        for (i, r) in records.iter().enumerate() {
            lines.push(format!("  {}: {}", i + 1, format_history_line(tier, r)));
        }
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

fn format_verify_suffix(_tier: ComputerTier, v: Option<&VerifyOutcome>) -> String {
    let Some(v) = v else {
        return "verify: verifying".into();
    };
    match v.step_result.as_str() {
        "skipped" => "verify: skipped".into(),
        "pass" => "verify: verified - pass".into(),
        "n/a" => "verify: verified - n/a".into(),
        "fail" => {
            if let Some(ref c) = v.cause {
                if !c.is_empty() {
                    return format!("verify: verified - {c}");
                }
            }
            "verify: verified - fail".into()
        }
        other => format!("verify: verified - {other}"),
    }
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
        .map(|s| {
            s.trim()
                .trim_matches(|c: char| c == '.' || c == ';')
                .to_string()
        })
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("—") && !s.eq_ignore_ascii_case("-"));
    Some(ParsedVerify { step_result, cause })
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
    let goal = args
        .get("goal")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
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
    let index = args
        .get("index")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)?;
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
                return format!(
                    "({})",
                    keys.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
        }
    }
    parts.join(" ")
}

/// Per-round LLM overrides for computer tier.
#[derive(Debug, Clone)]
pub struct ComputerRoundLlmOverrides {
    pub provider_id: String,
    pub model: String,
    pub enable_thinking: bool,
    pub thinking_budget: Option<u32>,
}

impl ComputerRoundLlmOverrides {
    pub fn for_tier(tier: ComputerTier, config: &ComputerTierConfig) -> Self {
        let key = tier.label();
        if let Some(t) = config.tier_llm.get(key) {
            let provider_id = if t.provider_id.trim().is_empty() {
                DEFAULT_COMPUTER_LLM_PROVIDER.to_string()
            } else {
                t.provider_id.clone()
            };
            return Self {
                provider_id,
                model: t.model.clone(),
                enable_thinking: t.enable_thinking,
                thinking_budget: t.thinking_budget,
            };
        }
        match tier {
            ComputerTier::Primary => Self {
                provider_id: DEFAULT_COMPUTER_LLM_PROVIDER.into(),
                model: config.model_primary.clone(),
                enable_thinking: true,
                thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
            },
            ComputerTier::Intermediate => Self {
                provider_id: DEFAULT_COMPUTER_LLM_PROVIDER.into(),
                model: config.model_intermediate.clone(),
                enable_thinking: true,
                thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
            },
            ComputerTier::Advanced => Self {
                provider_id: DEFAULT_COMPUTER_LLM_PROVIDER.into(),
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
        assert!(line.contains("verify: verified - precision_miss"));
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
    fn primary_history_shows_fail_cause_in_verify_suffix() {
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
        assert!(line.contains("verify: verified - precision_miss"));
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
    fn open_row_shows_verifying_suffix() {
        let r = TierActionRecord {
            tool_name: "mouse_click_index".into(),
            goal: "g".into(),
            action: None,
            coords: None,
            verify_result: None,
            extra_args_hint: None,
        };
        let line = format_history_line(ComputerTier::Primary, &r);
        assert!(line.contains("verify: verifying"));
    }

    #[test]
    fn skipped_row_shows_skipped_not_verified() {
        let r = TierActionRecord {
            tool_name: "mouse_click_index".into(),
            goal: "g".into(),
            action: None,
            coords: None,
            verify_result: Some(VerifyOutcome {
                step_result: "skipped".into(),
                cause: None,
            }),
            extra_args_hint: None,
        };
        let line = format_history_line(ComputerTier::Primary, &r);
        assert!(line.contains("verify: skipped"));
        assert!(!line.contains("verified"));
    }

    #[test]
    fn push_action_closes_open_rows_as_skipped() {
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        let tier = ComputerTier::Primary;
        rt.push_action(
            tier,
            TierActionRecord {
                tool_name: "a".into(),
                goal: "g1".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
        );
        rt.push_action(
            tier,
            TierActionRecord {
                tool_name: "b".into(),
                goal: "g2".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
        );
        let records = rt.history_for(tier);
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].verify_result.as_ref().unwrap().step_result,
            "skipped"
        );
        assert!(records[1].verify_result.is_none());
    }

    #[test]
    fn backfill_newest_open_targets_newest_not_oldest() {
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        let tier = ComputerTier::Primary;
        rt.test_push_open_row(
            tier,
            TierActionRecord {
                tool_name: "a".into(),
                goal: "g".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
        );
        rt.test_push_open_row(
            tier,
            TierActionRecord {
                tool_name: "b".into(),
                goal: "g".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
        );
        let applied = rt.backfill_newest_open_verify(
            tier,
            VerifyOutcome {
                step_result: "pass".into(),
                cause: None,
            },
        );
        assert!(applied);
        let records = rt.history_for(tier);
        assert!(records[0].verify_result.is_none());
        assert_eq!(
            records[1].verify_result.as_ref().unwrap().step_result,
            "pass"
        );
    }

    #[test]
    fn backfill_pending_does_not_close_row() {
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        let tier = ComputerTier::Primary;
        rt.push_action(
            tier,
            TierActionRecord {
                tool_name: "a".into(),
                goal: "g".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
        );
        let applied = rt.backfill_newest_open_verify(
            tier,
            VerifyOutcome {
                step_result: "pending".into(),
                cause: None,
            },
        );
        assert!(!applied);
        assert!(rt.history_for(tier)[0].verify_result.is_none());
    }

    #[test]
    fn skipped_does_not_count_toward_repetition() {
        let records = vec![
            TierActionRecord {
                tool_name: "t".into(),
                goal: "same".into(),
                action: None,
                coords: None,
                verify_result: Some(VerifyOutcome {
                    step_result: "skipped".into(),
                    cause: None,
                }),
                extra_args_hint: None,
            },
            TierActionRecord {
                tool_name: "t".into(),
                goal: "same".into(),
                action: None,
                coords: None,
                verify_result: Some(VerifyOutcome {
                    step_result: "fail".into(),
                    cause: None,
                }),
                extra_args_hint: None,
            },
        ];
        assert_eq!(same_goal_repetition_count_in_history(&records), 1);
    }

    #[test]
    fn stale_auto_close_keeps_newest_open() {
        let mut records = vec![
            TierActionRecord {
                tool_name: "a".into(),
                goal: "g".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
            TierActionRecord {
                tool_name: "b".into(),
                goal: "g".into(),
                action: None,
                coords: None,
                verify_result: None,
                extra_args_hint: None,
            },
        ];
        let n = auto_close_stale_open_rows_slice(&mut records);
        assert_eq!(n, 1);
        assert_eq!(
            records[0].verify_result.as_ref().unwrap().step_result,
            "skipped"
        );
        assert!(records[1].verify_result.is_none());
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
        let block = format_tier_history_block(ComputerTier::Intermediate, &records, None).unwrap();
        assert!(block.contains("verify: verified - unexpected_change"));
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
            5,
        );
        assert!(rt.should_give_up);
    }

    #[test]
    fn on_round_complete_ignores_inflated_sidecar_repetition_count() {
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
            1,
        );
        assert!(
            !rt.should_give_up,
            "sidecar must not exceed host history for give-up"
        );
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
    fn reset_for_new_user_guidance_clears_streaks_and_archives_reference() {
        let mut rt = ComputerTierRuntime::new(ComputerTier::Advanced);
        rt.tier_error_streak = 3;
        rt.task_error_streak = 2;
        rt.goal_fail_streak = 2;
        rt.goal_fail_fingerprint = Some("fp".into());
        rt.should_give_up = true;
        rt.locked_goal = Some(LockedGoal {
            fingerprint: "fp".into(),
            label: "Open app".into(),
        });
        rt.push_action(
            ComputerTier::Advanced,
            TierActionRecord {
                tool_name: "mouse_click_index".into(),
                goal: "Open app".into(),
                action: Some("click wrong icon".into()),
                coords: None,
                verify_result: Some(VerifyOutcome {
                    step_result: "fail".into(),
                    cause: Some("wrong_operation".into()),
                }),
                extra_args_hint: None,
            },
        );
        rt.reset_for_new_user_guidance(ComputerTier::Primary);
        assert!(!rt.should_give_up);
        assert_eq!(rt.tier_error_streak, 0);
        assert_eq!(rt.task_error_streak, 0);
        assert_eq!(rt.goal_fail_streak, 0);
        assert!(rt.locked_goal.is_none());
        assert_eq!(rt.current_tier, ComputerTier::Primary);
        assert!(rt.history_for(ComputerTier::Advanced).is_empty());
        let reference = rt.give_up_reference().expect("reference");
        assert!(reference.contains("wrong_operation"));
        assert!(reference.contains("Stop reason"));
    }

    #[test]
    fn history_block_includes_give_up_reference_without_live_rows() {
        let reference = "Failed operations (do not repeat):\n  - mouse_click_index goal=\"g\" | verify: verified - wrong_operation";
        let block = format_tier_history_block(ComputerTier::Primary, &[], Some(reference)).unwrap();
        assert!(block.contains("[Prior attempt — give up reference"));
        assert!(block.contains("wrong_operation"));
        assert!(block.contains("[Current session — no desktop tool calls yet"));
    }

    #[test]
    fn reset_for_new_user_guidance_fresh_repetition_count() {
        let config = ComputerTierConfig::default();
        let mut rt = ComputerTierRuntime::new(ComputerTier::Primary);
        for _ in 0..5 {
            rt.push_action(
                ComputerTier::Primary,
                TierActionRecord {
                    tool_name: "mouse_click_index".into(),
                    goal: "same goal".into(),
                    action: Some("click".into()),
                    coords: None,
                    verify_result: Some(VerifyOutcome {
                        step_result: "fail".into(),
                        cause: Some("wrong_operation".into()),
                    }),
                    extra_args_hint: None,
                },
            );
        }
        assert_eq!(
            same_goal_repetition_count_in_history(rt.history_for(ComputerTier::Primary)),
            5
        );
        rt.should_give_up = true;
        rt.reset_for_new_user_guidance(ComputerTier::Primary);
        assert_eq!(
            same_goal_repetition_count_in_history(rt.history_for(ComputerTier::Primary)),
            0
        );
        rt.on_round_complete(
            &config,
            Some(&ParsedVerify {
                step_result: "fail".into(),
                cause: Some("wrong_operation".into()),
            }),
            Some("same goal"),
            Some(5),
            0,
        );
        assert!(
            !rt.should_give_up,
            "sidecar rep=5 must not re-trigger give-up after guidance reset"
        );
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
    fn pipeline_llm_default_models() {
        let cfg = ComputerTierConfig::default();
        assert_eq!(
            cfg.pipeline_llm.model_decision,
            DEFAULT_MODEL_PIPELINE_DECISION
        );
        assert_eq!(
            cfg.pipeline_llm.model_position,
            DEFAULT_MODEL_PIPELINE_POSITION
        );
        assert_eq!(cfg.pipeline_llm.model_verify, DEFAULT_MODEL_PIPELINE_VERIFY);
        assert_eq!(
            cfg.pipeline_llm.thinking_budget_position,
            DEFAULT_PIPELINE_POSITION_THINKING_BUDGET
        );
        assert_eq!(
            cfg.pipeline_llm.thinking_budget_verify,
            DEFAULT_PIPELINE_VERIFY_THINKING_BUDGET
        );
    }

    #[test]
    fn pipeline_llm_thinking_for_phase() {
        let cfg = ComputerTierConfig::default();
        assert_eq!(
            cfg.pipeline_llm
                .thinking_for_phase(PipelineLlmPhase::Position),
            (true, DEFAULT_PIPELINE_POSITION_THINKING_BUDGET)
        );
        assert_eq!(
            cfg.pipeline_llm
                .thinking_for_phase(PipelineLlmPhase::Verify),
            (true, DEFAULT_PIPELINE_VERIFY_THINKING_BUDGET)
        );
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
}

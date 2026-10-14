//! `pointer-cli` — headless, GUI-free CLI runner for the Pointer agent.
//!
//! Runs one instruction through the same `RunDispatcher` the desktop (Tauri)
//! and server (axum) hosts use, but without any of their dependencies: no
//! Tauri, no axum, no display server.
//!
//! Credentials come **only** from an environment variable (default
//! `POINTER_API_KEY`). The user's real data directory
//! (`~/.local/share/PointerApp`) is never read: unless `--data-dir` is given,
//! a throwaway temp directory is used for `user_settings.json` and SQLite.
//!
//! Controllability switches (all opt-in; omitting them keeps the previous
//! behaviour):
//!
//! - `--allowed-tools` — restrict the tools the agent may see / call.
//! - `--max-turns` — hard cap on tool-call rounds (maps to `maxToolRounds`).
//! - `--timeout` — wall-clock deadline that **cancels** the run (killing an
//!   in-flight `terminal` child process) instead of only abandoning the wait.
//!
//! Exit codes: `0` success · `2` usage · `10` timeout · `11` auth ·
//! `12` model/provider · `13` internal · `14` tool-round limit reached.

mod artifacts;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand};
use pointer_core::chat_service::AppState;
use pointer_core::dispatcher::{
    DeliverTarget, RunOutcome, TriggerMeta, TriggerRequest, TriggerSource,
};
use pointer_core::models::{ChatMessage, ProviderConfig, Role, StreamEvent};
use pointer_core::tools::ToolRegistry;

/// Provider id used for the single env-var-backed provider the CLI injects.
const PROVIDER_ID: &str = "cli";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4o-mini";
const DEFAULT_API_KEY_ENV: &str = "POINTER_API_KEY";

/// Wall-clock grace period after a timeout cancel, before the process exits
/// anyway. Long enough for the agent loop to trip its cancel guard and for a
/// cooperatively-cancelled tool call to reap its child process.
const CANCEL_GRACE: Duration = Duration::from_secs(10);

const EXIT_SUCCESS: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_TIMEOUT: i32 = 10;
const EXIT_AUTH: i32 = 11;
const EXIT_MODEL: i32 = 12;
const EXIT_INTERNAL: i32 = 13;
const EXIT_TURN_LIMIT: i32 = 14;

/// Error carrying the process exit code the CLI should return.
#[derive(Debug)]
struct CliError {
    code: i32,
    message: String,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// Build an [`anyhow::Error`] that still carries a semantic exit code.
fn cli_error(code: i32, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CliError {
        code,
        message: message.into(),
    })
}

/// Best-effort classification of a provider / runtime failure string.
fn classify_run_failure(error: &str) -> i32 {
    let s = error.to_ascii_lowercase();
    let auth = [
        "401",
        "403",
        "unauthorized",
        "invalid_api_key",
        "invalid api key",
        "api key",
        "authentication",
        "insufficient_quota",
        "permission",
    ];
    if auth.iter().any(|needle| s.contains(needle)) {
        return EXIT_AUTH;
    }
    let model = [
        "400",
        "404",
        "422",
        "429",
        "rate limit",
        "too many requests",
        "model",
        "http 5",
        "connection",
        "dns",
        "tls",
        "unexpected eof",
        "stream",
        "provider",
    ];
    if model.iter().any(|needle| s.contains(needle)) {
        return EXIT_MODEL;
    }
    EXIT_INTERNAL
}

#[derive(Parser)]
#[command(
    name = "pointer-cli",
    version,
    about = "Headless Pointer agent runner (no GUI / display server required)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run one instruction to completion and print the assistant reply.
    Run(RunArgs),
}

#[derive(Args)]
struct RunArgs {
    /// Instruction text to send to the agent.
    #[arg(long, conflicts_with = "instruction_file")]
    instruction: Option<String>,
    /// Read the instruction text from a file.
    #[arg(long, value_name = "PATH")]
    instruction_file: Option<PathBuf>,
    /// Workspace root for file / terminal tools.
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Model id sent to the provider.
    #[arg(long, default_value = DEFAULT_MODEL)]
    model: String,
    /// Name of the environment variable that holds the provider API key.
    #[arg(long, default_value = DEFAULT_API_KEY_ENV)]
    api_key_env: String,
    /// OpenAI-compatible provider base URL (…/v1).
    #[arg(long, default_value = DEFAULT_BASE_URL)]
    base_url: String,
    /// Tool approval mode (`auto`, `manual`, …).
    #[arg(long, default_value = "auto")]
    approval: String,
    /// Agent role (lead worker id) that runs the instruction. Defaults to
    /// `coder` for this headless runner; the desktop app default is unchanged.
    #[arg(long, default_value = "coder", value_parser = ["general", "coder", "explore", "computer"])]
    agent_role: String,
    /// Thinking intensity for the active provider (`low` | `medium` | `high` |
    /// `max`). Maps onto the provider's thinking strategy (budget / effort / …).
    #[arg(long, default_value = "max", value_parser = ["low", "medium", "high", "max"])]
    thinking_intensity: String,
    /// Echo the model's reasoning (`reasoning_content`) back on assistant
    /// messages sent to the provider. Mirrors the desktop setting
    /// `reasoningInMessages`; defaults to `false`.
    #[arg(
        long,
        default_value_t = false,
        action = clap::ArgAction::Set,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    reasoning_in_messages: bool,
    /// Lead system-prompt profile. `default` keeps the shipped agent prompt;
    /// `minimal` swaps it for a short mini-swe-agent-style prompt and drops the
    /// desktop scaffolding layers (tool system appendix, project context,
    /// memory, user rules, locale rule).
    #[arg(long, value_parser = ["default", "minimal"])]
    prompt_profile: Option<String>,
    /// Replace the lead system prompt with the contents of this file.
    /// Implies `--prompt-profile minimal` unless that flag is given explicitly.
    #[arg(long, value_name = "PATH")]
    system_prompt_file: Option<PathBuf>,
    /// Restrict the run to this comma-separated tool allow list. Entries may be
    /// exact tool ids (`file_read`) or family names (`file`, `terminal`).
    /// When omitted, every registered tool stays available (unchanged default).
    #[arg(long, value_name = "NAME[,NAME...]", value_delimiter = ',')]
    allowed_tools: Option<Vec<String>>,
    /// Hard cap on tool-call rounds for this run (`maxToolRounds`).
    /// When the cap is reached the run stops and the CLI exits `14`.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=10_000))]
    max_turns: Option<u32>,
    /// Wall-clock timeout for the run, in seconds. On expiry the run is
    /// cancelled (killing an in-flight `terminal` child) and the CLI exits `10`.
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    /// Directory for run artifacts (`result.json`). Defaults to the data dir.
    ///
    /// When set explicitly, the CLI also writes the observability artifacts
    /// `events.jsonl`, `agent.log`, `usage.json` and `trajectory.json`
    /// (ATIF v1.7) next to `result.json`.
    #[arg(long, value_name = "PATH")]
    out_dir: Option<PathBuf>,
    /// Mirror the `AgentEvent` stream as JSONL on stdout (`off` | `jsonl`).
    /// With `jsonl`, stdout carries only event lines (the final assistant
    /// reply is not printed; it stays in `result.json`).
    #[arg(long, default_value = "off", value_parser = ["off", "jsonl"])]
    events: String,
    /// Data directory (settings + SQLite). Defaults to a fresh temp dir.
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,
    /// Resolve configuration and exit without calling the model.
    #[arg(long)]
    dry_run: bool,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Some(Command::Run(args)) => run(args),
        None => {
            // No subcommand: print help. `print_help` failure is a usage problem.
            match Cli::command().print_help() {
                Ok(()) => {
                    println!();
                    EXIT_SUCCESS
                }
                Err(err) => {
                    eprintln!("pointer-cli: {err:#}");
                    EXIT_USAGE
                }
            }
        }
    };
    std::process::exit(code);
}

fn run(args: RunArgs) -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("pointer-cli: failed to start the tokio runtime: {err:#}");
            return EXIT_INTERNAL;
        }
    };
    match runtime.block_on(run_async(args)) {
        Ok(code) => code,
        Err(err) => {
            let code = err
                .downcast_ref::<CliError>()
                .map(|e| e.code)
                .unwrap_or(EXIT_INTERNAL);
            eprintln!("pointer-cli: {err:#}");
            code
        }
    }
}

/// Env var read by `pointer_core::agents::LeadPromptOverride` for the profile.
const ENV_PROMPT_PROFILE: &str = "POINTER_PROMPT_PROFILE";
/// Env var read by `pointer_core::agents::LeadPromptOverride` for the file path.
const ENV_SYSTEM_PROMPT_FILE: &str = "POINTER_SYSTEM_PROMPT_FILE";

/// Effective prompt profile: an explicit `--prompt-profile` wins; a bare
/// `--system-prompt-file` implies the minimal layer set.
fn effective_prompt_profile(args: &RunArgs) -> &'static str {
    match args.prompt_profile.as_deref() {
        Some("minimal") => "minimal",
        Some(_) => "default",
        None if args.system_prompt_file.is_some() => "minimal",
        None => "default",
    }
}

/// Export the prompt-profile flags through the environment variables the core
/// prompt assembler reads, validating the override file up front so a typo
/// fails fast with a usage error instead of silently falling back.
fn install_prompt_override(args: &RunArgs) -> Result<&'static str> {
    let profile = effective_prompt_profile(args);
    std::env::set_var(ENV_PROMPT_PROFILE, profile);
    if let Some(path) = args.system_prompt_file.as_ref() {
        let text = std::fs::read_to_string(path).map_err(|err| {
            cli_error(
                EXIT_USAGE,
                format!("failed to read system prompt file {}: {err}", path.display()),
            )
        })?;
        if text.trim().is_empty() {
            return Err(cli_error(
                EXIT_USAGE,
                format!("system prompt file {} is empty", path.display()),
            ));
        }
        let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        std::env::set_var(ENV_SYSTEM_PROMPT_FILE, absolute);
    }
    Ok(profile)
}

fn resolve_instruction(args: &RunArgs) -> Result<String> {
    let text = match (&args.instruction, &args.instruction_file) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|err| {
            cli_error(
                EXIT_USAGE,
                format!(
                    "failed to read instruction file {}: {err}",
                    path.display()
                ),
            )
        })?,
        (None, None) => {
            return Err(cli_error(
                EXIT_USAGE,
                "no instruction: pass --instruction <TEXT> or --instruction-file <PATH>",
            ))
        }
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(cli_error(EXIT_USAGE, "instruction is empty"));
    }
    Ok(text)
}

/// Remove every registered tool that is not permitted by `allowed`.
///
/// Returns `(kept, removed)`, both sorted. Because the model only ever sees
/// tools that are still registered (and `ToolRegistry::invoke` looks up the
/// same map), this filters both visibility and invocation.
fn apply_tool_whitelist(registry: &ToolRegistry, allowed: &[String]) -> (Vec<String>, Vec<String>) {
    let registered: Vec<String> = registry.list_defs().into_iter().map(|def| def.name).collect();
    let mut kept = Vec::new();
    let mut removed = Vec::new();
    for name in registered {
        let permitted = allowed.iter().any(|entry| {
            pointer_core::tools::registry_tool_in_allow_list(std::slice::from_ref(entry), &name)
        });
        if permitted {
            kept.push(name);
        } else {
            registry.unregister(&name);
            removed.push(name);
        }
    }
    kept.sort();
    removed.sort();
    (kept, removed)
}

async fn run_async(args: RunArgs) -> Result<i32> {
    let instruction = resolve_instruction(&args)?;
    // Must be exported before dispatch: the core prompt assembler reads it.
    let prompt_profile = install_prompt_override(&args)?;
    let workspace = std::fs::canonicalize(&args.workspace)
        .with_context(|| format!("workspace not found: {}", args.workspace.display()))?;

    // Isolated data dir — never the user's real PointerApp directory.
    let temp_guard = if args.data_dir.is_none() {
        Some(
            tempfile::Builder::new()
                .prefix("pointer-cli-")
                .tempdir()
                .context("failed to create a temp data dir")?,
        )
    } else {
        None
    };
    let data_dir: PathBuf = match (&args.data_dir, &temp_guard) {
        (Some(path), _) => {
            std::fs::create_dir_all(path)
                .with_context(|| format!("failed to create data dir {}", path.display()))?;
            path.clone()
        }
        (None, Some(guard)) => guard.path().to_path_buf(),
        (None, None) => unreachable!("temp_guard is set when data_dir is None"),
    };

    // These must be set before `AppState::new()` reads them.
    std::env::set_var("POINTER_APP_DATA_DIR", &data_dir);
    std::env::set_var("POINTER_HOME", data_dir.join("home"));
    std::env::set_var("POINTER_DEPLOYMENT_MODE", "standalone");

    let api_key = std::env::var(&args.api_key_env)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or_else(|| data_dir.clone());
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create out dir {}", out_dir.display()))?;

    if args.dry_run {
        let resolved = serde_json::json!({
            "instruction_chars": instruction.chars().count(),
            "workspace": workspace.to_string_lossy(),
            "data_dir": data_dir.to_string_lossy(),
            "out_dir": out_dir.to_string_lossy(),
            "provider": {
                "id": PROVIDER_ID,
                "base_url": args.base_url,
                "model": args.model,
                "api_key_env": args.api_key_env,
                "api_key_present": api_key.is_some(),
            },
            "approval": args.approval,
            "agent_role": args.agent_role,
            "thinking_intensity": args.thinking_intensity,
            "reasoning_in_messages": args.reasoning_in_messages,
            "prompt_profile": prompt_profile,
            "system_prompt_file": args
                .system_prompt_file
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            "allowed_tools": args.allowed_tools,
            "max_turns": args.max_turns,
            "timeout_secs": args.timeout,
            "computer_feature": cfg!(feature = "computer"),
            "exit_codes": {
                "success": EXIT_SUCCESS,
                "usage": EXIT_USAGE,
                "timeout": EXIT_TIMEOUT,
                "auth": EXIT_AUTH,
                "model": EXIT_MODEL,
                "internal": EXIT_INTERNAL,
                "turn_limit": EXIT_TURN_LIMIT,
            },
        });
        println!("{}", serde_json::to_string_pretty(&resolved)?);
        return Ok(EXIT_SUCCESS);
    }

    let api_key = api_key.ok_or_else(|| {
        cli_error(
            EXIT_AUTH,
            format!(
                "environment variable {} is not set; export it with the provider API key",
                args.api_key_env
            ),
        )
    })?;

    // Seed the isolated settings: user layer holds routing, the runtime key is
    // injected through the in-memory platform layer (mirrors login injection).
    let mut user_settings = serde_json::json!({
        "providers": [],
        "activeProviderId": PROVIDER_ID,
        "model": args.model,
        "toolApprovalMode": args.approval,
        "workspaceRoot": workspace.to_string_lossy(),
    });
    if let Some(max_turns) = args.max_turns {
        // `maxToolRounds` is the per-turn tool-call cap enforced by the agent loop.
        user_settings["maxToolRounds"] = serde_json::json!(max_turns);
    }
    std::fs::write(
        data_dir.join("user_settings.json"),
        serde_json::to_vec_pretty(&user_settings)?,
    )
    .context("failed to write user_settings.json")?;

    let state = Arc::new(AppState::new());
    {
        let mut platform = state.platform_config.write();
        platform.providers = vec![ProviderConfig {
            id: PROVIDER_ID.to_string(),
            name: "CLI".to_string(),
            base_url: args.base_url.clone(),
            api_key,
            models: vec![args.model.clone()],
            reasoning_in_messages: Some(args.reasoning_in_messages),
            temperature: None,
            top_p: None,
            max_tokens: None,
            context_budget_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: Some(args.thinking_intensity.clone()),
            source: Some("platform".to_string()),
            extra_body: None,
        }];
    }

    // Normalise the optional tool allow list before touching the registry.
    let allowed_tools: Option<Vec<String>> = match args.allowed_tools.as_ref() {
        Some(list) => {
            let cleaned: Vec<String> = list
                .iter()
                .map(|entry| entry.trim().to_string())
                .filter(|entry| !entry.is_empty())
                .collect();
            if cleaned.is_empty() {
                return Err(cli_error(
                    EXIT_USAGE,
                    "--allowed-tools must name at least one tool",
                ));
            }
            Some(cleaned)
        }
        None => None,
    };

    // Apply the tool allow list (if any) to the freshly built registry, before
    // dispatch: the model-visible tool list and `ToolRegistry::invoke` both read
    // this same map, so unregistered tools can be neither seen nor called.
    let whitelist_report = allowed_tools.as_ref().map(|allowed| {
        let (kept, removed) = apply_tool_whitelist(&state.tools, allowed);
        let unknown: Vec<String> = allowed
            .iter()
            .filter(|entry| {
                !kept.iter().any(|name| {
                    pointer_core::tools::registry_tool_in_allow_list(
                        std::slice::from_ref(entry),
                        name,
                    )
                })
            })
            .cloned()
            .collect();
        if !unknown.is_empty() {
            eprintln!(
                "pointer-cli: warning: --allowed-tools entries matched no registered tool: {}",
                unknown.join(", ")
            );
        }
        eprintln!(
            "pointer-cli: tool whitelist active — kept {} tool(s), removed {}",
            kept.len(),
            removed.len()
        );
        serde_json::json!({
            "requested": allowed,
            "kept": kept,
            "removed": removed,
            "unknown": unknown,
        })
    });

    let conversation_id = format!("cli-{}", std::process::id());
    state
        .session_index
        .ensure_cron_session(&conversation_id, "Pointer Agent CLI")
        .context("failed to create the CLI conversation")?;

    let dispatcher = state.build_dispatcher();

    // Observability: subscribe to the typed AgentEvent bus *before* dispatch so
    // the recorder sees the very first `run_queued` event. New artifacts are
    // only produced when `--out-dir` was given explicitly; `--events jsonl`
    // additionally mirrors the stream to stdout.
    let explicit_out_dir = args.out_dir.is_some();
    let mirror_stdout = args.events == "jsonl";
    let recorder = if explicit_out_dir || mirror_stdout {
        let jsonl_dir = explicit_out_dir.then_some(out_dir.as_path());
        match artifacts::EventRecorder::new(jsonl_dir, mirror_stdout) {
            Ok(recorder) => {
                recorder.start(dispatcher.events());
                Some(recorder)
            }
            Err(err) => {
                eprintln!("pointer-cli: warning: failed to start the event recorder: {err:#}");
                None
            }
        }
    } else {
        None
    };

    let request = TriggerRequest {
        run_id: None,
        idempotency_key: None,
        conversation_id: Some(conversation_id.clone()),
        trigger_source: TriggerSource::HttpRuns,
        trigger_meta: TriggerMeta::default(),
        lane: None,
        messages: vec![ChatMessage::user_text(instruction.clone())],
        enabled_skill_ids: vec![],
        agent_skill_overrides: std::collections::HashMap::new(),
        lead_agent_id: Some(args.agent_role.clone()),
        performance_mode: None,
        tool_rounds_used_single_start: 0,
        workspace_root: workspace.to_string_lossy().to_string(),
        workspace_inherit_disabled: Some(true),
        deliver: DeliverTarget::None,
        web_session_auth: None,
    };

    // Capture the run's terminal tool-round counters so a cap hit is reported
    // as a distinct outcome. `StreamEvent::Done` carries both the consumed
    // rounds and the configured max (`maxToolRounds`).
    let done_counts: Arc<Mutex<Option<(u32, u32)>>> = Arc::new(Mutex::new(None));
    {
        let done_counts = done_counts.clone();
        let watched_conversation = conversation_id.clone();
        pointer_core::stream_broadcast::subscribe_stream(Arc::new(move |item| {
            if item.conversation_id.as_deref() != Some(watched_conversation.as_str()) {
                return;
            }
            if let StreamEvent::Done {
                tool_rounds_used_total,
                max_tool_rounds,
                ..
            } = &item.event
            {
                if let (Some(used), Some(max)) = (tool_rounds_used_total, max_tool_rounds) {
                    *done_counts.lock().unwrap() = Some((*used, *max));
                }
            }
        }));
    }

    let handle = dispatcher
        .dispatch(request)
        .await
        .map_err(|err| cli_error(EXIT_INTERNAL, format!("dispatch failed: {err:#}")))?;
    let run_id = handle.run_id.clone();

    enum Stop {
        Done(RunOutcome),
        Timeout,
    }

    // Race the run against the wall-clock deadline. On expiry we *cancel* the
    // run (not merely stop waiting): the cancel token trips the agent loop's
    // guard and the `terminal` tool's cooperative cancel kills its child tree.
    let stop = tokio::select! {
        biased;
        outcome = dispatcher.wait(&run_id) => Stop::Done(outcome.map_err(|err| {
            cli_error(EXIT_INTERNAL, format!("dispatcher wait failed: {err:#}"))
        })?),
        _ = tokio::time::sleep(Duration::from_secs(args.timeout)) => {
            eprintln!(
                "pointer-cli: run exceeded --timeout {}s; cancelling run {run_id}",
                args.timeout
            );
            dispatcher.cancel(&run_id);
            // `dispatcher.cancel(run_id)` only trips the dispatcher's own token;
            // the agent loop observes the token registered on the conversation
            // (created by `run_chat`), so also cancel the conversation. This is
            // the same path the desktop "stop generation" button uses, and it
            // aborts an in-flight terminal child via the run-abort flag.
            dispatcher.cancel_conversation(&conversation_id, true);
            // Give the runner a moment to trip its cancel guard and reap a
            // cooperatively-cancelled tool child before we exit regardless.
            let _ = tokio::time::timeout(CANCEL_GRACE, dispatcher.wait(&run_id)).await;
            match dispatcher.run_status(&run_id) {
                Some(record) if record.status == "finished" => Stop::Done(RunOutcome::Finished {
                    run_id: run_id.clone(),
                    conversation_id: conversation_id.clone(),
                }),
                _ => Stop::Timeout,
            }
        }
    };

    let (used, max) = done_counts.lock().unwrap().unwrap_or((0, 0));
    let turn_limit_hit = max > 0 && used >= max;

    let (status, error, code) = match &stop {
        Stop::Timeout => ("timeout", None, EXIT_TIMEOUT),
        Stop::Done(RunOutcome::Finished { .. }) => ("finished", None, EXIT_SUCCESS),
        Stop::Done(RunOutcome::Cancelled { .. }) => ("cancelled", None, EXIT_INTERNAL),
        Stop::Done(RunOutcome::Failed { error, .. }) => {
            if turn_limit_hit {
                ("turn_limit", Some(error.clone()), EXIT_TURN_LIMIT)
            } else {
                ("failed", Some(error.clone()), classify_run_failure(error))
            }
        }
    };

    let messages = state
        .session_index
        .load_messages(&conversation_id)
        .unwrap_or_default();
    let assistant_reply = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::Assistant) && !m.content.trim().is_empty())
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // Flush the event recorder, then write the observability artifacts. These
    // are a side channel: a write failure is reported but never changes the
    // run's semantic exit code.
    if let Some(recorder) = recorder.as_ref() {
        recorder.stop().await;
    }
    let usage = if explicit_out_dir {
        artifacts::load_usage(&conversation_id)
    } else {
        artifacts::UsageSummary::default()
    };
    if explicit_out_dir {
        let events = recorder.as_ref().map(|r| r.events()).unwrap_or_default();
        if let Err(err) = write_observability_artifacts(
            &out_dir,
            &conversation_id,
            &run_id,
            &args.model,
            status,
            code,
            error.as_deref(),
            &instruction,
            &messages,
            &usage,
            &events,
        ) {
            eprintln!("pointer-cli: warning: failed to write observability artifacts: {err:#}");
        }
    }

    let result = serde_json::json!({
        "conversation_id": conversation_id,
        "run_id": run_id,
        "status": status,
        "error": error,
        "exit_code": code,
        "assistant_reply": assistant_reply,
        "tool_rounds_used": used,
        "max_tool_rounds": max,
        "max_turns": args.max_turns,
        "timeout_secs": args.timeout,
        "agent_role": args.agent_role,
        "thinking_intensity": args.thinking_intensity,
        "reasoning_in_messages": args.reasoning_in_messages,
        "prompt_profile": prompt_profile,
        "allowed_tools": whitelist_report,
    });
    let result_path = out_dir.join("result.json");
    std::fs::write(&result_path, serde_json::to_vec_pretty(&result)?)
        .with_context(|| format!("failed to write {}", result_path.display()))?;

    // With `--events jsonl`, stdout is a pure event stream (the reply stays in
    // `result.json`) so downstream JSONL parsers are not corrupted.
    if !mirror_stdout && !assistant_reply.is_empty() {
        println!("{assistant_reply}");
    }
    if let Some(error) = error.as_deref() {
        eprintln!("pointer-cli: {status}: {error}");
    }
    eprintln!(
        "pointer-cli: {status} (exit {code}; result written to {})",
        result_path.display()
    );

    Ok(code)
}

/// Write the four observability artifacts for an explicit `--out-dir`.
#[allow(clippy::too_many_arguments)]
fn write_observability_artifacts(
    out_dir: &std::path::Path,
    conversation_id: &str,
    run_id: &str,
    model: &str,
    status: &str,
    exit_code: i32,
    error: Option<&str>,
    instruction: &str,
    messages: &[ChatMessage],
    usage: &artifacts::UsageSummary,
    events: &[(i64, serde_json::Value)],
) -> Result<()> {
    let meta = artifacts::RunMeta {
        conversation_id,
        run_id,
        model,
        status,
        exit_code,
        error,
    };
    let usage_path = artifacts::write_usage_json(out_dir, &meta, usage)?;
    let log_path = artifacts::write_agent_log(out_dir, events, &meta, usage)?;
    let trajectory = artifacts::build_trajectory(&artifacts::TrajectoryContext {
        instruction,
        conversation_id,
        run_id,
        model,
        status,
        exit_code,
        messages,
        usage,
    });
    let trajectory_path = artifacts::write_trajectory(out_dir, &trajectory)?;
    eprintln!(
        "pointer-cli: artifacts written — {}, {}, {}",
        usage_path.display(),
        log_path.display(),
        trajectory_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn exit_code_classification_is_stable() {
        assert_eq!(classify_run_failure("HTTP 401 Unauthorized"), EXIT_AUTH);
        assert_eq!(classify_run_failure("invalid_api_key"), EXIT_AUTH);
        assert_eq!(classify_run_failure("HTTP 404 model_not_found"), EXIT_MODEL);
        assert_eq!(classify_run_failure("HTTP 429 rate limit"), EXIT_MODEL);
        assert_eq!(classify_run_failure("something exploded"), EXIT_INTERNAL);
        assert_eq!(EXIT_SUCCESS, 0);
        assert_eq!(EXIT_USAGE, 2);
        assert_eq!(EXIT_TIMEOUT, 10);
        assert_eq!(EXIT_TURN_LIMIT, 14);
    }

    /// Parse `run` with a minimal instruction plus any extra flags.
    fn parse_run(extra: &[&str]) -> RunArgs {
        let mut argv: Vec<&str> = vec!["pointer-cli", "run", "--instruction", "hello"];
        argv.extend_from_slice(extra);
        match Cli::try_parse_from(argv).expect("args should parse").command {
            Some(Command::Run(args)) => args,
            None => panic!("expected the run subcommand"),
        }
    }

    #[test]
    fn agent_role_and_thinking_intensity_default_to_coder_and_max() {
        let args = parse_run(&[]);
        assert_eq!(args.agent_role, "coder");
        assert_eq!(args.thinking_intensity, "max");
    }

    #[test]
    fn agent_role_and_thinking_intensity_can_be_overridden() {
        let args = parse_run(&["--agent-role", "explore", "--thinking-intensity", "high"]);
        assert_eq!(args.agent_role, "explore");
        assert_eq!(args.thinking_intensity, "high");
    }

    #[test]
    fn reasoning_in_messages_defaults_to_false_and_can_be_set() {
        let args = parse_run(&[]);
        assert!(!args.reasoning_in_messages);

        let args = parse_run(&["--reasoning-in-messages", "true"]);
        assert!(args.reasoning_in_messages);

        let args = parse_run(&["--reasoning-in-messages", "false"]);
        assert!(!args.reasoning_in_messages);
    }

    #[test]
    fn invalid_agent_role_and_thinking_intensity_are_rejected() {
        assert!(Cli::try_parse_from([
            "pointer-cli",
            "run",
            "--instruction",
            "hello",
            "--agent-role",
            "nope",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pointer-cli",
            "run",
            "--instruction",
            "hello",
            "--thinking-intensity",
            "ultra",
        ])
        .is_err());
    }

    #[test]
    fn prompt_profile_defaults_to_unset_and_accepts_known_values() {
        let args = parse_run(&[]);
        assert!(args.prompt_profile.is_none());
        assert!(args.system_prompt_file.is_none());
        assert_eq!(effective_prompt_profile(&args), "default");

        let args = parse_run(&["--prompt-profile", "minimal"]);
        assert_eq!(args.prompt_profile.as_deref(), Some("minimal"));
        assert_eq!(effective_prompt_profile(&args), "minimal");

        assert!(Cli::try_parse_from([
            "pointer-cli",
            "run",
            "--instruction",
            "hello",
            "--prompt-profile",
            "verbose",
        ])
        .is_err());
    }

    #[test]
    fn system_prompt_file_implies_minimal_unless_overridden() {
        let args = parse_run(&["--system-prompt-file", "/tmp/pointer-prompt.md"]);
        assert_eq!(effective_prompt_profile(&args), "minimal");

        let args = parse_run(&[
            "--system-prompt-file",
            "/tmp/pointer-prompt.md",
            "--prompt-profile",
            "default",
        ]);
        assert_eq!(effective_prompt_profile(&args), "default");
    }

    #[test]
    fn install_prompt_override_rejects_missing_or_empty_files() {
        let args = parse_run(&["--system-prompt-file", "/nonexistent/pointer-prompt.md"]);
        let err = install_prompt_override(&args).expect_err("missing file must fail");
        assert!(err.to_string().contains("failed to read system prompt file"));

        let dir = tempfile::tempdir().expect("temp dir");
        let empty = dir.path().join("empty.md");
        std::fs::write(&empty, "   \n").expect("write empty prompt");
        let args = parse_run(&["--system-prompt-file", empty.to_str().unwrap()]);
        let err = install_prompt_override(&args).expect_err("empty file must fail");
        assert!(err.to_string().contains("is empty"));
    }

    #[test]
    fn install_prompt_override_exports_profile_and_absolute_file_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let prompt = dir.path().join("prompt.md");
        std::fs::write(&prompt, "You are a tiny agent.").expect("write prompt");
        let args = parse_run(&["--system-prompt-file", prompt.to_str().unwrap()]);
        let profile = install_prompt_override(&args).expect("override installs");
        assert_eq!(profile, "minimal");
        assert_eq!(
            std::env::var(ENV_PROMPT_PROFILE).as_deref(),
            Ok("minimal")
        );
        let exported = std::env::var(ENV_SYSTEM_PROMPT_FILE).expect("file env exported");
        assert!(std::path::Path::new(&exported).is_absolute());
        assert_eq!(
            std::fs::read_to_string(&exported).expect("exported path readable"),
            "You are a tiny agent."
        );
        // Reset so sibling tests see the default profile.
        std::env::remove_var(ENV_PROMPT_PROFILE);
        std::env::remove_var(ENV_SYSTEM_PROMPT_FILE);
    }

    #[test]
    fn tool_whitelist_keeps_exact_ids_and_families() {
        let registry = ToolRegistry::new();
        let store = Arc::new(pointer_core::task_board::TaskBoardStore::new());
        pointer_core::tools::builtin::register_all(&registry, store);
        assert!(
            registry.list_defs().len() > 5,
            "expected the builtin registry to be populated"
        );

        let allowed = vec!["terminal".to_string(), "file_read".to_string()];
        let (kept, removed) = apply_tool_whitelist(&registry, &allowed);
        assert!(kept.contains(&"terminal".to_string()));
        assert!(kept.contains(&"file_read".to_string()));
        assert!(!kept.contains(&"web_search".to_string()));
        assert!(removed.contains(&"web_search".to_string()));
        assert!(!removed.contains(&"terminal".to_string()));
        // Invocation lookup must also fail for a removed tool.
        assert!(registry.get_def("web_search").is_none());
        assert!(registry.get_def("terminal").is_some());

        // Family name `file` expands to the whole file_* family.
        let registry = ToolRegistry::new();
        let store = Arc::new(pointer_core::task_board::TaskBoardStore::new());
        pointer_core::tools::builtin::register_all(&registry, store);
        let (kept, _) = apply_tool_whitelist(&registry, &["file".to_string()]);
        assert!(kept.iter().all(|name| name.starts_with("file_")));
        assert!(kept.contains(&"file_read".to_string()));
    }
}

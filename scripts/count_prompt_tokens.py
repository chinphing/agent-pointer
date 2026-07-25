#!/usr/bin/env python3
"""Count Pointer prompt tokens with cl100k_base (tiktoken, same BPE family as Qwen3.5).

No transformers / modelscope dependency needed → pip install tiktoken pyyaml only.

Evaluates three agent profiles:

- **Computer** (Primary tier) — direct single-agent with images
- **Coder** — direct single-agent (no images)
- **Explore** — sub-agent (read-only)

Static slices read from a git ref (default HEAD). Dynamic image tokens use the
official Qwen3.x vision formula: (h_bar * w_bar) / (32 * 32) + 2.

Usage:
  python3 scripts/count_prompt_tokens.py              # print summary
  python3 scripts/count_prompt_tokens.py --json       # machine-readable metrics
  python3 scripts/count_prompt_tokens.py --update-baseline  # refresh baseline files

Requires:
  pip install tiktoken pyyaml
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "scripts"))
from compare_prompt_sizes import _appendix_sizes, git_show  # noqa: E402

DEFAULT_REF = "HEAD"
IMAGE_W, IMAGE_H = 1920, 1080

SLOT_BEFORE = "[Screen before action]"
SLOT_AFTER = "[Screen after action]"
SLOT_ANNOTATED = "[Screen annotated]"

# ── Tokenizer ────────────────────────────────────────────────────────────────

_tokenizer = None


def get_tokenizer():
    global _tokenizer
    if _tokenizer is None:
        import tiktoken
        _tokenizer = tiktoken.get_encoding("cl100k_base")
    return _tokenizer


def count_tokens(text: str) -> int:
    if not text:
        return 0
    return len(get_tokenizer().encode(text, disallowed_special=()))


# ── Computer Primary tier tool docs ──────────────────────────────────────────

TOOL_FILES = [
    "crates/pointer-core/src/agents/computer/tools/prompts/mouse.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/input.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/modified_click.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/captcha_verify.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/clipboard.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/hotkey.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/wait.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/action_verify.md",
    "crates/pointer-core/src/task_board/prompts/task_board.md",
]

NATIVE_TOOL_FAMILIES: list[tuple[str, str, str | None]] = [
    ("mouse", "crates/pointer-core/src/agents/computer/tools/prompts/mouse.md", "mouse.schema.yaml"),
    ("input", "crates/pointer-core/src/agents/computer/tools/prompts/input.md", "input.schema.yaml"),
    ("modified_click", "crates/pointer-core/src/agents/computer/tools/prompts/modified_click.md", "modified_click.schema.yaml"),
    ("captcha_verify", "crates/pointer-core/src/agents/computer/tools/prompts/captcha_verify.md", "captcha_verify.schema.yaml"),
    ("clipboard", "crates/pointer-core/src/agents/computer/tools/prompts/clipboard.md", "clipboard.schema.yaml"),
    ("hotkey", "crates/pointer-core/src/agents/computer/tools/prompts/hotkey.md", None),
    ("wait", "crates/pointer-core/src/agents/computer/tools/prompts/wait.md", None),
    ("action_verify", "crates/pointer-core/src/agents/computer/tools/prompts/action_verify.md", None),
    ("task_board", "crates/pointer-core/src/task_board/prompts/task_board.md", "task_board.schema.yaml"),
]

# ── Coder / Explore tool docs (system Tools appendix) ────────────────────────

CODER_TOOL_DOCS = [
    "crates/pointer-core/src/tools/prompts/file.md",
    "crates/pointer-core/src/tools/prompts/terminal.md",
    "crates/pointer-core/src/tools/prompts/run_subagent.md",
    "crates/pointer-core/src/tools/prompts/web_search.md",
    "crates/pointer-core/src/agents/coder/prompts/read_lints.md",
    "crates/pointer-core/src/task_board/prompts/task_board.md",
]

EXPLORE_TOOL_DOCS = [
    "crates/pointer-core/src/tools/prompts/file.md",
    "crates/pointer-core/src/task_board/prompts/task_board.md",
]

# ── Native tools known per agent (from AGENT.md allowTools) ─────────────────

# Each entry: (tool_name, doc_source_md_path, schema_yaml_path | None)
# When schema_yaml_path is None → schema is parsed from frontmatter of the doc.

CODER_NATIVE_TOOLS: list[tuple[str, str, str | None]] = [
    # Flat file tools — share file.md, schema from file.schema.yaml
    ("file_read",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_write", "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_edit",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_glob",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_grep",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_list",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    # Single-tool — own doc, schema from frontmatter
    ("terminal",    "crates/pointer-core/src/tools/prompts/terminal.md", None),
    ("read_lints",  "crates/pointer-core/src/agents/coder/prompts/read_lints.md", None),
    ("run_subagent", "crates/pointer-core/src/tools/prompts/run_subagent.md", None),
    ("web_search",  "crates/pointer-core/src/tools/prompts/web_search.md", None),
    # Task board — share task_board.md, schema from task_board.schema.yaml
    ("task_board_init",          "crates/pointer-core/src/task_board/prompts/task_board.md", "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"),
    ("task_board_replace",       "crates/pointer-core/src/task_board/prompts/task_board.md", "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"),
    ("task_board_patch",         "crates/pointer-core/src/task_board/prompts/task_board.md", "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"),
    ("task_board_prune",         "crates/pointer-core/src/task_board/prompts/task_board.md", "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"),
    ("task_board_finalize",      "crates/pointer-core/src/task_board/prompts/task_board.md", "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"),
    ("task_board_check_deps",    "crates/pointer-core/src/task_board/prompts/task_board.md", "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"),
]

EXPLORE_NATIVE_TOOLS: list[tuple[str, str, str | None]] = [
    # From AGENT.md: only file tools for Explore (read-only sub-agent)
    ("file_read",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_write", "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_edit",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_glob",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_grep",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
    ("file_list",  "crates/pointer-core/src/tools/prompts/file.md", "crates/pointer-core/src/tools/prompts/file.schema.yaml"),
]

# ── Coder composed body slices (order matches mod.rs composed_system_body) ───

CODER_SLICES = [
    ("role", "crates/pointer-core/src/agents/coder/prompts/role.md"),
    ("routine", "crates/pointer-core/src/agents/coder/prompts/flow/routine.md"),
    ("delegation", "crates/pointer-core/src/agents/coder/prompts/delegation.md"),
    ("task_board", "crates/pointer-core/src/agents/coder/prompts/task_board.md"),
    ("scenario_impl", "crates/pointer-core/src/agents/coder/prompts/scenarios/implementation.md"),
    ("scenario_debug", "crates/pointer-core/src/agents/coder/prompts/scenarios/debugging.md"),
    ("scenario_refactor", "crates/pointer-core/src/agents/coder/prompts/scenarios/refactor.md"),
    ("scenario_design", "crates/pointer-core/src/agents/coder/prompts/scenarios/design_only.md"),
    ("scenario_spec", "crates/pointer-core/src/agents/coder/prompts/scenarios/spec_audit.md"),
]

# ── Explore composed body slices (order matches mod.rs composed_system_body) ─

EXPLORE_SLICES = [
    ("role", "crates/pointer-core/src/agents/explore/prompts/role.md"),
    ("router", "crates/pointer-core/src/agents/explore/prompts/flow/router.md"),
    ("flow_standard", "crates/pointer-core/src/agents/explore/prompts/flow/standard.md"),
    ("flow_narrow", "crates/pointer-core/src/agents/explore/prompts/flow/fast_narrow.md"),
    ("flow_reach", "crates/pointer-core/src/agents/explore/prompts/flow/fast_reachability.md"),
    ("scenario_single", "crates/pointer-core/src/agents/explore/prompts/scenarios/single_module_fix.md"),
    ("scenario_cross", "crates/pointer-core/src/agents/explore/prompts/scenarios/cross_module_change.md"),
    ("scenario_arch", "crates/pointer-core/src/agents/explore/prompts/scenarios/architecture_explain.md"),
    ("scenario_reach", "crates/pointer-core/src/agents/explore/prompts/scenarios/reachability_audit.md"),
    ("scenario_spec", "crates/pointer-core/src/agents/explore/prompts/scenarios/spec_map.md"),
    ("scenario_debug", "crates/pointer-core/src/agents/explore/prompts/scenarios/production_debug.md"),
    ("scenario_design", "crates/pointer-core/src/agents/explore/prompts/scenarios/design_only.md"),
    ("impact_scan", "crates/pointer-core/src/agents/_shared/exploration/impact_scan.md"),
    ("handoff_contract", "crates/pointer-core/src/agents/_shared/exploration/handoff_contract.md"),
    ("trace_when", "crates/pointer-core/src/agents/_shared/exploration/trace_when.md"),
    ("file_discipline", "crates/pointer-core/src/agents/_shared/exploration/file_discipline.md"),
    ("deliverable", "crates/pointer-core/src/agents/explore/prompts/deliverable.md"),
]


def qwen3_image_tokens(width: int, height: int) -> tuple[int, int, int]:
    h_bar = round(height / 32) * 32
    w_bar = round(width / 32) * 32
    tokens = (h_bar * w_bar) // (32 * 32) + 2
    return tokens, w_bar, h_bar


def schema_from_md_frontmatter(md: str):
    import yaml
    s = md.lstrip("\ufeff")
    if not s.startswith("---\n"):
        return None
    rest = s[4:]
    end = rest.find("\n---\n")
    if end < 0:
        return None
    doc = yaml.safe_load(rest[:end])
    if isinstance(doc, dict):
        return doc.get("schema")
    return None


def load_schema_yaml(ref: str, path: str) -> list[tuple[str, dict]]:
    import yaml
    text = git_show(ref, path)
    if not text:
        return []
    data = yaml.safe_load(text)
    if isinstance(data, dict):
        return [(str(k), v) for k, v in data.items()]
    return []


def openai_description(name: str, doc: str, peers_sharing_doc_source: int) -> str:
    if peers_sharing_doc_source > 1:
        return f"{name}: parameters in schema; full usage in system Tools appendix."
    t = doc.strip()
    if not t:
        return f"{name}: parameters in schema; full usage in system Tools appendix."
    if len(t) <= 240:
        return t
    return f"{name}: parameters in schema; full usage in system Tools appendix."


# ── Compose body from slices (mirrors join_agent_prompt_sections) ────────────


def _join_sections(texts: list[str]) -> str:
    parts = [t.strip() for t in texts if t.strip()]
    return "\n\n---\n\n".join(parts)


def build_composed_body(ref: str, slices: list[tuple[str, str]], static_header: str | None = None) -> str:
    texts = []
    for _label, path in slices:
        texts.append(git_show(ref, path))
    if static_header:
        texts.insert(len(slices) // 2, static_header)
    return _join_sections(texts)


def coder_composed_body(ref: str) -> str:
    return build_composed_body(ref, CODER_SLICES, "## Scenario playbooks")


def explore_composed_body(ref: str) -> str:
    return build_composed_body(ref, EXPLORE_SLICES, "## Scenario playbooks")


# ── Agent prompt wrapper ─────────────────────────────────────────────────────


def agent_prompt_header(agent_id: str, name: str, role: str, profile: str, description: str) -> str:
    return (
        f"Active agent:\n"
        f"- id: {agent_id}\n"
        f"- name: {name}\n"
        f"- role: {role}\n"
        f"- profile: {profile}\n"
        f"- description: {description}\n\n"
    )


CODER_ID = "coder"
CODER_NAME = "vibe-coding"
CODER_ROLE = "worker"
CODER_PROFILE = "Coder"
CODER_DESC = "Code generation, debugging, explanation, refactoring, and engineering implementation."

EXPLORE_ID = "explore"
EXPLORE_NAME = "Explore Agent"
EXPLORE_ROLE = "worker"
EXPLORE_PROFILE = "Explore"
EXPLORE_DESC = (
    "Read-only codebase reconnaissance: map symbols, callers/callees, and data flow. "
    "Deliver a structured Markdown digest in final assistant content for the parent. "
    "Use via run_subagent when the lead thread risks context bloat from many grep/read rounds, "
    "or when a self-contained instruction can state goal, scope, completion criteria, and optional lead facts."
)


# ── Coder system prompt builder ──────────────────────────────────────────────


def build_coder_system_prompt(ref: str) -> tuple[str, list[dict]]:
    comm = git_show(ref, "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md")
    media = git_show(ref, "crates/pointer-core/src/agents/_shared/MEDIA_DELIVERY.md")
    coder_comm = git_show(ref, "crates/pointer-core/src/agents/coder/COMMUNICATION.md")
    body = coder_composed_body(ref)

    if coder_comm.strip() and body.strip():
        system_body = f"{coder_comm.strip()}\n\n---\n\n{body.strip()}"
    elif body.strip():
        system_body = body.strip()
    else:
        system_body = coder_comm.strip()

    wrapped = agent_prompt_header(CODER_ID, CODER_NAME, CODER_ROLE, CODER_PROFILE, CODER_DESC) + system_body

    tool_docs = []
    for path in CODER_TOOL_DOCS:
        md = git_show(ref, path)
        if md.strip():
            tool_docs.append(md.strip())
    tools_block = "## Tools\n\n" + "\n\n".join(tool_docs) if tool_docs else ""

    env_stub = "x" * 530

    sections = [
        {"name": "COMMUNICATION_PUBLIC", "path": "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md", "chars": len(comm), "tokens": count_tokens(comm)},
        {"name": "MEDIA_DELIVERY", "path": "crates/pointer-core/src/agents/_shared/MEDIA_DELIVERY.md", "chars": len(media), "tokens": count_tokens(media)},
        {"name": "agent_prompt(compose_system_prompt(COMMUNICATION.md, composed_system_body()))", "path": "runtime merge", "chars": len(wrapped), "tokens": count_tokens(wrapped)},
        {"name": "Tools appendix (deduped)", "path": f"{len(CODER_TOOL_DOCS)} tool doc files", "chars": len(tools_block), "tokens": count_tokens(tools_block)},
        {"name": "[Environment] stub (~530 chars)", "path": "runtime-built", "chars": len(env_stub), "tokens": count_tokens(env_stub)},
    ]

    all_parts = [comm, media, wrapped, tools_block, env_stub]
    merged = "\n\n".join(p for p in all_parts if p.strip())
    return merged, sections


# ── Explore system prompt builder ────────────────────────────────────────────


def explore_sub_agent_header(ref: str) -> str:
    comm = git_show(ref, "crates/pointer-core/src/agents/explore/COMMUNICATION.md")
    body = explore_composed_body(ref)

    if comm.strip() and body.strip():
        system_prompt = f"{comm.strip()}\n\n---\n\n{body.strip()}"
    elif body.strip():
        system_prompt = body.strip()
    else:
        system_prompt = comm.strip()

    allowed_tools = (
        "file_read, file_glob, file_grep, file_list, "
        "task_board_init, task_board_patch, task_board_replace, task_board_prune, "
        "task_board_finalize, task_board_check_deps"
    )
    header = (
        f"Sub-agent: {EXPLORE_NAME} ({EXPLORE_ID})\n"
        f"profile: {EXPLORE_PROFILE}\n"
        f"description: {EXPLORE_DESC}\n\n"
        f"{system_prompt}\n\n"
        f"Complete only the subtask delivered in the next user message from the Supervisor. "
        f"That message is task instructions (it may include a digest of prior task outputs) and does "
        f"**not** include the main chat history. Finish by writing your full handoff directly in "
        f"assistant Markdown content (conclusions, evidence, traces, open questions). When no further "
        f"tool calls are required, the run ends and the lead reads the final assistant content from "
        f"**`run_subagent`** result field **`content`**.\n"
        f"Allowed tools: {allowed_tools}"
    )
    return header


def build_explore_system_prompt(ref: str) -> tuple[str, list[dict]]:
    comm = git_show(ref, "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md")
    media = git_show(ref, "crates/pointer-core/src/agents/_shared/MEDIA_DELIVERY.md")
    sub_header = explore_sub_agent_header(ref)

    tool_docs = []
    for path in EXPLORE_TOOL_DOCS:
        md = git_show(ref, path)
        if md.strip():
            tool_docs.append(md.strip())
    tools_block = "## Tools\n\n" + "\n\n".join(tool_docs) if tool_docs else ""

    env_stub = "x" * 530

    sections = [
        {"name": "COMMUNICATION_PUBLIC", "path": "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md", "chars": len(comm), "tokens": count_tokens(comm)},
        {"name": "MEDIA_DELIVERY", "path": "crates/pointer-core/src/agents/_shared/MEDIA_DELIVERY.md", "chars": len(media), "tokens": count_tokens(media)},
        {"name": "sub_agent_header (COMMUNICATION.md + composed_system_body)", "path": "runtime", "chars": len(sub_header), "tokens": count_tokens(sub_header)},
        {"name": "Tools appendix (deduped)", "path": f"{len(EXPLORE_TOOL_DOCS)} tool doc files", "chars": len(tools_block), "tokens": count_tokens(tools_block)},
        {"name": "[Environment] stub (~530 chars)", "path": "runtime-built", "chars": len(env_stub), "tokens": count_tokens(env_stub)},
    ]

    all_parts = [comm, media, sub_header, tools_block, env_stub]
    merged = "\n\n".join(p for p in all_parts if p.strip())
    return merged, sections


# ── Sliced detail helpers ────────────────────────────────────────────────────


def _sliced_detail(ref: str, slices: list[tuple[str, str]],
                   comm_path: str, static_header: str | None = None) -> list[dict]:
    comm = git_show(ref, comm_path)
    items = [{"label": "COMMUNICATION.md", "path": comm_path, "chars": len(comm),
              "tokens": count_tokens(comm)}]
    for label, path in slices:
        text = git_show(ref, path)
        items.append({"label": label, "path": path, "chars": len(text),
                       "tokens": count_tokens(text)})
    if static_header:
        hdr = "## Scenario playbooks"
        items.append({"label": "playbooks_header", "path": "(static)", "chars": len(hdr),
                       "tokens": count_tokens(hdr)})
    return items


def coder_sliced_detail(ref: str) -> list[dict]:
    return _sliced_detail(ref, CODER_SLICES,
                          "crates/pointer-core/src/agents/coder/COMMUNICATION.md",
                          "## Scenario playbooks")


def explore_sliced_detail(ref: str) -> list[dict]:
    return _sliced_detail(ref, EXPLORE_SLICES,
                          "crates/pointer-core/src/agents/explore/COMMUNICATION.md",
                          "## Scenario playbooks")


# ═══════════════════════════════════════════════════════════════════════════════
# Computer Primary functions (unchanged logic)
# ═══════════════════════════════════════════════════════════════════════════════


def build_native_openai_tools(ref: str) -> list[dict]:
    family_docs: dict[str, list[str]] = {}
    for _family, md_path, schema_file in NATIVE_TOOL_FAMILIES:
        if schema_file:
            if schema_file == "task_board.schema.yaml":
                schema_path = "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"
            else:
                schema_path = f"crates/pointer-core/src/agents/computer/tools/prompts/{schema_file}"
            for tool_name, _ in load_schema_yaml(ref, schema_path):
                family_docs.setdefault(md_path, []).append(tool_name)
        else:
            name = Path(md_path).stem
            if "action_verify" in md_path:
                name = "action_verify"
            family_docs.setdefault(md_path, []).append(name)

    tools: list[dict] = []
    for _family, md_path, schema_file in NATIVE_TOOL_FAMILIES:
        doc = git_show(ref, md_path)
        peers = len(family_docs.get(md_path, []))
        if schema_file:
            if schema_file == "task_board.schema.yaml":
                schema_path = "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"
            else:
                schema_path = f"crates/pointer-core/src/agents/computer/tools/prompts/{schema_file}"
            for name, schema in load_schema_yaml(ref, schema_path):
                tools.append({
                    "type": "function",
                    "function": {"name": name, "description": openai_description(name, doc, peers), "parameters": schema},
                })
        else:
            schema = schema_from_md_frontmatter(doc)
            if not isinstance(schema, dict):
                raise RuntimeError(f"Missing schema frontmatter in {md_path}")
            name = {"action_verify": "action_verify", "hotkey": "hotkey", "wait": "wait"}.get(Path(md_path).stem, Path(md_path).stem)
            tools.append({
                "type": "function",
                "function": {"name": name, "description": openai_description(name, doc, peers), "parameters": schema},
            })

    tools.sort(key=lambda t: t["function"]["name"])
    return tools


def tools_appendix_text(ref: str) -> str:
    body = "\n\n".join(git_show(ref, p) for p in TOOL_FILES)
    return f"## Tools\n\n{body}"


def build_runtime_tier_slice(ref: str) -> str:
    comm = git_show(ref, "crates/pointer-core/src/agents/computer/prompts/tiers/primary/communication.md")
    os_md = git_show(ref, "crates/pointer-core/src/agents/computer/prompts/os/macos.md")
    loop_md = git_show(ref, "crates/pointer-core/src/agents/computer/prompts/tiers/primary/loop.md")
    return f"{comm}\n\n---\n\n{os_md}\n\n---\n\n{loop_md}"


def build_static_primary(ref: str) -> dict:
    env_stub = "x" * 530
    comm_public = git_show(ref, "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md")
    tier_slice = build_runtime_tier_slice(ref)
    tools_text = tools_appendix_text(ref)

    sections = [
        {"name": "COMMUNICATION_PUBLIC", "path": "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md", "chars": len(comm_public), "tokens": count_tokens(comm_public)},
        {"name": "tier slice (communication + os + loop)", "path": "runtime merge", "chars": len(tier_slice), "tokens": count_tokens(tier_slice)},
        {"name": "Tools appendix (deduped)", "path": "(see TOOL_FILES)", "chars": len(tools_text), "tokens": count_tokens(tools_text)},
        {"name": "[Environment] stub (~530 chars)", "path": "runtime-built", "chars": len(env_stub), "tokens": count_tokens(env_stub)},
    ]
    merged = "\n\n".join([comm_public, tier_slice, tools_text, env_stub])
    nodup, ded = _appendix_sizes(ref)
    return {
        "sections": sections,
        "merged_chars": len(merged),
        "merged_tokens": count_tokens(merged),
        "appendix_chars_nodup": nodup,
        "appendix_chars_dedup": ded,
    }


def build_cur_screen_preamble_primary(has_before: bool) -> str:
    cite = (
        "Each screenshot below is preceded by its slot label on its own line. "
        "Treat only what you see in that labeled image as ground truth — when reasoning internally, "
        "cite **On [slot name]:**; do not invent UI from task text or prior turns. "
        "Do not write internal checklists in assistant message text. "
        "When the user must see a reply (question, blockage, completion), write plain text in **content** "
        "in the same turn — reasoning alone is invisible to the user."
    )
    before_note = f"optional {SLOT_BEFORE}, " if has_before else ""
    return (
        f"[CUR_SCREEN] Primary uses two or three labeled images this turn: {before_note}"
        f"then {SLOT_AFTER}, then {SLOT_ANNOTATED}. {cite} "
        "Text below includes **Pointer position** and **Nearby overlay reference bboxes** "
        "(10 nearest the pointer; session 0–1000 rects). "
        "**Verify:** compare before/after first; if first capture, before is n/a. "
        "**Next:** judge **N–target relation** (inner-center-wrap / inner-edge-wrap / unwrapped), "
        "then choose index or coordinate route.\n"
    )


def build_tier_history_block(history_rows: int) -> str:
    if history_rows <= 0:
        return ""
    lines = [
        "[Recent desktop tool calls — ordered oldest to newest; repetition uses goal; "
        "coordinates are session 0-1000; overlay indices are not comparable across turns.]",
        "Verify suffix: only the newest row may show verify: verifying; verify: skipped = never verified; "
        "verify: verified - * = closed — do not re-verify or re-report.",
    ]
    row = '  {{i}}: mouse_click_index goal="Save document" action="click Save" at (512, 384)'
    for i in range(1, history_rows + 1):
        suffix = " | verify: verifying" if i == history_rows else " | verify: verified - pass"
        lines.append(row.format(i=i) + suffix)
    return "\n".join(lines)


def build_pointer_anchor_block() -> str:
    pointer = (
        "**Pointer position** (capture pixels): (960, 540); normalized (500, 500) on 0–1000.\n\n"
        "**Nearby overlay reference bboxes** (10 indices nearest the **pointer** on this capture — "
        "session 0-1000; each row **R: (left, top, right, bottom)**; use **`index`** for index tools "
        "(bbox center); sorted nearest-first; digits are **labels only**, not click targets):"
    )
    rows = "\n".join(f"- {i}: ({100 + i * 20}, {200 + i * 5}, {180 + i * 20}, {280 + i * 5})" for i in range(1, 11))
    tail = (
        "Image-grounded analysis: cite **On [slot name]:** internally. **Nearby** rows must copy a bullet "
        "below character-for-character — if **`- R:`** is missing, **Inject match: NOT FOUND** and "
        "**hover_index** only; **forbidden** inventing **(left, top, right, bottom)**. **Verify:** judge "
        "**Expected vs Actual UI change** — pointer on target is **not** pass for click/copy goals. "
        "Overlay digits label bboxes only — **forbidden** treating digit position as the click point. "
        "Do not write reasoning in assistant message text. Follow **communication** rules."
    )
    return f"{pointer}\n{rows}\n\n{tail}"


def image_slot_label_tokens(image_count: int, has_before: bool) -> int:
    labels: list[str] = []
    if has_before:
        labels.append(SLOT_BEFORE)
    labels.append(SLOT_AFTER)
    labels.append(SLOT_ANNOTATED)
    labels = labels[:image_count]
    return sum(count_tokens(f"{lab}\n") for lab in labels)


def build_dialog_history_sample(rounds: int) -> tuple[int, str]:
    if rounds <= 0:
        return 0, ""
    user_goal = "Open the app and complete the workflow described in the task.\n\nSteps: launch → navigate → perform action → verify result."
    tool_overlay = "Attempted overlay click (index-targeted). Do not assume success. Verify on the next screenshot using visible UI cues only."
    tool_verify = "Sidecar verify signal accepted: action_result=pass, repetition_count=0 — host will close newest verifying row as verified - pass"
    tool_board = '{"board_len":4,"method":"patch","ok":true,"patched":[{"id":"1","status":"done"}],"reflection_required":false}'
    parts = [user_goal]
    for _i in range(rounds):
        parts.extend([tool_overlay, "", tool_verify, tool_board])
    text = "\n".join(parts)
    return count_tokens(text), text


def build_dynamic_scenarios(ref: str, static_merged_tokens: int, native_tools_tokens: int) -> dict:
    img_each, img_w, img_h = qwen3_image_tokens(IMAGE_W, IMAGE_H)
    clock = "Local wall-clock at capture: 2026-06-05 12:00:00 +08:00\n\n"
    anchor = build_pointer_anchor_block()

    scenarios = []
    for label, history_rows, images, dialog_rounds in [
        ("first_turn_minimal", 0, 2, 0),
        ("mid_task_typical", 5, 3, 3),
        ("heavy_history", 10, 3, 6),
    ]:
        has_before = images >= 3
        text = clock + build_cur_screen_preamble_primary(has_before)
        hist_block = build_tier_history_block(history_rows)
        if hist_block:
            text += "\n\n" + hist_block
        text += "\n\n" + anchor
        text_tokens = count_tokens(text)
        label_tokens = image_slot_label_tokens(images, has_before)
        image_tokens = img_each * images
        dialog_tokens, _ = build_dialog_history_sample(dialog_rounds)

        round_content = static_merged_tokens + text_tokens + label_tokens + image_tokens
        api_estimate = round_content + native_tools_tokens + dialog_tokens

        scenarios.append({
            "id": label,
            "cur_screen_text_tokens": text_tokens,
            "image_slot_label_tokens": label_tokens,
            "image_count": images,
            "image_tokens_each": img_each,
            "image_tokens_total": image_tokens,
            "dialog_history_tokens": dialog_tokens,
            "round_content_tokens": round_content,
            "api_prompt_estimate_tokens": api_estimate,
        })

    return {
        "image_assumption": f"{IMAGE_W}x{IMAGE_H} monitor JPEG full capture",
        "image_resized_for_formula": f"{img_w}x{img_h}",
        "image_tokens_each": img_each,
        "scenarios": scenarios,
    }


def count_native_tools(ref: str, tool_specs: list[tuple[str, str, str | None]]) -> dict:
    """Build and count OpenAI tools[] JSON for a given tool spec list.

    Each spec: (tool_name, doc_source_path, schema_yaml_path | None)
    - When schema_yaml_path is given → schema loaded from that .yaml keyed by tool name.
    - When schema_yaml_path is None → schema parsed from doc's YAML frontmatter.
    """
    # Count peers per doc source
    doc_peers: dict[str, int] = {}
    for _name, doc_path, _schema_path in tool_specs:
        doc_peers[doc_path] = doc_peers.get(doc_path, 0) + 1

    import yaml

    tools = []
    for name, doc_path, schema_path in tool_specs:
        doc = git_show(ref, doc_path)
        peers = doc_peers.get(doc_path, 0)

        # Resolve schema
        if schema_path:
            # Use standalone .schema.yaml
            schemas = load_schema_yaml(ref, schema_path)
            schema_dict = dict(schemas)
            schema = schema_dict.get(name)
            if schema is None or not isinstance(schema, dict):
                # Fallback: generic object
                schema = {"type": "object", "additionalProperties": True}
        else:
            # Parse YAML frontmatter from doc markdown
            s = doc.lstrip("\ufeff")
            if s.startswith("---\n"):
                rest = s[4:]
                end = rest.find("\n---\n")
                if end > 0:
                    front = rest[:end]
                    parsed = yaml.safe_load(front)
                    if isinstance(parsed, dict) and isinstance(parsed.get("schema"), dict):
                        schema = parsed["schema"]
                    else:
                        schema = {"type": "object", "additionalProperties": True}
                else:
                    schema = {"type": "object", "additionalProperties": True}
            else:
                schema = {"type": "object", "additionalProperties": True}

        desc = openai_description(name, doc, peers)
        tools.append({
            "type": "function",
            "function": {"name": name, "description": desc, "parameters": schema},
        })

    tools.sort(key=lambda t: t["function"]["name"])

    compact = json.dumps(tools, ensure_ascii=False, separators=(",", ":"))
    per_tool = [
        {
            "name": t["function"]["name"],
            "tokens": count_tokens(json.dumps(t, ensure_ascii=False, separators=(",", ":"))),
        }
        for t in tools
    ]
    return {
        "tool_count": len(tools),
        "json_chars": len(compact),
        "json_tokens": count_tokens(compact),
        "note": "Sent in API tools[] on every round; omitted from llm_prompts dump.",
        "per_tool": per_tool,
    }


# ── Computer-specific: build_native_openai_tools (kept for backward compat) ─


def build_native_openai_tools(ref: str) -> list[dict]:
    family_docs: dict[str, list[str]] = {}
    for _family, md_path, schema_file in NATIVE_TOOL_FAMILIES:
        if schema_file:
            if schema_file == "task_board.schema.yaml":
                schema_path = "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"
            else:
                schema_path = f"crates/pointer-core/src/agents/computer/tools/prompts/{schema_file}"
            for tool_name, _ in load_schema_yaml(ref, schema_path):
                family_docs.setdefault(md_path, []).append(tool_name)
        else:
            name = Path(md_path).stem
            if "action_verify" in md_path:
                name = "action_verify"
            family_docs.setdefault(md_path, []).append(name)

    tools: list[dict] = []
    for _family, md_path, schema_file in NATIVE_TOOL_FAMILIES:
        doc = git_show(ref, md_path)
        peers = len(family_docs.get(md_path, []))
        if schema_file:
            if schema_file == "task_board.schema.yaml":
                schema_path = "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"
            else:
                schema_path = f"crates/pointer-core/src/agents/computer/tools/prompts/{schema_file}"
            for name, schema in load_schema_yaml(ref, schema_path):
                tools.append({
                    "type": "function",
                    "function": {"name": name, "description": openai_description(name, doc, peers), "parameters": schema},
                })
        else:
            schema = schema_from_md_frontmatter(doc)
            if not isinstance(schema, dict):
                raise RuntimeError(f"Missing schema frontmatter in {md_path}")
            name = {"action_verify": "action_verify", "hotkey": "hotkey", "wait": "wait"}.get(Path(md_path).stem, Path(md_path).stem)
            tools.append({
                "type": "function",
                "function": {"name": name, "description": openai_description(name, doc, peers), "parameters": schema},
            })

    tools.sort(key=lambda t: t["function"]["name"])
    return tools


def count_native_tools_old(ref: str) -> dict:
    tools = build_native_openai_tools(ref)
    compact = json.dumps(tools, ensure_ascii=False, separators=(",", ":"))
    per_tool = [
        {
            "name": t["function"]["name"],
            "tokens": count_tokens(json.dumps(t, ensure_ascii=False, separators=(",", ":"))),
        }
        for t in tools
    ]
    return {
        "tool_count": len(tools),
        "json_chars": len(compact),
        "json_tokens": count_tokens(compact),
        "note": "Sent in API tools[] on every round; omitted from llm_prompts dump.",
        "per_tool": per_tool,
    }


# ═══════════════════════════════════════════════════════════════════════════════
# Coder / Explore dynamic scenario builders
# ═══════════════════════════════════════════════════════════════════════════════


def build_coder_dynamic_scenarios(static_coder_tokens: int) -> dict:
    env_stub = "x" * 530
    scenarios = []
    for label, task_board_tokens in [
        ("first_turn", 0),
        ("mid_task", 600),
        ("with_history", 1500),
    ]:
        text = env_stub
        dialog_tokens, _ = build_dialog_history_sample(0)
        total_text_tokens = count_tokens(text) + task_board_tokens
        round_content = static_coder_tokens + total_text_tokens
        scenarios.append({
            "id": label,
            "task_board_tokens": task_board_tokens,
            "env_text_tokens": count_tokens(env_stub),
            "dialog_history_tokens": dialog_tokens,
            "round_content_tokens": round_content,
        })
    return {"scenarios": scenarios}


def build_explore_dynamic_scenarios(static_explore_tokens: int) -> dict:
    env_stub = "x" * 530
    scenarios = []
    for label, task_board_tokens in [
        ("first_turn", 0),
        ("typical", 300),
    ]:
        text = env_stub
        total_text_tokens = count_tokens(text) + task_board_tokens
        round_content = static_explore_tokens + total_text_tokens
        scenarios.append({
            "id": label,
            "task_board_tokens": task_board_tokens,
            "env_text_tokens": count_tokens(env_stub),
            "round_content_tokens": round_content,
        })
    return {"scenarios": scenarios}


# ═══════════════════════════════════════════════════════════════════════════════
# Metrics collection
# ═══════════════════════════════════════════════════════════════════════════════


def collect_metrics(ref: str) -> dict:
    static_primary = build_static_primary(ref)
    native = count_native_tools_old(ref)
    coder_native = count_native_tools(ref, CODER_NATIVE_TOOLS)
    explore_native = count_native_tools(ref, EXPLORE_NATIVE_TOOLS)

    tools_per_file = [
        {"file": Path(p).name, "path": p, "tokens": count_tokens(git_show(ref, p))}
        for p in TOOL_FILES
    ]

    coder_merged, coder_sections = build_coder_system_prompt(ref)
    explore_merged, explore_sections = build_explore_system_prompt(ref)

    return {
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "git": {
            "ref": ref,
            "commit": git_show(ref, "").split("\n")[0] if False else subprocess.run(["git", "rev-parse", ref], cwd=REPO, capture_output=True, text=True).stdout.strip(),
            "short": subprocess.run(["git", "rev-parse", "--short", ref], cwd=REPO, capture_output=True, text=True).stdout.strip(),
            "date": subprocess.run(["git", "log", "-1", "--format=%ad", "--date=iso", ref], cwd=REPO, capture_output=True, text=True).stdout.strip(),
            "subject": subprocess.run(["git", "log", "-1", "--format=%s", ref], cwd=REPO, capture_output=True, text=True).stdout.strip(),
        },
        "tokenizer": {
            "model": "cl100k_base (tiktoken)",
            "note": "OpenAI-compatible BPE; same family as Qwen3.5 tokenizer. ~10% off native Qwen tokenizer but pip-installable.",
        },
        "primary_computer": {
            "static_primary_cacheable": static_primary,
            "native_openai_tools": native,
            "tools_per_file": tools_per_file,
            "dynamic_primary_round": build_dynamic_scenarios(ref, static_primary["merged_tokens"], native["json_tokens"]),
        },
        "coder": {
            "static_chars": len(coder_merged),
            "static_tokens": count_tokens(coder_merged),
            "sections": coder_sections,
            "sliced_detail": coder_sliced_detail(ref),
            "composed_body_chars": len(coder_composed_body(ref)),
            "composed_body_tokens": count_tokens(coder_composed_body(ref)),
            "composed_body_slices": [
                {"label": label, "path": path, "chars": len(git_show(ref, path)),
                 "tokens": count_tokens(git_show(ref, path))}
                for label, path in CODER_SLICES
            ],
            "dynamic_scenarios": build_coder_dynamic_scenarios(count_tokens(coder_merged)),
            "native_openai_tools": coder_native,
        },
        "explore": {
            "static_chars": len(explore_merged),
            "static_tokens": count_tokens(explore_merged),
            "sections": explore_sections,
            "sliced_detail": explore_sliced_detail(ref),
            "composed_body_chars": len(explore_composed_body(ref)),
            "composed_body_tokens": count_tokens(explore_composed_body(ref)),
            "composed_body_slices": [
                {"label": label, "path": path, "chars": len(git_show(ref, path)),
                 "tokens": count_tokens(git_show(ref, path))}
                for label, path in EXPLORE_SLICES
            ],
            "dynamic_scenarios": build_explore_dynamic_scenarios(count_tokens(explore_merged)),
            "native_openai_tools": explore_native,
        },
    }


# ═══════════════════════════════════════════════════════════════════════════════
# CLI
# ═══════════════════════════════════════════════════════════════════════════════


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ref", nargs="?", default=DEFAULT_REF)
    parser.add_argument("--json", action="store_true", help="Print JSON metrics to stdout")
    parser.add_argument("--update-baseline", action="store_true", help="Write scripts/prompt-token-baseline.{json,md}")
    args = parser.parse_args()

    metrics = collect_metrics(args.ref)

    if args.update_baseline:
        json_path = REPO / "scripts" / "prompt-token-baseline.json"
        md_path = REPO / "scripts" / "prompt-token-baseline.md"
        json_path.write_text(json.dumps(metrics, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        md_path.write_text(_render_markdown(metrics), encoding="utf-8")
        print(f"Updated {json_path.relative_to(REPO)}")
        print(f"Updated {md_path.relative_to(REPO)}")

    if args.json:
        print(json.dumps(metrics, indent=2, ensure_ascii=False))
        return

    # Console output
    g = metrics["git"]
    print(f"{g['short']} {g['subject']}")
    tok = metrics["tokenizer"]
    print(f"Tokenizer: {tok['model']}")
    print()

    # Computer
    pc = metrics["primary_computer"]
    static_t = pc["static_primary_cacheable"]["merged_tokens"]
    native_t = pc["native_openai_tools"]["json_tokens"]
    print(f"── Computer (Primary) ──")
    print(f"  Cacheable system:      {static_t:>6,} tok")
    print(f"  Native tools (API):     {native_t:>6,} tok")
    print(f"  Combined static:       {static_t + native_t:>6,} tok")
    for s in pc["static_primary_cacheable"]["sections"]:
        print(f"    {s['name']:<50} {s['tokens']:>6,} tok ({s['chars']:,} chars)")
    for s in pc["dynamic_primary_round"]["scenarios"]:
        print(f"  {s['id']:20} round_content {s['round_content_tokens']:>6,} · api_estimate {s['api_prompt_estimate_tokens']:>6,}")

    # Coder
    cdr = metrics["coder"]
    cdr_native_t = cdr["native_openai_tools"]["json_tokens"]
    print(f"\n── Coder (direct) ──")
    print(f"  Cacheable system: {cdr['static_tokens']:>6,} tok ({cdr['static_chars']:,} chars)")
    print(f"  Native tools (API):  {cdr_native_t:>6,} tok ({cdr['native_openai_tools']['tool_count']} tools)")
    print(f"  Combined static:    {cdr['static_tokens'] + cdr_native_t:>6,} tok")
    for s in cdr["sections"]:
        print(f"    {s['name']:<50} {s['tokens']:>6,} tok ({s['chars']:,} chars)")
    print(f"  composed_system_body: {cdr['composed_body_tokens']:>6,} tok ({cdr['composed_body_chars']:,} chars)")
    for d in cdr["composed_body_slices"]:
        print(f"    {d['label']:30} {d['tokens']:>5,} tok ({d['chars']:,} chars)")
    # Per-tool breakdown
    for pt in cdr["native_openai_tools"]["per_tool"]:
        print(f"    tool {pt['name']:30} {pt['tokens']:>5,} tok")
    combined_coder = cdr['static_tokens'] + cdr_native_t
    for s in cdr["dynamic_scenarios"]["scenarios"]:
        total = combined_coder + s['round_content_tokens'] - cdr['static_tokens']
        print(f"  {s['id']:20} round_content {total:>6,} tok (static+dynamic+native)")

    # Explore
    ex = metrics["explore"]
    ex_native_t = ex["native_openai_tools"]["json_tokens"]
    print(f"\n── Explore (sub-agent) ──")
    print(f"  Cacheable system: {ex['static_tokens']:>6,} tok ({ex['static_chars']:,} chars)")
    print(f"  Native tools (API): {ex_native_t:>6,} tok ({ex['native_openai_tools']['tool_count']} tools)")
    print(f"  Combined static:   {ex['static_tokens'] + ex_native_t:>6,} tok")
    for s in ex["sections"]:
        print(f"    {s['name']:<50} {s['tokens']:>6,} tok ({s['chars']:,} chars)")
    print(f"  composed_system_body: {ex['composed_body_tokens']:>6,} tok ({ex['composed_body_chars']:,} chars)")
    for d in ex["composed_body_slices"]:
        print(f"    {d['label']:30} {d['tokens']:>5,} tok ({d['chars']:,} chars)")
    for pt in ex["native_openai_tools"]["per_tool"]:
        print(f"    tool {pt['name']:30} {pt['tokens']:>5,} tok")
    combined_explore = ex['static_tokens'] + ex_native_t
    for s in ex["dynamic_scenarios"]["scenarios"]:
        total = combined_explore + s['round_content_tokens'] - ex['static_tokens']
        print(f"  {s['id']:20} round_content {total:>6,} tok (static+dynamic+native)")


def _render_markdown(metrics: dict) -> str:
    """Minimal baseline markdown (for --update-baseline)."""
    lines = ["# Prompt token baseline", "", f"Generated: {metrics['generated_at']}", ""]
    g = metrics["git"]
    lines.append(f"Commit: `{g['short']}` — {g['subject']}")
    lines.append("")
    lines.append(f"Tokenizer: {metrics['tokenizer']['model']}")
    lines.append("")
    for agent_key, label in [("primary_computer", "Computer Primary"), ("coder", "Coder"), ("explore", "Explore")]:
        a = metrics[agent_key]
        lines.append(f"## {label}")
        lines.append(f"- Cacheable system: **{a['static_tokens']:,} tok**")
        lines.append("")
    return "\n".join(lines)


if __name__ == "__main__":
    main()

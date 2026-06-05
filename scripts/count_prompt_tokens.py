#!/usr/bin/env python3
"""Count Pointer prompt tokens with the Qwen3.5 tokenizer (real, not heuristic).

Static slices are read from a git ref (default HEAD). Dynamic image tokens use the
official Qwen3.x vision formula: (h_bar * w_bar) / (32 * 32) + 2.

The **API prompt estimate** includes native OpenAI `tools[]` JSON (sent on every
chat/completions call but omitted from local `llm_prompts` dump files).

Usage:
  python3 scripts/count_prompt_tokens.py              # print summary
  python3 scripts/count_prompt_tokens.py --json       # machine-readable metrics
  python3 scripts/count_prompt_tokens.py --update-baseline  # refresh baseline files

Requires:
  pip install transformers modelscope pyyaml
  modelscope snapshot_download Qwen/Qwen3.5-0.8B  # once, for tokenizer files
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
DEFAULT_TOKENIZER_REPO = "Qwen/Qwen3.5-0.8B"
DEFAULT_TOKENIZER_CACHE = (
    Path.home() / ".cache/modelscope/hub/models/Qwen/Qwen3___5-0___8B"
)
IMAGE_W, IMAGE_H = 1920, 1080

SLOT_BEFORE = "[Screen before action]"
SLOT_AFTER = "[Screen after action]"
SLOT_ANNOTATED = "[Screen annotated]"

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

# Flat tool families registered in agents/computer/tools/mod.rs (Computer agent allowlist).
NATIVE_TOOL_FAMILIES: list[tuple[str, str, str | None]] = [
    ("mouse", "crates/pointer-core/src/agents/computer/tools/prompts/mouse.md", "mouse.schema.yaml"),
    ("input", "crates/pointer-core/src/agents/computer/tools/prompts/input.md", "input.schema.yaml"),
    (
        "modified_click",
        "crates/pointer-core/src/agents/computer/tools/prompts/modified_click.md",
        "modified_click.schema.yaml",
    ),
    (
        "captcha_verify",
        "crates/pointer-core/src/agents/computer/tools/prompts/captcha_verify.md",
        "captcha_verify.schema.yaml",
    ),
    ("clipboard", "crates/pointer-core/src/agents/computer/tools/prompts/clipboard.md", "clipboard.schema.yaml"),
    ("hotkey", "crates/pointer-core/src/agents/computer/tools/prompts/hotkey.md", None),
    ("wait", "crates/pointer-core/src/agents/computer/tools/prompts/wait.md", None),
    ("action_verify", "crates/pointer-core/src/agents/computer/tools/prompts/action_verify.md", None),
    ("task_board", "crates/pointer-core/src/task_board/prompts/task_board.md", "task_board.schema.yaml"),
]

SCHEMA_DIR = "crates/pointer-core/src/agents/computer/tools/prompts"
TASK_BOARD_SCHEMA = "crates/pointer-core/src/task_board/prompts/task_board.schema.yaml"


def git(*args: str) -> str:
    r = subprocess.run(["git", *args], cwd=REPO, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout.strip()


def load_tokenizer(local_path: Path):
    try:
        from transformers import AutoTokenizer
    except ImportError as e:
        raise SystemExit(
            "transformers is required: pip install transformers modelscope pyyaml"
        ) from e

    if not local_path.exists():
        raise SystemExit(
            f"Tokenizer not found at {local_path}.\n"
            f"Run once:\n"
            f"  python3 -c \"from modelscope import snapshot_download; "
            f"snapshot_download('{DEFAULT_TOKENIZER_REPO}', "
            f"allow_file_pattern=['tokenizer.json','tokenizer_config.json','vocab*','merges.txt'])\""
        )

    return AutoTokenizer.from_pretrained(
        str(local_path), trust_remote_code=True, local_files_only=True
    )


def count_tokens(tokenizer, text: str) -> int:
    if not text:
        return 0
    return len(tokenizer.encode(text, add_special_tokens=False))


def qwen3_image_tokens(width: int, height: int) -> tuple[int, int, int]:
    h_bar = round(height / 32) * 32
    w_bar = round(width / 32) * 32
    tokens = (h_bar * w_bar) // (32 * 32) + 2
    return tokens, w_bar, h_bar


def schema_from_md_frontmatter(md: str):
    try:
        import yaml
    except ImportError as e:
        raise SystemExit("pyyaml is required: pip install pyyaml") from e

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
    try:
        import yaml
    except ImportError as e:
        raise SystemExit("pyyaml is required: pip install pyyaml") from e

    text = git_show(ref, path)
    if not text:
        return []
    data = yaml.safe_load(text)
    if isinstance(data, dict):
        return [(str(k), v) for k, v in data.items()]
    return []


def openai_description(name: str, doc: str, peers_sharing_doc_source: int) -> str:
    """Match tools/mod.rs openai_description_for_entry."""
    if peers_sharing_doc_source > 1:
        return f"{name}: parameters in schema; full usage in system Tools appendix."
    t = doc.strip()
    if not t:
        return f"{name}: parameters in schema; full usage in system Tools appendix."
    if len(t) <= 240:
        return t
    return f"{name}: parameters in schema; full usage in system Tools appendix."


def build_native_openai_tools(ref: str) -> list[dict]:
    """Mirror ToolRegistry::openai_tools for the Computer agent allowlist."""
    # Count peers per doc source (md path).
    family_docs: dict[str, list[str]] = {}
    for _family, md_path, schema_file in NATIVE_TOOL_FAMILIES:
        if schema_file:
            if schema_file == "task_board.schema.yaml":
                schema_path = TASK_BOARD_SCHEMA
            else:
                schema_path = f"{SCHEMA_DIR}/{schema_file}"
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
                schema_path = TASK_BOARD_SCHEMA
            else:
                schema_path = f"{SCHEMA_DIR}/{schema_file}"
            for name, schema in load_schema_yaml(ref, schema_path):
                tools.append(
                    {
                        "type": "function",
                        "function": {
                            "name": name,
                            "description": openai_description(name, doc, peers),
                            "parameters": schema,
                        },
                    }
                )
        else:
            schema = schema_from_md_frontmatter(doc)
            if not isinstance(schema, dict):
                raise RuntimeError(f"Missing schema frontmatter in {md_path}")
            if "action_verify" in md_path:
                name = "action_verify"
            elif "hotkey" in md_path:
                name = "hotkey"
            elif "wait" in md_path:
                name = "wait"
            else:
                name = Path(md_path).stem
            tools.append(
                {
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": openai_description(name, doc, peers),
                        "parameters": schema,
                    },
                }
            )

    tools.sort(key=lambda t: t["function"]["name"])
    return tools


def tools_appendix_text(ref: str) -> str:
    body = "\n\n".join(git_show(ref, p) for p in TOOL_FILES)
    return f"## Tools\n\n{body}"


def build_runtime_tier_slice(ref: str) -> str:
    """Match computer_communication_for_tier(Primary) + body in single_agent_prompt."""
    comm = git_show(ref, "crates/pointer-core/src/agents/computer/prompts/tiers/primary/communication.md")
    os_md = git_show(ref, "crates/pointer-core/src/agents/computer/prompts/os/macos.md")
    loop_md = git_show(ref, "crates/pointer-core/src/agents/computer/prompts/tiers/primary/loop.md")
    comm_block = f"{comm}\n\n---\n\n{os_md}"
    return f"{comm_block}\n\n---\n\n{loop_md}"


def build_static_sections(ref: str, tokenizer) -> dict:
    env_stub = "x" * 530
    comm_public = git_show(ref, "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md")
    tier_slice = build_runtime_tier_slice(ref)
    tools_text = tools_appendix_text(ref)

    sections = [
        {
            "name": "COMMUNICATION_PUBLIC",
            "path": "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md",
            "chars": len(comm_public),
            "tokens": count_tokens(tokenizer, comm_public),
        },
        {
            "name": "tier slice (communication + os + loop)",
            "path": "runtime merge (see single_agent_prompt.rs)",
            "chars": len(tier_slice),
            "tokens": count_tokens(tokenizer, tier_slice),
        },
        {
            "name": "Tools appendix (deduped)",
            "path": "(see TOOL_FILES in script)",
            "chars": len(tools_text),
            "tokens": count_tokens(tokenizer, tools_text),
        },
        {
            "name": "[Environment] stub (~530 chars)",
            "path": "runtime-built (see env_prompt.rs)",
            "chars": len(env_stub),
            "tokens": count_tokens(tokenizer, env_stub),
        },
    ]
    merged = "\n\n".join([comm_public, tier_slice, tools_text, env_stub])
    nodup, ded = _appendix_sizes(ref)
    return {
        "sections": sections,
        "merged_chars": len(merged),
        "merged_tokens": count_tokens(tokenizer, merged),
        "appendix_chars_nodup": nodup,
        "appendix_chars_dedup": ded,
    }


def build_cur_screen_preamble_primary(has_before: bool) -> str:
    """Match screen_inject.rs build_cur_screen_preamble(Primary)."""
    cite = (
        "Each screenshot below is preceded by its slot label on its own line. "
        "Treat only what you see in that labeled image as ground truth — when reasoning internally, "
        "cite **On [slot name]:**; do not invent UI from task text or prior turns. "
        "Do not write internal checklists in assistant message text. "
        "When the user must see a reply (question, blockage, completion), write plain text in **content** "
        "in the same turn — reasoning alone is invisible to the user."
    )
    before_note = "optional " + SLOT_BEFORE + ", " if has_before else ""
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
    """Match tier::format_tier_history_block (no give-up reference)."""
    if history_rows <= 0:
        return ""
    lines = [
        "[Recent desktop tool calls — ordered oldest to newest; repetition uses goal; "
        "coordinates are session 0-1000; overlay indices are not comparable across turns.]",
        "Verify suffix: only the newest row may show verify: verifying; verify: skipped = never verified; "
        "verify: verified - * = closed — do not re-verify or re-report.",
    ]
    row = (
        '  {i}: mouse_click_index goal="Save document" action="click Save" '
        'at (512, 384) | verify: verified - pass'
    )
    for i in range(1, history_rows + 1):
        suffix = " | verify: verifying" if i == history_rows else " | verify: verified - pass"
        lines.append(row.format(i=i).rsplit(" | ", 1)[0] + suffix)
    return "\n".join(lines)


def build_pointer_anchor_block() -> str:
    """Typical Primary-tier pointer + nearby bbox inject (reference_anchors.rs shape)."""
    pointer = (
        "**Pointer position** (capture pixels): (960, 540); normalized (500, 500) on 0–1000.\n\n"
        "**Nearby overlay reference bboxes** (10 indices nearest the **pointer** on this capture — "
        "session 0-1000; each row **R: (left, top, right, bottom)**; use **`index`** for index tools "
        "(bbox center); sorted nearest-first; digits are **labels only**, not click targets):"
    )
    rows = "\n".join(
        f"- {i}: ({100 + i * 20}, {200 + i * 5}, {180 + i * 20}, {280 + i * 5})"
        for i in range(1, 11)
    )
    tail = (
        "Image-grounded analysis: cite **On [slot name]:** internally. **Nearby** rows must copy a bullet "
        "below character-for-character — if **`- R:`** is missing, **Inject match: NOT FOUND** and "
        "**hover_index** only; **forbidden** inventing **(left, top, right, bottom)**. **Verify:** judge "
        "**Expected vs Actual UI change** — pointer on target is **not** pass for click/copy goals. "
        "Overlay digits label bboxes only — **forbidden** treating digit position as the click point. "
        "Do not write reasoning in assistant message text. Follow **communication** rules."
    )
    return f"{pointer}\n{rows}\n\n{tail}"


def image_slot_label_tokens(tokenizer, image_count: int, has_before: bool) -> int:
    labels: list[str] = []
    if has_before:
        labels.append(SLOT_BEFORE)
    labels.append(SLOT_AFTER)
    labels.append(SLOT_ANNOTATED)
    labels = labels[:image_count]
    return sum(count_tokens(tokenizer, f"{lab}\n") for lab in labels)


def build_dialog_history_sample(tokenizer, rounds: int) -> tuple[int, str]:
    """Synthetic mid-task dialog (user goal + tool results between rounds)."""
    if rounds <= 0:
        return 0, ""
    user_goal = (
        "Open the app and complete the workflow described in the task.\n\n"
        "Steps: launch → navigate → perform action → verify result."
    )
    tool_overlay = (
        "Attempted overlay click (index-targeted). Do not assume success. "
        "Verify on the next screenshot using visible UI cues only."
    )
    tool_verify = (
        "Sidecar verify signal accepted: action_result=pass, repetition_count=0 — "
        "host will close newest verifying row as verified - pass"
    )
    tool_board = (
        '{"board_len":4,"method":"patch","ok":true,'
        '"patched":[{"id":"1","status":"done"}],"reflection_required":false}'
    )
    parts = [user_goal]
    for i in range(rounds):
        parts.extend([tool_overlay, "", tool_verify, tool_board])
    text = "\n".join(parts)
    return count_tokens(tokenizer, text), text


def build_dynamic_scenarios(
    ref: str, tokenizer, static_merged_tokens: int, native_tools_tokens: int
) -> dict:
    wire = git_show(ref, "crates/pointer-core/src/agents/_shared/JSON_WIRE_TAIL.md").strip()
    wire_tokens = count_tokens(tokenizer, wire)
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
        text_tokens = count_tokens(tokenizer, text)
        label_tokens = image_slot_label_tokens(tokenizer, images, has_before)
        image_tokens = img_each * images
        dialog_tokens, _ = build_dialog_history_sample(tokenizer, dialog_rounds)

        round_content = static_merged_tokens + text_tokens + label_tokens + wire_tokens + image_tokens
        api_estimate = round_content + native_tools_tokens + dialog_tokens

        scenarios.append(
            {
                "id": label,
                "description": {
                    "first_turn_minimal": "No tool history, 2 images (after + annotated), no prior dialog",
                    "mid_task_typical": "5 history rows, 3 images (before + after + annotated), ~3 tool rounds dialog",
                    "heavy_history": "10 history rows (cap), 3 images, ~6 tool rounds dialog",
                }[label],
                "cur_screen_text_chars": len(text),
                "cur_screen_text_tokens": text_tokens,
                "image_slot_label_tokens": label_tokens,
                "image_count": images,
                "image_tokens_each": img_each,
                "image_tokens_total": image_tokens,
                "json_wire_tokens": wire_tokens,
                "dialog_history_tokens": dialog_tokens,
                "round_content_tokens": round_content,
                "api_prompt_estimate_tokens": api_estimate,
            }
        )

    return {
        "image_assumption": f"{IMAGE_W}x{IMAGE_H} monitor JPEG full capture",
        "image_resized_for_formula": f"{img_w}x{img_h}",
        "image_tokens_each": img_each,
        "json_wire_tail_chars": len(wire),
        "json_wire_tail_tokens": wire_tokens,
        "scenarios": scenarios,
    }


def count_native_tools(ref: str, tokenizer) -> dict:
    tools = build_native_openai_tools(ref)
    compact = json.dumps(tools, ensure_ascii=False, separators=(",", ":"))
    per_tool = [
        {
            "name": t["function"]["name"],
            "tokens": count_tokens(tokenizer, json.dumps(t, ensure_ascii=False, separators=(",", ":"))),
            "description_tokens": count_tokens(tokenizer, t["function"]["description"]),
        }
        for t in tools
    ]
    return {
        "tool_count": len(tools),
        "json_chars": len(compact),
        "json_tokens": count_tokens(tokenizer, compact),
        "note": (
            "Sent in API tools[] on every round; omitted from local llm_prompts dump JSON. "
            "Descriptions are compact when tools share a doc_source; full prose is in system Tools appendix."
        ),
        "per_tool": per_tool,
    }


def reconcile_with_dump(tokenizer, metrics: dict) -> dict | None:
    dump_dir = Path.home() / "Library/Application Support/PointerApp/logs/llm_prompts"
    if not dump_dir.exists():
        return None
    dumps = sorted(dump_dir.glob("*.json"), key=lambda p: p.stat().st_mtime, reverse=True)
    if not dumps:
        return None

    path = dumps[0]
    data = json.loads(path.read_text(encoding="utf-8"))
    img_each, _, _ = qwen3_image_tokens(IMAGE_W, IMAGE_H)

    def msg_text_tokens(m: dict) -> int:
        c = m.get("content")
        if isinstance(c, str):
            return count_tokens(tokenizer, c)
        if isinstance(c, list):
            return sum(
                count_tokens(tokenizer, p.get("text", ""))
                for p in c
                if p.get("type") == "text"
            )
        return 0

    def msg_image_count(m: dict) -> int:
        c = m.get("content")
        if not isinstance(c, list):
            return 0
        return sum(1 for p in c if p.get("type") == "image_url")

    per_msg = []
    text_total = 0
    image_total = 0
    for i, m in enumerate(data["messages"]):
        tt = msg_text_tokens(m)
        ic = msg_image_count(m)
        it = ic * img_each
        text_total += tt
        image_total += it
        per_msg.append({"index": i, "role": m["role"], "text_tokens": tt, "image_count": ic})

    native_t = metrics["native_openai_tools"]["json_tokens"]
    return {
        "file": str(path),
        "mtime_iso": datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc).isoformat(),
        "model": data.get("model"),
        "phase": data.get("phase"),
        "dump_text_tokens": text_total,
        "dump_image_tokens": image_total,
        "dump_text_plus_images": text_total + image_total,
        "native_tools_tokens_estimate": native_t,
        "full_api_estimate_from_dump": text_total + image_total + native_t,
        "per_message": per_msg,
        "note": (
            "Dump excludes tools[]; add native_tools_tokens_estimate for API prompt_tokens parity. "
            "API may differ slightly due to chat-template special tokens and provider-side counting."
        ),
    }


def latest_llm_dump_metrics(tokenizer) -> dict | None:
    dump_dir = Path.home() / "Library/Application Support/PointerApp/logs/llm_prompts"
    if not dump_dir.exists():
        return None
    dumps = sorted(dump_dir.glob("*.json"), key=lambda p: p.stat().st_mtime, reverse=True)
    if not dumps:
        return None
    path = dumps[0]
    data = json.loads(path.read_text(encoding="utf-8"))
    sys_text = ""
    for m in data["messages"]:
        if m["role"] != "system":
            continue
        c = m["content"]
        if isinstance(c, list):
            for part in c:
                if part.get("type") == "text":
                    sys_text += part.get("text", "")
        else:
            sys_text = c

    cur_screen_text = ""
    image_slots = 0
    for m in data["messages"]:
        if m["role"] != "user" or not isinstance(m.get("content"), list):
            continue
        for part in m["content"]:
            if part.get("type") == "image_url":
                image_slots += 1
            if part.get("type") == "text" and "[CUR_SCREEN]" in part.get("text", ""):
                cur_screen_text = part["text"]

    wire = next(
        (
            m["content"]
            for m in data["messages"]
            if m["role"] == "user"
            and isinstance(m.get("content"), str)
            and "Native tool-calling" in m.get("content", "")
        ),
        "",
    )
    task_board = next(
        (
            m["content"]
            for m in reversed(data["messages"])
            if m["role"] == "user"
            and isinstance(m.get("content"), str)
            and "[TASK_BOARD]" in m.get("content", "")
        ),
        "",
    )
    return {
        "file": str(path),
        "mtime_iso": datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc).isoformat(),
        "model": data.get("model"),
        "phase": data.get("phase"),
        "system_tokens": count_tokens(tokenizer, sys_text),
        "cur_screen_text_tokens": count_tokens(tokenizer, cur_screen_text) if cur_screen_text else None,
        "cur_screen_text_chars": len(cur_screen_text) if cur_screen_text else None,
        "json_wire_tokens": count_tokens(tokenizer, wire) if wire else None,
        "task_board_tokens": count_tokens(tokenizer, task_board) if task_board else None,
        "image_slots": image_slots,
        "note": "Historical dump; may predate current system size or omit tools[] from file.",
    }


def collect_metrics(ref: str, tokenizer_path: Path) -> dict:
    tokenizer = load_tokenizer(tokenizer_path)
    static = build_static_sections(ref, tokenizer)
    native = count_native_tools(ref, tokenizer)
    tools_per_file = [
        {"file": Path(p).name, "path": p, "tokens": count_tokens(tokenizer, git_show(ref, p))}
        for p in TOOL_FILES
    ]
    return {
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "git": {
            "ref": ref,
            "commit": git("rev-parse", ref),
            "short": git("rev-parse", "--short", ref),
            "date": git("log", "-1", "--format=%ad", "--date=iso", ref),
            "subject": git("log", "-1", "--format=%s", ref),
        },
        "tokenizer": {
            "model_id": DEFAULT_TOKENIZER_REPO,
            "local_path": str(tokenizer_path),
            "method": "len(tokenizer.encode(text, add_special_tokens=False))",
            "image_method": "Qwen3.x formula: (h_bar*w_bar)/(32*32)+2; not text tokenizer",
        },
        "static_primary_cacheable": static,
        "native_openai_tools": native,
        "static_other": {
            "tools_per_file": tools_per_file,
        },
        "dynamic_primary_round": build_dynamic_scenarios(
            ref, tokenizer, static["merged_tokens"], native["json_tokens"]
        ),
        "runtime_dump_reference": latest_llm_dump_metrics(tokenizer),
        "dump_reconciliation": reconcile_with_dump(tokenizer, {"native_openai_tools": native}),
    }


def render_markdown(metrics: dict) -> str:
    g = metrics["git"]
    tok = metrics["tokenizer"]
    static = metrics["static_primary_cacheable"]
    dynamic = metrics["dynamic_primary_round"]
    native = metrics["native_openai_tools"]
    lines = [
        "# Prompt token baseline",
        "",
        "Baseline for comparing prompt size across git iterations.",
        "Regenerate with:",
        "",
        "```bash",
        "pip install transformers modelscope pyyaml  # once",
        "python3 -c \"from modelscope import snapshot_download; "
        "snapshot_download('Qwen/Qwen3.5-0.8B', "
        "allow_file_pattern=['tokenizer.json','tokenizer_config.json','vocab*','merges.txt'])\"",
        "python3 scripts/count_prompt_tokens.py --update-baseline",
        "```",
        "",
        "## Git snapshot",
        "",
        f"| Field | Value |",
        f"|-------|-------|",
        f"| Commit | `{g['commit']}` |",
        f"| Short | `{g['short']}` |",
        f"| Date | {g['date']} |",
        f"| Subject | {g['subject']} |",
        f"| Baseline generated (UTC) | {metrics['generated_at']} |",
        "",
        "## Tokenizer",
        "",
        f"- Model: `{tok['model_id']}` (same vocab family as hosted **qwen3.5-plus**)",
        f"- Local cache: `{Path(tok['local_path']).name}` under ModelScope hub "
        f"(override with `--tokenizer-path`; default see script)",
        f"- Text: `{tok['method']}`",
        f"- Images: `{tok['image_method']}`",
        "",
        "## Static — Primary cacheable system prompt",
        "",
        "Runtime merge: `COMMUNICATION_PUBLIC` + tier slice (`communication` + `---` + `os` + `---` + `loop`) "
        + "`## Tools` appendix + `[Environment]`. Matches `single_agent_prompt.rs`.",
        "",
        f"**Total: {static['merged_tokens']:,} tokens** ({static['merged_chars']:,} chars)",
        "",
        "| Section | Chars | Tokens | Share |",
        "|---------|------:|-------:|------:|",
    ]
    total = static["merged_tokens"]
    for s in static["sections"]:
        share = 0 if total == 0 else s["tokens"] / total * 100
        lines.append(f"| {s['name']} | {s['chars']:,} | {s['tokens']:,} | {share:.1f}% |")

    lines.extend(
        [
            "",
            f"Tools appendix dedup: {static['appendix_chars_dedup']:,} chars "
            f"(pre-dedup would be {static['appendix_chars_nodup']:,} chars).",
            "",
            "### Tools appendix per file (deduped, each once)",
            "",
            "| File | Tokens |",
            "|------|-------:|",
        ]
    )
    for row in metrics["static_other"]["tools_per_file"]:
        lines.append(f"| {row['file']} | {row['tokens']:,} |")

    lines.extend(
        [
            "",
            "## Native OpenAI tools (API only)",
            "",
            f"**{native['json_tokens']:,} tokens** ({native['json_chars']:,} chars JSON) · "
            f"{native['tool_count']} flat tools",
            "",
            "_Not in `llm_prompts` dump files; included in provider `usage.prompt_tokens`._",
            "",
            "Descriptions are **compact** when flat tools share a `doc_source` "
            "(full prose once in system Tools appendix).",
            "",
        ]
    )

    lines.extend(
        [
            "## Dynamic — Computer Primary per-round",
            "",
            f"Image assumption: {dynamic['image_assumption']} → "
            f"{dynamic['image_resized_for_formula']} → **{dynamic['image_tokens_each']:,} tokens/image**.",
            f" JSON wire tail: **{dynamic['json_wire_tail_tokens']:,} tokens**.",
            "",
            "**round_content** = cacheable system + `[CUR_SCREEN]` text + image slot labels + JSON wire + images.",
            "",
            "**api_prompt_estimate** = round_content + native tools JSON + dialog history (user goal + tool results).",
            "",
            "| Scenario | CUR_SCREEN | Labels | Images | Wire | Dialog | round_content | **api_estimate** |",
            "|----------|----------:|-------:|-------:|-----:|-------:|--------------:|-----------------:|",
        ]
    )
    for s in dynamic["scenarios"]:
        lines.append(
            f"| {s['id']} | {s['cur_screen_text_tokens']:,} | {s['image_slot_label_tokens']:,} | "
            f"{s['image_tokens_total']:,} ({s['image_count']}×{s['image_tokens_each']:,}) | "
            f"{s['json_wire_tokens']:,} | {s['dialog_history_tokens']:,} | "
            f"{s['round_content_tokens']:,} | **{s['api_prompt_estimate_tokens']:,}** |"
        )
    for s in dynamic["scenarios"]:
        lines.extend(["", f"### `{s['id']}`", "", s["description"], ""])

    dump = metrics.get("runtime_dump_reference")
    lines.extend(["", "## Runtime dump reference (optional)", ""])
    if dump:
        lines.extend(
            [
                f"Latest local dump: `{dump['file']}`",
                f"Model: `{dump.get('model')}` · mtime: {dump.get('mtime_iso')}",
                "",
                "| Field | Tokens |",
                "|-------|-------:|",
                f"| system (as dumped) | {dump.get('system_tokens', 'n/a')} |",
                f"| `[CUR_SCREEN]` text | {dump.get('cur_screen_text_tokens', 'n/a')} |",
                f"| JSON wire | {dump.get('json_wire_tokens', 'n/a')} |",
                f"| `[TASK_BOARD]` | {dump.get('task_board_tokens', 'n/a')} |",
                f"| image slots | {dump.get('image_slots', 'n/a')} (vision tokens not in dump) |",
                "",
                f"_{dump.get('note', '')}_",
            ]
        )
    else:
        lines.append("_No local LLM dump found._")

    recon = metrics.get("dump_reconciliation")
    if recon:
        lines.extend(
            [
                "",
                "## Dump reconciliation",
                "",
                f"File: `{recon['file']}`",
                "",
                "| Component | Tokens |",
                "|-----------|-------:|",
                f"| Dump messages (text) | {recon['dump_text_tokens']:,} |",
                f"| Dump images ({recon['per_message'][-2]['image_count']}× formula) | {recon['dump_image_tokens']:,} |",
                f"| Native tools (estimate) | {recon['native_tools_tokens_estimate']:,} |",
                f"| **Full API estimate** | **{recon['full_api_estimate_from_dump']:,}** |",
                "",
                f"_{recon.get('note', '')}_",
            ]
        )

    lines.extend(
        [
            "",
            "## Compare with a later ref",
            "",
            "```bash",
            "python3 scripts/count_prompt_tokens.py HEAD",
            "python3 scripts/count_prompt_tokens.py --json > /tmp/prompt-metrics.json",
            "git diff --no-index scripts/prompt-token-baseline.json /tmp/prompt-metrics.json",
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ref", nargs="?", default=DEFAULT_REF, help="Git ref (default HEAD)")
    parser.add_argument("--json", action="store_true", help="Print JSON metrics to stdout")
    parser.add_argument(
        "--update-baseline",
        action="store_true",
        help="Write scripts/prompt-token-baseline.{json,md}",
    )
    parser.add_argument(
        "--tokenizer-path",
        type=Path,
        default=DEFAULT_TOKENIZER_CACHE,
        help="Local Qwen3.5 tokenizer directory",
    )
    args = parser.parse_args()

    metrics = collect_metrics(args.ref, args.tokenizer_path)

    if args.update_baseline:
        json_path = REPO / "scripts" / "prompt-token-baseline.json"
        md_path = REPO / "scripts" / "prompt-token-baseline.md"
        json_path.write_text(json.dumps(metrics, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        md_path.write_text(render_markdown(metrics), encoding="utf-8")
        print(f"Updated {json_path.relative_to(REPO)}")
        print(f"Updated {md_path.relative_to(REPO)}")

    if args.json:
        print(json.dumps(metrics, indent=2, ensure_ascii=False))
        return

    if not args.update_baseline:
        g = metrics["git"]
        static_t = metrics["static_primary_cacheable"]["merged_tokens"]
        native_t = metrics["native_openai_tools"]["json_tokens"]
        print(f"{g['short']} {g['subject']} — cacheable system: {static_t:,} · native tools: {native_t:,}")
        for s in metrics["dynamic_primary_round"]["scenarios"]:
            print(
                f"  {s['id']}: round_content {s['round_content_tokens']:,} · "
                f"api_estimate {s['api_prompt_estimate_tokens']:,}"
            )


if __name__ == "__main__":
    main()

"""Compare prompt asset sizes between two git refs."""
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]

FILES = [
    "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md",
    "crates/pointer-core/src/agents/computer/prompts/tiers/primary/communication.md",
    "crates/pointer-core/src/agents/computer/prompts/tiers/primary/loop.md",
    "crates/pointer-core/src/agents/computer/AGENT.md",
    "crates/pointer-core/src/agents/computer/prompts/os/windows.md",
    "crates/pointer-core/src/agents/computer/prompts/os/macos.md",
    "crates/pointer-core/src/agents/computer/prompts/os/linux.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/action_verify.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/captcha_verify.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/clipboard.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/hotkey.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/input.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/modified_click.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/mouse.md",
    "crates/pointer-core/src/agents/computer/tools/prompts/wait.md",
    "crates/pointer-core/src/tools/prompts/task_board.md",
]


def est_tokens(s: str) -> int:
    if not s:
        return 0
    cjk = len(re.findall(r"[\u4e00-\u9fff]", s))
    other = len(s) - cjk
    return int(cjk / 1.5 + other / 4)


def git_show(ref: str, path: str) -> str:
    r = subprocess.run(
        ["git", "show", f"{ref}:{path}"],
        cwd=REPO,
        capture_output=True,
    )
    if r.returncode != 0:
        return ""
    return r.stdout.decode("utf-8", errors="replace")


def glob_at_ref(ref: str, pattern: str) -> list[str]:
    r = subprocess.run(
        ["git", "ls-tree", "-r", "--name-only", ref, pattern],
        cwd=REPO,
        capture_output=True,
        text=True,
    )
    return [ln.strip() for ln in r.stdout.splitlines() if ln.strip()]


def main() -> None:
    old_ref = sys.argv[1] if len(sys.argv) > 1 else "ef71c0d"
    new_ref = sys.argv[2] if len(sys.argv) > 2 else "HEAD"

    # discover computer tool prompt files at each ref
    old_tool_glob = glob_at_ref(old_ref, "crates/pointer-core/src/agents/computer/tools/prompts")
    new_tool_glob = glob_at_ref(new_ref, "crates/pointer-core/src/agents/computer/tools/prompts")
    all_paths = sorted(set(FILES + old_tool_glob + new_tool_glob))
    all_paths = [p for p in all_paths if p.endswith(".md")]

    rows = []
    for path in all_paths:
        old = git_show(old_ref, path)
        new = git_show(new_ref, path)
        dc = len(new) - len(old)
        dt = est_tokens(new) - est_tokens(old)
        if old or new:
            rows.append((path, len(old), len(new), dc, dt))

    rows.sort(key=lambda r: abs(r[4]), reverse=True)

    print(f"Compare {old_ref} -> {new_ref}")
    print(f"{'path':70} {'old':>8} {'new':>8} {'d_chars':>8} {'d_tok':>7}")
    print("-" * 110)
    total_old = total_new = 0
    for path, oc, nc, dc, dt in rows:
        if dc == 0 and not git_show(new_ref, path):
            continue
        print(f"{path:70} {oc:8,} {nc:8,} {dc:+8,} {dt:+7,}")
        total_old += oc
        total_new += nc

    print("-" * 110)
    print(
        f"{'TOTAL (listed .md assets)':70} {total_old:8,} {total_new:8,} {total_new-total_old:+8,} {est_tokens('x'*max(total_new-total_old,0)) if total_new>=total_old else est_tokens('x'*(total_old-total_new)):+7,}"
    )
    print(f"Est token delta (sum per-file): {sum(r[4] for r in rows):+,}")

    # commits with largest primary communication growth
    print("\n=== Commits touching primary/communication (last 5d) ===")
    r = subprocess.run(
        [
            "git",
            "log",
            "--since=2026-05-31",
            "--pretty=format:%h %ad %s",
            "--date=short",
            "--",
            "crates/pointer-core/src/agents/computer/prompts/tiers/primary/communication.md",
            "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md",
            "crates/pointer-core/src/agents/computer/tools/prompts",
            "crates/pointer-core/src/tools_system_appendix.rs",
            "crates/pointer-core/src/agents/computer/tools/mod.rs",
        ],
        cwd=REPO,
        capture_output=True,
        text=True,
    )
    print(r.stdout)


def timeline() -> None:
    refs = [
        "ef71c0d",
        "ee47838",
        "f7a8208",
        "ecd5e22",
        "29853c9",
        "724d1c3^",
        "724d1c3",
        "HEAD",
    ]
    base_tok = None
    print(f"{'ref':10} {'cacheable_tok':>12} {'delta':>8}  appendix(nodup/dedup)")
    for ref in refs:
        parts = [
            git_show(ref, "crates/pointer-core/src/agents/_shared/COMMUNICATION_PUBLIC.md"),
            git_show(ref, "crates/pointer-core/src/agents/computer/prompts/tiers/primary/communication.md"),
            git_show(ref, "crates/pointer-core/src/agents/computer/prompts/tiers/primary/loop.md"),
            git_show(ref, "crates/pointer-core/src/agents/computer/AGENT.md"),
            git_show(ref, "crates/pointer-core/src/agents/computer/prompts/os/windows.md"),
        ]
        nodup, ded = _appendix_sizes(ref)
        has_dedup = "seen_sources" in git_show(ref, "crates/pointer-core/src/tools_system_appendix.rs")
        appendix = ded if has_dedup else nodup
        total_chars = sum(len(p) for p in parts) + appendix + 530
        t = est_tokens("x" * total_chars)
        delta = 0 if base_tok is None else t - base_tok
        if base_tok is None:
            base_tok = t
        flag = "dedup" if has_dedup else "DUP"
        print(f"{ref:10} {t:12,} {delta:+8,}  {nodup:,}/{ded:,} ({flag})")


def _appendix_sizes(ref: str) -> tuple[int, int]:
    modrs = git_show(ref, "crates/pointer-core/src/agents/computer/tools/mod.rs")
    flat = "mouse_click_index" in modrs
    tb = git_show(ref, "crates/pointer-core/src/task_board/prompts/task_board.md")
    if not flat:
        paths = [
            "composite_action",
            "modified_click",
            "captcha_verify",
            "clipboard",
            "hotkey",
            "wait",
        ]
        total = sum(
            len(git_show(ref, f"crates/pointer-core/src/agents/computer/tools/prompts/{p}.md"))
            for p in paths
        )
        total += len(
            git_show(ref, "crates/pointer-core/src/agents/computer/tools/prompts/sidecar/tier_signal.md")
        )
        total += len(tb)
        return total, total

    families: list[tuple[int, str]] = [
        (12, "crates/pointer-core/src/agents/computer/tools/prompts/mouse.md"),
        (4, "crates/pointer-core/src/agents/computer/tools/prompts/modified_click.md"),
        (3, "crates/pointer-core/src/agents/computer/tools/prompts/captcha_verify.md"),
        (2, "crates/pointer-core/src/agents/computer/tools/prompts/clipboard.md"),
        (1, "crates/pointer-core/src/agents/computer/tools/prompts/hotkey.md"),
        (1, "crates/pointer-core/src/agents/computer/tools/prompts/wait.md"),
        (1, "crates/pointer-core/src/task_board/prompts/task_board.md"),
    ]
    if "input_index" in modrs:
        families.insert(
            1, (3, "crates/pointer-core/src/agents/computer/tools/prompts/input.md")
        )
    av = "crates/pointer-core/src/agents/computer/tools/prompts/action_verify.md"
    if not git_show(ref, av):
        av = "crates/pointer-core/src/agents/computer/tools/prompts/sidecar/tier_signal.md"
    families.append((1, av))

    nodup = sum(len(git_show(ref, path)) * cnt for cnt, path in families)
    seen: set[str] = set()
    ded = 0
    for _cnt, path in families:
        if path not in seen:
            ded += len(git_show(ref, path))
            seen.add(path)
    return nodup, ded


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--timeline":
        timeline()
    else:
        main()

import json
import re
import sys
from pathlib import Path


def est_tokens(s: str) -> int:
    if not s:
        return 0
    cjk = len(re.findall(r"[\u4e00-\u9fff]", s))
    other = len(s) - cjk
    return int(cjk / 1.5 + other / 4)


def chars(s: str) -> int:
    return len(s) if s else 0


def slice_between(text: str, start: str, end: str | None) -> str:
    i = text.find(start)
    if i < 0:
        return ""
    j = text.find(end, i + len(start)) if end else len(text)
    if j < 0:
        j = len(text)
    return text[i:j]


def main() -> None:
    path = Path(sys.argv[1])
    data = json.loads(path.read_text(encoding="utf-8"))

    sys_text = ""
    for m in data["messages"]:
        if m["role"] == "system":
            c = m["content"]
            if isinstance(c, list):
                for part in c:
                    if part.get("type") == "text":
                        sys_text += part.get("text", "")
            else:
                sys_text = c

    markers = [
        ("COMMUNICATION_PUBLIC", "# General rules", "## App data directory"),
        ("Skills / data dir", "## App data directory", "## Role"),
        ("Computer COMMUNICATION", "## Role", "## Windows platform guidance"),
        ("Windows OS prompt", "## Windows platform guidance", "# Computer Use Agent"),
        ("Computer AGENT.md (primary)", "# Computer Use Agent", "## Tools"),
        ("Tools appendix", "## Tools", "[Environment]"),
        ("Environment", "[Environment]", None),
    ]
    sections = {name: slice_between(sys_text, start, end) for name, start, end in markers}

    msgs = data["messages"]
    final_user_idx = None
    for i, m in enumerate(msgs):
        if m["role"] == "user" and isinstance(m.get("content"), list):
            text_parts = [p.get("text", "") for p in m["content"] if p.get("type") == "text"]
            if any("[CUR_SCREEN]" in t for t in text_parts):
                final_user_idx = i

    hist_text = ""
    for i, m in enumerate(msgs):
        if m["role"] == "system" or (final_user_idx is not None and i >= final_user_idx):
            continue
        c = m.get("content", "")
        if isinstance(c, str):
            hist_text += c
        if m.get("tool_calls"):
            hist_text += json.dumps(m["tool_calls"], ensure_ascii=False)

    cur_screen_text = 0
    cur_screen_images = 0
    image_base64_est = 0
    wire_tail = 0
    for i, m in enumerate(msgs):
        if i == final_user_idx:
            for p in m["content"]:
                if p.get("type") == "text":
                    cur_screen_text += chars(p.get("text", ""))
                elif p.get("type") == "image_url":
                    cur_screen_images += 1
                    url = p.get("image_url", {}).get("url", "")
                    m2 = re.search(r"(\d+) chars", url)
                    if m2:
                        image_base64_est += int(m2.group(1))
        if (
            m["role"] == "user"
            and isinstance(m.get("content"), str)
            and "Native tool-calling" in m.get("content", "")
        ):
            wire_tail = chars(m["content"])

    sys_total = chars(sys_text)
    sys_tokens = est_tokens(sys_text)
    hist_tokens = est_tokens(hist_text)
    cur_tokens = est_tokens("x" * cur_screen_text)
    wire_tokens = est_tokens("x" * wire_tail)
    image_tokens_est = image_base64_est // 4

    print("=== META ===")
    print(f"model: {data.get('model')}")
    print(f"phase: {data.get('phase')}")
    print(f"thinking_budget: {data.get('thinking_budget')}")
    print(f"messages count: {len(msgs)}")
    print(f"cache_control on system: yes (qwen ephemeral)")
    print()

    print("=== SYSTEM PROMPT SECTIONS (cacheable block) ===")
    for name, start, end in markers:
        s = sections[name]
        ch = chars(s)
        tk = est_tokens(s)
        print(f"{name:35} {ch:7,} chars  ~{tk:6,} tok  {ch / sys_total * 100:5.1f}%")
    print(f"{'SYSTEM TOTAL':35} {sys_total:7,} chars  ~{sys_tokens:6,} tok  100.0%")
    print()

    print("=== MESSAGES (non-system) ===")
    print(f"History                          {chars(hist_text):7,} chars  ~{hist_tokens:6,} tok")
    print(f"CUR_SCREEN text                  {cur_screen_text:7,} chars  ~{cur_tokens:6,} tok")
    print(
        f"Images x{cur_screen_images} base64               {image_base64_est:7,} chars  ~{image_tokens_est:6,} tok (rough)"
    )
    print(f"JSON wire tail                   {wire_tail:7,} chars  ~{wire_tokens:6,} tok")
    print()

    grand_tokens_text = sys_tokens + hist_tokens + cur_tokens + wire_tokens
    grand_tokens_all = grand_tokens_text + image_tokens_est
    print("=== SHARE OF TOTAL EST. TOKENS ===")
    for label, tk in [
        ("System (cacheable)", sys_tokens),
        ("History", hist_tokens),
        ("CUR_SCREEN text", cur_tokens),
        ("JSON wire tail", wire_tokens),
        ("Images (3 slots)", image_tokens_est),
    ]:
        print(f"{label:25} ~{tk:6,} tok  {tk / grand_tokens_all * 100:5.1f}%")
    print(f"Grand total               ~{grand_tokens_all:,} tokens")
    print()

    cs = ""
    for m in msgs:
        if isinstance(m.get("content"), list):
            for p in m["content"]:
                if p.get("type") == "text" and "[CUR_SCREEN]" in p.get("text", ""):
                    cs = p["text"]
    if cs:
        print("=== CUR_SCREEN TEXT BREAKDOWN ===")
        cs_total = len(cs)
        blocks = [
            ("Preamble / slot rules", cs[: cs.find("[Recent desktop")]),
            (
                "Recent tool calls history",
                slice_between(cs, "[Recent desktop tool calls", "**Pointer position**"),
            ),
            (
                "Pointer + analysis rules",
                slice_between(cs, "**Pointer position**", "**Nearby overlay"),
            ),
            (
                "Nearby bboxes (10 rows)",
                slice_between(cs, "**Nearby overlay reference bboxes**", None),
            ),
        ]
        for name, sub in blocks:
            print(f"{name:30} {len(sub):6,} chars  {len(sub) / cs_total * 100:5.1f}%")
        print(f"{'CUR_SCREEN text total':30} {cs_total:6,} chars")
        print()

    print("=== TOOLS APPENDIX SUB-BREAKDOWN ===")
    tools = sections["Tools appendix"]
    tool_parts = [
        ("action_verify", "### action_verify", "### captcha_verify"),
        ("captcha family", "### captcha_verify", "### clipboard_read"),
        ("clipboard", "### clipboard_read", "### hotkey"),
        ("hotkey", "### hotkey\nDescription:", "### modified_click"),
        ("modified_click", "### modified_click", "### mouse_click"),
        ("mouse_*", "### mouse_click", "### `task_board`"),
        ("task_board", "### `task_board`", "### wait"),
        ("wait", "### wait", None),
    ]
    tools_total = chars(tools)
    for name, start, end in tool_parts:
        sub = slice_between(tools, start, end)
        if not sub:
            continue
        print(f"{name:20} {chars(sub):7,} chars  {chars(sub) / tools_total * 100:5.1f}% of tools")


if __name__ == "__main__":
    main()

# Prompt token baseline

Baseline for comparing prompt size across git iterations.
Regenerate with:

```bash
pip install transformers modelscope pyyaml  # once
python3 -c "from modelscope import snapshot_download; snapshot_download('Qwen/Qwen3.5-0.8B', allow_file_pattern=['tokenizer.json','tokenizer_config.json','vocab*','merges.txt'])"
python3 scripts/count_prompt_tokens.py --update-baseline
```

## Git snapshot

| Field | Value |
|-------|-------|
| Commit | `e0eb3d148516805118e363694715d3c7f3b7aa25` |
| Short | `e0eb3d1` |
| Date | 2026-06-05 18:09:18 +0800 |
| Subject | 经验集成 |
| Baseline generated (UTC) | 2026-06-05T15:38:54Z |

## Tokenizer

- Model: `Qwen/Qwen3.5-0.8B` (same vocab family as hosted **qwen3.5-plus**)
- Local cache: `Qwen3___5-0___8B` under ModelScope hub (override with `--tokenizer-path`; default see script)
- Text: `len(tokenizer.encode(text, add_special_tokens=False))`
- Images: `Qwen3.x formula: (h_bar*w_bar)/(32*32)+2; not text tokenizer`

## Static — Primary cacheable system prompt

Runtime merge: `COMMUNICATION_PUBLIC` + tier slice (`communication` + `---` + `os` + `---` + `loop`) `## Tools` appendix + `[Environment]`. Matches `single_agent_prompt.rs`.

**Total: 18,332 tokens** (68,879 chars)

| Section | Chars | Tokens | Share |
|---------|------:|-------:|------:|
| COMMUNICATION_PUBLIC | 10,736 | 2,744 | 15.0% |
| tier slice (communication + os + loop) | 35,167 | 9,444 | 51.5% |
| Tools appendix (deduped) | 22,440 | 6,077 | 33.1% |
| [Environment] stub (~530 chars) | 530 | 67 | 0.4% |

Tools appendix dedup: 22,414 chars (pre-dedup would be 51,793 chars).

### Tools appendix per file (deduped, each once)

| File | Tokens |
|------|-------:|
| mouse.md | 434 |
| input.md | 630 |
| modified_click.md | 238 |
| captcha_verify.md | 536 |
| clipboard.md | 315 |
| hotkey.md | 430 |
| wait.md | 481 |
| action_verify.md | 564 |
| task_board.md | 2,446 |

## Native OpenAI tools (API only)

**2,974 tokens** (13,903 chars JSON) · 34 flat tools

_Not in `llm_prompts` dump files; included in provider `usage.prompt_tokens`._

Descriptions are **compact** when flat tools share a `doc_source` (full prose once in system Tools appendix).

## Dynamic — Computer Primary per-round

Image assumption: 1920x1080 monitor JPEG full capture → 1920x1088 → **2,042 tokens/image**.

**round_content** = cacheable system + `[CUR_SCREEN]` text + image slot labels + images.

**api_prompt_estimate** = round_content + native tools JSON + dialog history (user goal + tool results).

| Scenario | CUR_SCREEN | Labels | Images | Dialog | round_content | **api_estimate** |
|----------|----------:|-------:|-------:|-------:|--------------:|-----------------:|
| first_turn_minimal | 726 | 11 | 4,084 (2×2,042) | 0 | 23,153 | **26,127** |
| mid_task_typical | 978 | 17 | 6,126 (3×2,042) | 280 | 25,453 | **28,707** |
| heavy_history | 1,154 | 17 | 6,126 (3×2,042) | 535 | 25,629 | **29,138** |

### `first_turn_minimal`

No tool history, 2 images (after + annotated), no prior dialog


### `mid_task_typical`

5 history rows, 3 images (before + after + annotated), ~3 tool rounds dialog


### `heavy_history`

10 history rows (cap), 3 images, ~6 tool rounds dialog


## Runtime dump reference (optional)

Latest local dump: `{app_data}/logs/llm_prompts/example.json`
Model: `qwen3.5-plus` · mtime: 2026-06-03T00:49:03.301654+00:00

| Field | Tokens |
|-------|-------:|
| system (as dumped) | 14099 |
| `[CUR_SCREEN]` text | 1637 |
| `[TASK_BOARD]` | None |
| image slots | 3 (vision tokens not in dump) |

_Historical dump; may predate current system size or omit tools[] from file._

## Dump reconciliation

File: `{app_data}/logs/llm_prompts/example.json`

| Component | Tokens |
|-----------|-------:|
| Dump messages (text) | 16,272 |
| Dump images (3× formula) | 6,126 |
| Native tools (estimate) | 2,974 |
| **Full API estimate** | **25,372** |

_Dump excludes tools[]; add native_tools_tokens_estimate for API prompt_tokens parity. API may differ slightly due to chat-template special tokens and provider-side counting._

## Compare with a later ref

```bash
python3 scripts/count_prompt_tokens.py HEAD
python3 scripts/count_prompt_tokens.py --json > /tmp/prompt-metrics.json
git diff --no-index scripts/prompt-token-baseline.json /tmp/prompt-metrics.json
```

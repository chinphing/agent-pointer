# Prompt token baseline — 2026-06-15

Tokenizer: **cl100k_base (tiktoken)** — pip-installable BPE, same family as Qwen3.5.

Git: `368e8a6` — grep调用加强path参数 (2026-06-15)

## 三智能体总览

| 维度 | Computer (Primary) | Coder (direct) | Explore (sub-agent) |
|------|-----------:|------------:|--------------:|
| 缓存 prompt (in-prompt text) | **17,376** | **17,752** | **9,826** |
| 原生 API tools[] | **2,973** | **1,532** | **606** |
| **基础静态合计** | **20,349** | **19,284** | **10,432** |
| 首轮 total | **25,052** | **19,349** | **10,499** |
| 完整轮次 | 25k–28k | 19k–21k | 10k–11k |
| 原生工具数量 | 34 | 17 | 6 |

---

## Computer (Primary)

### 缓存 system prompt 组成

| 切片 | Chars | Token | 占比 |
|------|------:|------:|-----:|
| COMMUNICATION_PUBLIC | 3,254 | 753 | 4.3% |
| tier slice (comm + os + loop) | 37,484 | 9,509 | 54.7% |
| Tools appendix (去重 9 文件) | 27,241 | 7,047 | 40.6% |
| [Environment] stub | 530 | 67 | 0.4% |
| **缓存合计** | **68,515** | **17,376** | |

Tools appendix 去重前: 76,408 chars → 去重后: 27,215 chars。

#### Tools appendix per file

| File | Token |
|------|------:|
| mouse.md | 813 |
| input.md | 842 |
| modified_click.md | 227 |
| captcha_verify.md | 518 |
| clipboard.md | 582 |
| hotkey.md | 553 |
| wait.md | 456 |
| action_verify.md | 533 |
| task_board.md | 2,519 |

### 原生 API tools[]（34 个）

JSON 总长: 13,903 chars → **2,973 tok**。描述全部为 compact（共享 doc_source 时用简短替代，全文在 Tools appendix 中）。

| 工具 | Token |
|------|------:|
| action_verify | 97 |
| captcha_verify_click | 74 |
| captcha_verify_drag | 97 |
| captcha_verify_type | 86 |
| clipboard_read | 58 |
| clipboard_write | 66 |
| hotkey | 98 |
| input_at | 110 |
| input_focused | 96 |
| input_index | 102 |
| modified_click_range_select_at | 86 |
| modified_click_range_select_index | 93 |
| modified_click_select_at | 84 |
| modified_click_select_index | 91 |
| mouse_click_at | 90 |
| mouse_click_index | 82 |
| mouse_double_click_at | 92 |
| mouse_double_click_index | 84 |
| mouse_drag_from_to_at | 118 |
| mouse_drag_from_to_index | 98 |
| mouse_hover_at | 90 |
| mouse_hover_index | 82 |
| mouse_right_click_at | 92 |
| mouse_right_click_index | 84 |
| mouse_scroll_current | 82 |
| mouse_scroll_index | 90 |
| task_board_check_deps | 66 |
| task_board_finalize | 55 |
| task_board_init | 135 |
| task_board_patch | 103 |
| task_board_prune | 70 |
| task_board_replace | 82 |
| task_board_sync_finding | 66 |
| wait | 72 |

### 动态场景（Primary 轮次）

图片假设: 1920×1080 → 1920×1088 → **2,042 tok/图** (Qwen3.x formula)。

**round_content** = 缓存系统 + CUR_SCREEN 文本 + 图片标签 + 图片。
**api_estimate** = round_content + native tools + dialog history。

| 场景 | CUR_SCREEN | 图片 | 对话 | round_content | **api_estimate** |
|------|----------:|-----:|-----:|--------------:|-----------------:|
| first_turn_minimal | 610 | 4,084 (2×) | 0 | **22,079** | **25,052** |
| mid_task_typical | 839 | 6,126 (3×) | 273 | **24,355** | **27,601** |
| heavy_history | 994 | 6,126 (3×) | 522 | **24,510** | **28,005** |

---

## Coder (direct)

### 缓存 system prompt 组成

| 切片 | Chars | Token |
|------|------:|------:|
| COMMUNICATION_PUBLIC | 3,254 | 753 |
| MEDIA_DELIVERY | 1,929 | 469 |
| agent_prompt + compose_system_prompt | 23,748 | 5,972 |
| Tools appendix (6 文件) | 38,152 | 10,490 |
| [Environment] stub | 530 | 67 |
| **缓存合计** | **67,621** | **17,752** |

### composed_system_body 切片（内部 9 个 .md）

| 切片 | Chars | Token |
|------|------:|------:|
| role | 2,795 | 650 |
| routine | 3,028 | 767 |
| delegation | 2,317 | 559 |
| task_board | 1,955 | 518 |
| scenario_impl | 710 | 167 |
| scenario_debug | 924 | 209 |
| scenario_refactor | 610 | 129 |
| scenario_design | 539 | 128 |
| scenario_spec | 415 | 93 |
| **composed_system_body 合计** | **13,368** | **3,234** |

### 原生 API tools[]（17 个）— 1,532 tok

所有工具均用 compact 描述（共享 doc_source → 简短一行，全文在 Tools appendix）。

| 工具系列 | 工具名 | Token | 说明 |
|---------|-------|------:|------|
| **file** | file_read | 123 | share file.md (6 peers) |
| | file_write | 58 | |
| | file_edit | 94 | |
| | file_glob | 95 | |
| | file_grep | 148 | schema 最大（含 includeGlobs 等） |
| | file_list | 88 | |
| **工具** | read_lints | 75 | 独立 doc source |
| | terminal | 123 | 独立 doc source |
| | run_subagent | 97 | 独立 doc source |
| | web_search | 52 | 独立 doc source，schema 最简 |
| **task_board** (7) | task_board_init | 135 | share task_board.md (7 peers) |
| | task_board_replace | 82 | |
| | task_board_patch | 103 | |
| | task_board_prune | 70 | |
| | task_board_finalize | 55 | 最小 |
| | task_board_sync_finding | 66 | |
| | task_board_check_deps | 66 | |

### 动态场景

| 场景 | 任务板增量 | round_content (含 native) |
|------|----------:|-------------------------:|
| first_turn | 0 | **19,284** |
| mid_task | 600 | **19,884** |
| with_history | 1,500 | **20,784** |

round_content = 缓存 prompt (17,752) + native tools (1,532) + env(67) + task_board 增量。

---

## Explore (sub-agent)

### 缓存 system prompt 组成

| 切片 | Chars | Token |
|------|------:|------:|
| COMMUNICATION_PUBLIC | 3,254 | 753 |
| MEDIA_DELIVERY | 1,929 | 469 |
| sub_agent_header (COMMUNICATION.md + composed_system_body) | 11,890 | 2,857 |
| Tools appendix (2 文件) | 20,179 | 5,678 |
| [Environment] stub | 530 | 67 |
| **缓存合计** | **37,790** | **9,826** |

### composed_system_body 切片（内部 17 个 .md）

| 切片 | Chars | Token |
|------|------:|------:|
| role | 638 | 151 |
| router | 504 | 118 |
| flow_standard | 439 | 100 |
| flow_narrow | 344 | 89 |
| flow_reach | 434 | 109 |
| scenario_single | 275 | 57 |
| scenario_cross | 323 | 67 |
| scenario_arch | 238 | 51 |
| scenario_reach | 207 | 45 |
| scenario_spec | 189 | 39 |
| scenario_debug | 486 | 105 |
| scenario_design | 273 | 58 |
| impact_scan | 1,380 | 307 |
| handoff_contract | 1,790 | 469 |
| trace_when | 958 | 258 |
| file_discipline | 913 | 253 |
| deliverable | 615 | 161 |
| **composed_system_body 合计** | **10,129** | **2,459** |

### 原生 API tools[]（6 个）— 606 tok

| 工具 | Token |
|------|------:|
| file_read | 123 |
| file_write | 58 |
| file_edit | 94 |
| file_glob | 95 |
| file_grep | 148 |
| file_list | 88 |

Explore 只读子智能体，仅 6 个文件工具（无 terminal / web_search / run_subagent 等）。

### 动态场景

| 场景 | 任务板增量 | round_content (含 native) |
|------|----------:|-------------------------:|
| first_turn | 0 | **10,499** |
| typical | 300 | **10,799** |

---

## 对比要点

1. **Coder vs Computer** 缓存大小接近（17.75k vs 17.38k），但组合不同——Coder 的 Tools appendix 占大头（10.5k），Computer 的 tier slice 占大头（9.5k）。

2. **Explore 仅 10.4k**——作为只读子智能体，没有历史管理、tier/loop prompt、Media delivery 只在 header 中。

3. **原生 tools[] 成本**：Computer 最大（2,973 tok, 34 工具），Coder 居中（1,532 tok, 17 工具），Explore 最小（606 tok, 6 工具）。文件工具的 schema 最重（file_grep 148 tok）。

4. 与旧基线（Qwen tokenizer, `e0eb3d1`）的差异主要来自 tokenizer 切换和 prompt 内容变更，不可直接比较。

---

## 生成

```bash
pip install tiktoken pyyaml
python3 scripts/count_prompt_tokens.py --update-baseline
```

或查看 JSON 含完整每工具明细：

```bash
python3 scripts/count_prompt_tokens.py --json
```

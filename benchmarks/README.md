# 定位链路评测 · 主入口

**按「测哪个环节」查，不按 benchmark 名字查。**
改哪一段代码，就查对应那一节；每节给出该环节的评测、命令、当前基线、状态。

- 数据集来源与规模 → [`pointer-eval/datasets.md`](../../pointer-eval/datasets.md)（辅助）
- 历史基线按时间倒序 → [`pointer-eval/baselines/`](../../pointer-eval/baselines/)
- 本节只回答一个问题：**我要改的东西，用什么测？**

---

## 链路与环节

定位层只有两件事：**产框**（relocate）和**判框**（verify_position）。
两者各自可以独立替换模型、独立调提示词、独立失败。

```
用户一句话
   │
   ├─ 环节 A：主 LLM 出框 ── 写 target_desc + bbox（hint）
   │        ↓
   ├─ 环节 B：relocate ── 产框
   │        起点两种：无 hint → 整屏找；有 hint → 直接用
   │        之后统一：crop → verify → refine（细调 / 扩大重找）循环 ≤3 轮
   │        ↓
   ├─ 环节 C：verify_position ── 判框（B 内部每轮都调）
   │        ↓
   ├─ 环节 D：动作执行
   │        ↓
   └─ 环节 E：verify_action ── 动作后界面校验
```

> **B 和 C 是循环关系，不是串联。** relocate 产一个框，verify_position 判一次；
> 判否就回 relocate 重来。分开列是因为它们能独立坏、独立测。

---

## 环节 A — 主 LLM 写描述 + 出粗框

**改什么会落到这一环**：`prompts/communication.md`（`## Positioning`、`### Writing target_desc`）、`AGENT.md`、工具 schema 里 `target_desc` / `bbox` 的说明。

| 项 | 内容 |
|---|---|
| 评测 | `full_chain` 的 **hint** 模式（= 数据集分段 `main_llm_bbox`） |
| 数据 | `pointer-eval-data/full_chain/sudoku/`，**104 条** |
| 当前基线 | **90.7 %**（三次均值；单遍历史基线 86.5 %） |
| 耗时 | 104 例约 130 秒（每例一次模型调用） |
| 文档 | [`full_chain/BASELINE.md`](full_chain/BASELINE.md) |

```bash
cd agent-pointer
POINTER_E2E_SPLIT=sudoku POINTER_E2E_MODE=hint POINTER_E2E_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test full_chain \
  full_chain_grounding -- --ignored --nocapture
```

**判据**：主 LLM 给的 bbox 中心是否落在真值框内。只做第一次模型调用就计分，不进定位层。

> 104 条极差 4.8 pp，**结论必须基于 ≥3 次**。

---

## 环节 B — relocate（产框）

**改什么会落到这一环**：`locate/prompts/locate.md`、`locate/geometry.rs`（裁片参数 `CROP_MULT` / `CROP_MIN` / `SMALL_TARGET_*` / `CROP_MAX`）、`locate/engine.rs`。

### 内部结构

```rust
// engine.rs:406 —— 有 hint 时整段跳过
if box_norm.is_none() {
    ...full_init...   // 整屏找
}

// engine.rs:426 —— 无条件进入，两条路径在这里汇合
loop {
    crop → verify → refine / widen     // ≤ max_locate 轮
}
```

**两条路径只差起点，进循环后完全相同**（同一个 `crop_side_px`、同一套 `verify_pair`、同样的 refine 分支）。

| 起点 | 何时 | 说明 |
|---|---|---|
| **整屏找**（`full_init`） | 无 hint | 从整屏 + 描述产第一个框 |
| **直接用 hint** | 有 hint | 主 LLM 的 bbox 直接当初始框 |

`full_init` **不占 verify 轮次**（`rounds` 计数器在它之后才用），所以无 hint 时总模型调用 = 1 + `max_locate`。

循环里判否有两种走法：

| 分支 | 代码 | 语义 |
|---|---|---|
| `crop_r{n}` | `engine.rs:732` | 在当前框附近**细调** |
| `full_widen_r{n}` | `engine.rs:774` | **扩大裁片重找**（框整体错了） |

### 评测

| 评测 | 起点 | 数据 | 当前基线 |
|---|---|---|---|
| `screenspot_pro` **M2** | **无 hint**（`mode=none`） | 300 条，26 个专业软件，4K，目标 15–1000 px | **80.4 %**（3 次均值，极差 1.6 pp） |
| `screenspot_grid` | 无 hint | 见 [`pointer-eval/screenspot_grid/`](../../pointer-eval/screenspot_grid/) | 见目录 |
| `full_chain` **agent** | **有 hint**（生产路径） | 104 条 | **95.2 %** |

```bash
POINTER_SP_SPLIT=M2 POINTER_LOCATE_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test screenspot_pro \
  screenspot_pro -- --ignored
```

**80.4 % 和 95.2 % 的 8.7 pp 差，主要是起点难度差**——整屏找比从 hint 出发难得多，流程本身相同。

**M2 已知短板**：

| 目标尺寸 | n | 命中率 |
|---|---|---|
| 0–20 px | 14 | **43 %** |
| 20–40 px | 88 | **66 %** |
| 40–80 px | 72 | 89 % |
| 80–160 px | 61 | 93 % |
| 160+ px | 65 | 83 % |

0–40 px 的 102 条（34 %）是主要失分点。

### refine 分支（`crop_r` / `full_widen_r`）

**改什么会落到这一环**：`locate/engine.rs` 的 refine 分支、`locate/prompt.rs` 的 `refine_hint`、`locate/types.rs` 的 `failed_checks`（决定 hint 说什么）。

| 项 | 内容 |
|---|---|
| 评测 | `refine_loop_eval` |
| 数据 | 复用 `verify_position` 的 148 条平移样本（`expect.center == "no"`） |
| 当前基线 | **85.59 %**（3 次均值，极差 **9.5 pp**） |
| 判分 | **最终交付点是否落回 `gt_px`**（不是 verify 判得对不对） |

```bash
POINTER_REFINE_EVAL_CONCURRENCY=8 \
POINTER_REFINE_EVAL_ALL_OUT=/tmp/refine/all.jsonl \
POINTER_REFINE_EVAL_FAILS_OUT=/tmp/refine/fails.jsonl \
  cargo test --release -p pointer-core --test refine_loop_eval -- --ignored --nocapture
```

**内部结构（比总分稳定得多）**：

| rounds | 含义 | 命中率 |
|---|---|---|
| 1 | verify 假阳性放行，refine 从未触发 | **0 %**（点全不在目标上） |
| 2 | 第 1 次 refine 就纠回来 | **99.1 / 99.2 %** |
| 3 | 第 2 次 refine 才纠回来 | 27.8 / 35.7 % |

**总分 9.5 pp 的抖动几乎全部来自 `rounds=1` 的条数（6 ↔ 16）** —— 也就是 verify 假阳性率（4.1 %–10.8 %）。
要提升端到端效果，优先压 verify 假阳性，而不是改 refine 策略。

详见 [`pointer-eval/baselines/verify-refine-baseline-2026-11-01.md`](../../pointer-eval/baselines/verify-refine-baseline-2026-11-01.md)。

---

## 环节 C — verify_position（判框）

**改什么会落到这一环**：`locate/prompts/verify_body.md`、`verify_intro_wide.md`、`verify_intro_close.md`、`locate/render.rs`（标记怎么画）。

| 项 | 内容 |
|---|---|
| 评测 | `verify_position` |
| 数据 | `pointer-eval-data/verify_position/`，**259 条**（37 case × 7 变体：pos/up/down/left/right/big/small） |
| 当前基线 | **93.44 %**（3 次均值，单次 93.05 / 93.05 / 94.21） |
| 生成器 | `pointer-eval/scripts/verify_position_gen.py` |

**分变体（3 次均值）**：pos / big / small **100 %**；down 94.6 %、up 94.6 %；left **84.7 %**、right **80.2 %**。

**三次交集**：225 条全对 / 30 条偶发 / **仅 4 条固定失败**（`06a4f464_left`、`06a4f464_right`、`c8a35d0d_left`、`ed32a73f_right`）。

> ⚠️ 旧的 94.2 % 已作废——那是生成器参数与生产不一致时（close 1920 而非 1600）跑出来的。
> 现行数字见 [`pointer-eval/baselines/verify-refine-baseline-2026-11-01.md`](../../pointer-eval/baselines/verify-refine-baseline-2026-11-01.md)。

```bash
# 改过 render.rs 或生成器后，必须先重新生成数据
cd pointer-eval/scripts && python3 verify_position_gen.py

# 再跑，3 次取均值
cd agent-pointer
POINTER_VERIFY_EVAL_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test verify_position_eval -- --nocapture
```

> ⚠️ **`--release` 不是可选项。** `locate_one` 每轮都同步渲染图片，
> debug 下 render 一次 **7129 ms**，release 只要 **457 ms**（15×）。
> 少了 `--release`，259 条要跑十几分钟，容易误判成"卡死"。
> 本仓所有跑模型的评测（`full_chain` / `screenspot_pro` / `verify_position` /
> `refine_loop` / parity）**一律加 `--release`**。

**渲染一致性必须先过**（Python 生成的图 ↔ Rust 生产的图）：

```bash
cargo test --release -p pointer-core --lib render::tests::dump_render_for_parity -- --ignored --nocapture
cd pointer-eval/scripts && python3 parity_check.py
# 期望：逐像素完全一致: (1600, 1600)  tol_r=33.33
```

**已知偏斜**：259 条里 189 条是数独（73 %），非数独只有 10 个 case。

---

## 环节 D — 动作执行

不属于定位评测。动作层的正确性由 `full_chain` 的 agent 模式间接覆盖。

---

## 环节 E — verify_action（动作后校验）

**改什么会落到这一环**：`prompts/modules/verify/*.md`、`verify.rs`。

| 项 | 内容 |
|---|---|
| 评测 | `verify_action` |
| 数据 | `pointer-eval-data/verify_action/`，**sudoku 209 条** |
| 当前基线 | 见 [`pointer-eval/baselines/`](../../pointer-eval/baselines/) |
| 状态 | 数据可用 |

---

## 全链路 — A+B+C+D+E

| 项 | 内容 |
|---|---|
| 评测 | `full_chain` 的 **agent** 模式 |
| 数据 | 同 `full_chain`，104 条 |
| 当前基线 | **95.2 %**（2026-10-30，单遍） |
| 与 hint 的差 | **8.7 pp = 定位层能补回的量** |
| 耗时 | hint 7.8 s + locate 8.9 s / 例 |

```bash
POINTER_E2E_SPLIT=sudoku POINTER_E2E_MODE=agent POINTER_E2E_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test full_chain \
  full_chain_grounding -- --ignored --nocapture
```

**验收用这个，调 prompt 用 hint。**

---

## 一页速查

| 我改了… | 跑这个 | 基线 |
|---|---|---|
| `communication.md` / `target_desc` 说明 | `full_chain` hint（`SPLIT=sudoku`） | 90.7 %（3 次均值） |
| `locate.md` / 裁片参数 | `screenspot_pro` M2 | 80.4 % |
| `verify_body.md` / `render.rs` | `verify_position`（先重新生成） | **93.44 %**（3 次均值） |
| refine 分支（`crop_r` / `full_widen_r`） | `refine_loop_eval`（148 条） | **85.59 %**（3 次均值，极差 9.5 pp） |
| `verify/*.md` / `verify.rs` | `verify_action` | 见 baselines |
| 想确认整体没退 | `full_chain` agent | 95.2 % |

---

## 已知的坑

| 坑 | 说明 |
|---|---|
| `--test computer_e2e` 报错 | 已改名 `--test full_chain`，函数是 `full_chain_grounding` |
| `SCOPE=full` 现在跑 404 条 | 2026-11-01 数据集重组：原 104 条 → 现在 404 条（并入 screenspot 300）。**一律改用 `SPLIT=sudoku`** |
| `verify_position` 数据过时 | 改了 `render.rs` 或生成器后**必须重新生成**，否则评的是旧图 |
| **单次结果不可信** | `refine_loop` **同代码同数据两次差 8.1 pp**（87.8 vs 79.7）；`verify_position` 极差 2.3 pp。**跑模型的评测一律 ≥3 次取均值 + 报极差** |
| **不能靠 grep stdout 取结果** | 必须用落盘变量：`POINTER_VERIFY_EVAL_ALL_OUT` / `_FAILS_OUT`、`POINTER_REFINE_EVAL_ALL_OUT` / `_FAILS_OUT`。日志会被 `tail` 截断，明细丢了无法回溯 |
| **少 `--release` 慢 15×** | debug 下 render 7129 ms vs release 457 ms。259 条会跑十几分钟，容易误判成卡死 |
| 数独占比 73 % | `verify_position` 259 条里 189 条是数独 |
| `full_chain` 真值框 | 51 条数独真值按「描述行列 + 9×9 格阵」重算，不是 pipeline 点出来的框 |
| 两个基线不可直接比 | `screenspot_pro` 80.4 % 是无 hint 起点、`full_chain` agent 95.2 % 是有 hint 起点 |
| `benchmarks/` 混装 | 本目录还有 `deepswe` / `terminal_bench` / `frontend_perf`，**与定位无关** |

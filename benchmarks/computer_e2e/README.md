# Computer-use 端到端定位 Benchmark

**给「真实主 LLM + locate 全链路」做端到端评测。** 输入只有一张全屏截图 + 一句 goal，
输出一个落点，用和 [`screenspot_grid`](../screenspot_grid/README.md) 完全相同的
指标（命中率 + 尺寸分档 + 框包裹度）打分。

数据集来自 **2026-10-28 一次真实生产会话**（`0a8eeb49`，17:00–18:57，116 分钟），
不是合成样本：每一条都是 agent 当时真的看着这张图、按这个 goal 点下去的。

---

## 和 screenspot_grid 的关系

| | `screenspot_grid` | `computer_e2e`（本目录） |
|---|---|---|
| 输入 | 元素描述（外观/位置/锚点 4 字段） | **agent 自己写的 goal**（`target_desc`） |
| 起点 | 无 hint，直接定位 | **主 LLM 先给 bbox，再进 locate** |
| 测什么 | 定位层 | **规划层 + 定位层合起来** |
| 真值来源 | 人工标注 | **pipeline 实际点中并 verify 通过的框** |
| 指标 | 同一套 `metrics.py` 口径 | 同左 |

**为什么要单独做一个**：`screenspot_grid` 喂的是人工写好的描述，绕过了主 LLM。
生产里主 LLM 要先把「用户的一句话」变成 bbox，这一步的误差会直接传给 locate。
本 benchmark 就是把这步算进去。

---

## 数据集

`data/e2e_truth.json` — 105 cases，一 case 一图。

```json
{
  "scene": "0069815c",
  "description": "数独盘面C行…第五列的空单元格，位于列标签5下方…",
  "goal": "同上",
  "bbox": [263.04, 327.24, 303.36, 358.56],
  "fixture": "0069815c_frame.jpg",
  "kind": "e2e",
  "size": [1920, 1080]
}
```

| 项 | 值 |
|---|---|
| cases | **105**（118 trace − 13 个 `outcome.ok=false`） |
| 图尺寸 | 1920×1080 × 104，**1512×982 × 1**（中途换过显示器） |
| 唯一图 | 105，无共享 |
| 真值 | `outcome.box`——pipeline 点下去且 verify 通过的框 |

**`bbox` 是「pipeline 认为对」的框，不是人工标注。** 这带来一个已知偏差：
如果 verify 本身判错，真值就是错的。冒烟测试里已经出现过一次——verify 说
`结论：yes`，但落点在 `bbox` 外。这类样本目前保留，因为它们是**生产真实
判定的完整记录**，剔除会掩盖 verify 的假阳性率。

### 图片在哪

不随仓库分发（105 张 / 约 30 MB）。默认从生产 dump 目录读：

```
~/Library/Application Support/PointerApp/computer-captures/locate/
```

换位置用 `POINTER_E2E_IMAGE_DIR`。

**图片不带指针叠加**（option A）。生产给模型看的 `screenshot_jpeg` 带指针，
但测试用的是 locate 自己的 `frame.jpg`（`render_full()` 产物），两者同尺寸
1920×1080，差别只在指针图标。

---

## 三类验证数据怎么来

生产跑一次真实任务，会留下三层可回放的记录。三层粒度不同，用途也不同：

| 文件 | 粒度 | 一行是什么 | 用途 |
|---|---|---|---|
| `locate.jsonl` | 每次定位 LLM 调用 | 一张 crop + 目标描述 + 真值框 | 定位准确率、首次命中率 |
| `verify.jsonl` | 每次 locate 内部 verify | 一张 close + 一张 wide + 三项判定 | locate verify 的假阳/假阴 |
| `pipeline_verify.jsonl` | 每次工具执行后的 host verify | `Executed …` + `Verify: pass/fail — step_summary` | 操作级验证、`step_summary` 质量 |

### locate + verify

```bash
cd pointer-eval
python3 scripts/export_datasets.py <OUT_DIR> [SINCE_ISO]
```

`SINCE_ISO` 为空表示导出全部 trace。输出两个文件：

**`locate.jsonl`** — `run_id` / `started_at` / `round` / `where` / `image` /
`bounds` / `frame_size` / `target_desc` / `goal` / `prev_box` / `model_raw` /
`model_box` / `not_found` / `answer_box` / `answer_box_frame` / `target_visible`

**`verify.jsonl`** — `run_id` / `round` / `close_image` / `wide_image` /
`target_desc` / `box` / `frame_size` / `labels{center,size,position,notes}` /
`render_ms` / `call_ms`

**只有 `outcome.ok == True` 的 trace 会进来**，所以 `answer_box` 是 pipeline
真的点下去并确认过的框——不是人工标注。verify 判错的样本会带着错的真值进来，
这是有意的（见「已知局限」）。

### pipeline_verify

```bash
cd pointer-eval
python3 scripts/export_pipeline_verify.py <OUT_JSONL> \
    [--conversation <id>] [--since <ISO>] [--db <path>]
```

**从 `conversations.db` 读，不要读日志。** 日志是调试流：应用重启时轮转，
下一个 run 开始时当前文件被截断，所以跨重启的会话会被切开、尾部直接消失。

实测代价：2026-10-29 那局数独，从日志里只捞到前十分钟 350 条，**全部是
`pass`**；同一会话在数据库里有 455 条，到最后一个动作为止，**含 53 条 `fail`**。
拿日志当数据源会把「一次失败都没有」当成结论。

数据库里这些消息是 `role='tool'`，`content` 形如 `[tool] 执行摘要\nVerify: …`，
`payload` 另存 `createdAt`（epoch ms）——截图文件名就是按这个时间戳命名的，
所以只有从数据库读才接得上图。

```json
{"conversation_id": "0a8eeb49-…",
 "created_at_ms": 1793243355152,
 "executed": "Executed mouse_click_at: 选中I1空格，准备填入数字5",
 "verdict": "pass",
 "step_summary": "I1 cell: unselected → highlighted blue (selected); …",
 "shots": ["0a8eeb49-…_96_1793243355152_verify_screen_before_action.jpg", "…after_action.jpg"]}
```

`shots` 不是导出时写的，是按 `created_at_ms` 在
`computer-captures/<date>/<conversation>/` 里 ±3 秒匹配出来的——**截图和日志行
之间没有关联 id，只有时间**。实测 455 条里 297 条（65%）能配上，其余是截图没
保存或已清理。

### 统计口径

**首次命中率要按 `run_id` 分组，不能按 `target_desc`。** 不同会话里同一句
描述（「保存按钮」）是完全不同的定位任务，按描述分组会把几十次独立定位算成
一个多轮任务，得出「21 轮」这种没有意义的数字。

```python
g = defaultdict(list)
for r in rows:
    g[r["run_id"]].append(r)
first_hit = sum(1 for v in g.values() if max(r["round"] for r in v) == 1)
```

### 数据存哪

`pointer-eval-data/<date>-<topic>/`，不进 git（`pointer-eval` 只存脚本和基线）。

| 目录 | 内容 |
|---|---|
| `pointer-2026-10-28/` | 早期一轮的 201 locate + 236 verify + 472 张图 |
| `pointer-2026-10-28-locate-verify/` | 10-28 全量：**1100 locate + 1907 verify + 4914 张图**（987 MB） |
| `pointer-2026-10-28-sudoku/` | 同一个数独会话，209 条 pipeline verify + 418 张图 |
| `pointer-2026-10-29-sudoku/` | 10-29 的 91 locate + 202 verify + 88 pipeline verify + 671 张图 |

**`pointer-2026-10-28/` 是 `pointer-2026-10-28-locate-verify/` 的子集**（同一轮
trace 的前 201 条），保留是因为已有基线跑在它上面。

**数独会话 `0a8eeb49-…` 是一个跨 5 天的长会话**，pipeline verify 共 455 条，
但只保留有截图的 10-28 和 10-29：

| 日期 | 条数 | 其中 fail | 截图 |
|---|---|---|---|
| 10-28 | 209 | 20 | ✅ 418 张 |
| 10-29 | 88 | 7 | ✅ 176 张 |
| **保留合计** | **297** | **27** | 594 张 |

**已删除** 10-25（92 条 / 10 fail）、10-26（22 条 / 2 fail）、10-27（44 条 /
14 fail）——截图已从 `computer-captures/` 清理，留着跑不了重放。

**10-29 的 locate/verify 只覆盖到 08:39**：那之后应用重启、日志轮转，trace 目录
里也没有新文件。pipeline verify 那层有数据库兜底，locate 这层没有。

---

## 四种 hint 模式

一次跑一种，用来把误差归因到正确的层：

| 模式 | hint 来源 | 回答什么问题 |
|---|---|---|
| `none` | 无 | **定位层单独能做到多少**（基线） |
| `oracle` | 真值框 | **这个 locate 版本的上限**（hint 完美时） |
| **`hint`** | **只问主 LLM 要 bbox，不进 locate** | **主 LLM 的 hint 有多准**（离线、不花 locate 的钱） |
| `agent` | 真实主 LLM 从 (图, goal) 产出 | **生产实际水平**——本 benchmark 存在的理由 |

**`hint` 和 `agent` 的区别：** `agent` 跑完整链路（hint → locate → verify），每例约 107 秒；
`hint` 只做第一次模型调用，每例不到 1 秒，105 例 8 并发约 70 秒。调 prompt 用 `hint`，
验收用 `agent`。

**三个数一起看才有意义：**

```
oracle - agent  →  主 LLM 的 hint 值多少
agent  - none   →  hint 是正资产还是负资产
```

---

## 跑

三个数据集，每个都能按**冒烟 / 小范围验证 / 全量**三档跑。分档按 **run 取样**，
不是按行截断——`locate.jsonl` 一行是一轮，`LIMIT=10` 会拿到同一个 run 的十轮，
说明不了别的目标还能不能定位。

```bash
# ── 数据集一：locate（computer_e2e_grounding）
POINTER_E2E_SCOPE=smoke  POINTER_E2E_MODE=none cargo test --release -p pointer-core --test computer_e2e -- --ignored --nocapture
POINTER_E2E_SCOPE=sample POINTER_E2E_MODE=none cargo test ...
POINTER_E2E_SCOPE=full   POINTER_E2E_MODE=none cargo test ...

# ── 数据集二：verify（computer_verify_e2e）
POINTER_E2E_SCOPE=smoke  cargo test --release -p pointer-core --test computer_e2e computer_verify_e2e -- --ignored --nocapture
POINTER_E2E_SCOPE=sample cargo test ... computer_verify_e2e ...
POINTER_E2E_SCOPE=full   cargo test ... computer_verify_e2e ...

# ── 单例（覆盖 scope）
POINTER_E2E_MODE=oracle POINTER_E2E_ONLY=0069815c cargo test ...
```

**三档的规模**（按 run 数取样，不是行数）：

| `POINTER_E2E_SCOPE` | 取多少个 run | 用途 |
|---|---|---|
| `smoke` | **10**（不足则全部） | 改完立刻跑，几十秒到一分钟 |
| `sample` | **20%**，但不少于 10 | 回归检查，不付全量的时间 |
| `full`（默认） | 全部 | 进基线文件的数字 |

**落到现有数据集上的实际条数：**

| 数据集 | 行数 | run 数 | `smoke` | `sample` | `full` |
|---|---|---|---|---|---|
| locate 10-28（全量） | 1100 | 831 | **29 行** | 259 行 | 1100 行 |
| locate 10-29 | 91 | 77 | **15 行** | 21 行 | 91 行 |
| verify 10-28（全量） | 1907 | 1145 | **29 行** | 392 行 | 1907 行 |
| verify 10-29 | 202 | 143 | **16 行** | 39 行 | 202 行 |

**冒烟是 29 行不是 10 行**，因为一个 run 平均有 1.3 轮，10 个 run 带出来的行数
超过 10。这正是按 run 取样的意义——10 行可能是同一个 run 的 10 轮。

**`sample` 有下限 10**：20% 在 12 个 run 的数据集上是 2，那会让「小范围」比「冒烟」
还弱。

**取样保持一个 run 的所有轮次**。`round 2` 只有在 `round 1` 旁边时才表示
「重试一次后纠正」；只留 `round 2` 会让它读成一次没重试的命中。

**`POINTER_E2E_LIMIT` 仍然可用，优先级最高**——给的是行数，跳过 scope 取样，
老命令不用改。

**环境变量：**

| 变量 | 作用 |
|---|---|
| **`POINTER_E2E_SCOPE`** | **`smoke` / `sample` / `full`（默认）。未知值直接 panic，不静默降级** |
| `POINTER_E2E_MODE` | `none` / `oracle` / `hint` / `agent`（默认 `none`） |
| `POINTER_E2E_LIMIT` | 只跑前 N **行**，覆盖 scope |
| `POINTER_E2E_ONLY` | 只跑某个 `scene`（locate）/ `run_id`（verify） |
| `POINTER_E2E_CONCURRENCY` | 并发数，默认 4。`hint` 模式可用 8；**Moonshot 组织 RPM 上限 100，必须用 `1`** |
| `POINTER_E2E_IMAGE_DIR` | 图片目录 |
| `POINTER_E2E_MINIMAL_PROMPT` | `1` = 用最小 prompt（约 400 字符），不用生产 28K |
| **`POINTER_E2E_BBOX_UNIT`** | **`pixel`（默认，与生产 `mouse.schema.yaml` 一致）/ `norm`（0-1000 归一化，实验臂）——两套最小 prompt 随之切换** |
| `POINTER_E2E_IMAGE_SIZE` | `1` = 在 `[CUR_SCREEN]` 里声明截图像素尺寸 + 换算说明（`pixel` 档自动开启） |
| `POINTER_E2E_PIXEL_BBOX` | 旧写法，等价于 `BBOX_UNIT=pixel`（保留兼容） |
| **`POINTER_E2E_TEMPERATURE`** | 采样温度，默认 `0.0`。**kimi-k3.1 只接受 `1.0`**，否则 400 |
| `POINTER_E2E_HISTORY_DUMP` | 从生产请求 dump 注入历史消息 |
| `POINTER_E2E_HISTORY_LIMIT` | 注入条数（默认 40） |
| `POINTER_E2E_AGENT_THINKING` | 主 LLM 思考（默认 `1`） |
| `POINTER_E2E_LOCATE_THINKING` | locate 思考（默认 `0`） |
| `POINTER_E2E_VERIFY_THINKING` | verify 思考（默认 `0`） |
| `POINTER_E2E_VERIFY_JSONL` | verify 数据集路径 |
| `POINTER_LOCATE_BASE_URL` / `_API_KEY` / `_MODEL` | provider（不设则读 `user_settings.json`） |

**耗时**：locate 约 **107 秒/例**（多轮 + verify），105 例全量 ≈ **3 小时**；
冒烟 10 个 run ≈ **5 分钟**。先跑 `smoke` 确认配置，再 `sample`，最后 `full`。

### 第三个数据集（pipeline verify）没有离线测试

`pipeline_verify.jsonl` 记的是**工具执行后**的 host verify——`submit_verify` 的
`action_result` / `failure_cause` / `step_summary`。它和 `verify.jsonl` 不是同一套
prompt：`verify.jsonl` 是 locate 内部的三项判定（center / size / position），
pipeline 那层是 `computer_verify_prompt(family)` + `submit_verify`。

要离线重放它，得从 jpg 造出 `ScreenCaptureResult` 再走
`build_verify_wire_messages`——`VerifyWireInput` 要的是 `before` / `after` 两个
`ScreenCaptureResult`，不是裸 jpeg。目前没做，`step_summary` 的质量只能用
`export_pipeline_verify.py` 导出后人工看长度和结构。

---

## 指标

与 `screenspot_grid/bench/metrics.py` 一致：

| 指标 | 定义 |
|---|---|
| **hit** | 最终落点落在 `bbox` 内 |
| **size bucket** | 真值框长边 `<40px` / `40–120px` / `>120px` |
| **cover macro** | 预测框**完整包住**真值框的比例 |
| **cover micro** | `min(1, ∩ / 真值面积)` 的平均 |
| **rounds_med** | locate 轮次中位数（耗时代理） |

输出示例：

```
ALL              n=3    hit=  2/3   =  66.7%  cover(macro/micro)=33%/59.7%  rounds_med=1
小(<40px)         n=0
中(40-120px)      n=3    hit=  2/3   =  66.7%  cover(macro/micro)=33%/59.7%  rounds_med=1
大(>120px)        n=0
```

---

## `agent` 模式的状态

**已接通。** 三处可见性放宽已完成：

1. `resolve_tools`（`agents/mod.rs`）→ `pub`
2. `build_cur_screen_preamble` / `assemble_cur_screen_payload`（`extension_hooks/screen_inject.rs`）→ `pub`
3. `e2e_harness` 模块：`production_system_prompt()` / `production_tools()` / `agent_hint()`

**图像注入走生产路径，不手工构造 `ChatMessage`：**

```rust
computer_state.store_pipeline_after_capture(conv_id, cap);   // 塞固定图
// screen_inject hook 随后 fresh_pipeline_capture() 命中缓存，不截屏
// → preamble / slot label / base64 全部由生产函数产出
```

**主 LLM 调用也走生产路径：** `hint_via_production_wire()` 用
`build_stream_chat_wire()` + `stream_chat_wired()`，和主循环同一套 wire 构造与流式解析。
`chat_once()` 也走流式，但它把 system 当单个字符串传、绕过 wire 构造，**曾经因此
让截图被静默剥离**（见下节）。

---

## 踩过的坑：`supportsVision` 缺失会静默盲猜

**e2e 早期 hint 命中率只有 1.9%，模型给出的框是整屏 `[0,0,1000,1000]`。**

**根因：`model_capability_flags` 的 `supports_vision` 默认 `false`。e2e 的 provider 没配
`modelConfigs`，于是 `make_openai_messages` 把截图替换成一行文字：**

```
[Current screen] (image not inlined: primary model does not support vision)
```

**模型收到的是一句「这里本来有张图」，然后靠文字描述猜坐标——不报错、不告警、返回合法的 bbox。**

**修法：provider 加 `modelConfigs: { "<model>": { "supportsVision": true } }`。**
**效果：hit 1.9% → 21.9%，`reasoning_len` 从 0 → 2475，`prompt_tokens` 从 8294 → 10321。**

**产品级风险：任何 provider 配置漏了 `supportsVision`，视觉任务都会静默降级为盲猜。**
**值得加一条 warn：视觉 agent 在 `supportsVision=false` 时不应静默继续。**

---

## 已知局限

| 局限 | 影响 |
|---|---|
| 真值来自 verify 而非人工 | verify 假阳性会污染 hit 率 |
| 单会话、单应用（数独 + 微信） | 不代表通用 GUI |
| **无消息历史（默认）** | 生产有 680 条上下文，hint 实验的数字是「冷启动」条件下的 |
| 图片无指针 | 与生产 `screenshot_jpeg` 有细微差异 |
| **system prompt 只有 1 个工具** | 生产是 31 个（约 46K 字符），本 benchmark 是 28.6K |
| 105 例 | 单例权重 0.95%，波动 ±2–3 个点时不宜下结论 |

**关于「无历史」这一条，有一个实测数据点：** 从生产 dump 注入 40 / 100 / 300 条历史后，
单例（`0069815c`）的偏差分别是 448 / 225 / 341 px，而冷启动是 677 px。**历史确实帮助，
但注入的是**另一段会话**的历史，目标描述容易串味，所以默认不开。**

---

## hint 精度实验（2026-10-29）

**目标：** 主 LLM 从 (截图, goal) 产出的 bbox 有多准，以及 prompt 的哪种写法最准。

**方法：** `POINTER_E2E_MODE=hint` 全量 105 例，8 并发。每次跑约 70–90 秒。
**每档跑两次**——单次波动 ±2–3.7 点，单跑对单跑不能下结论。

### 五档对比

| 方案 | prompt 关键内容 | hit | 中位偏差 | 均值 | <20px | >200px |
|---|---|---|---|---|---|---|
| **A. 无尺寸** | 只说「0-1000 归一化」 | 30.5% | 84 px | 100 px | 31 | — |
| **B. 只说尺寸** | 加「图是 1920×1080」 | 36.7% | 100 px | 125 px | 38 | — |
| **C. 归一化 + 换算说明** | 加尺寸 + 除法公式 + 反例 | 56.2% | 11 px | 97 px | 59 | 25 |
| **D. 纯像素** | 只说「bbox 是像素，别归一化」 | 59.5% | 15 px | 37 px | 61 | 2 |
| **E. 归一化思考 + 像素输出**（最小 prompt） | 先在 0-1000 网格定位，再转像素输出 | **80.0%** | **7 px** | 28 px | 85 | 4 |
| **E. 同上（完整 29K prompt）** | 同上，但 `AGENT.md` 同步改过 | **73.3%** | **7 px** | 33 px | 79 | 2 |

**E 档（最小 prompt）两次运行完全相同：80.0% / 80.0%，中位 7 px / 7 px。**
**分档：小(<40px) 85.7%、中(40-120px) 78.2%、大(>120px) 100%。**

**E 档（完整 prompt）分档：小 42.9%、中 79.3%、大 50.0%。中位 7 px 与最小 prompt 持平，
差距集中在尾部——29K 无关内容（另 30 个工具的文档）仍是干扰。**

### 结论一：模型需要一个**相对坐标系**来思考，但**像素**来输出

**这条结论是整组实验最有价值的发现，而且不是一次就找到的。**

**推理链：**

1. **A 档（30.5%）**：只有归一化契约，模型不知道图有多宽，只能猜分母
2. **B 档（36.7%）**：给了尺寸，模型开始算像素然后**忘记除**，反而更差
3. **C 档（56.2%）**：把除法写成明确步骤，中位偏差降到 11 px，但仍有 25 例忘记除
4. **D 档（59.5%）**：干脆只要求像素，消除除法——但**掉了 15 点**
5. **E 档（80.0%）**：**先在 0-1000 网格上定位，再转成像素输出**

**D 档为什么退步？** 因为「纯像素」把模型唯一可用的**相对参照系**也拿掉了。数独盘面上「第 C 行第 5 列」这种描述天然是相对坐标——模型用「左三分之一」判断位置比用「像素 640」判断更可靠。

**E 档的写法：**

```
The screenshot is 1920×1080 pixels.
Decide the position on a 0-1000 normalized grid first: (0,0) is the top-left
corner, (1000,1000) the bottom-right.
Then convert for `bbox`: x_pixel = x_norm / 1000 * 1920, y_pixel = y_norm / 1000 * 1080.
`bbox` is [x1, y1, x2, y2] in raw pixels of this screenshot.
```

**思考用相对坐标（模型擅长），输出用绝对像素（不需要算术）。两个动作各取所长。**

### 结论二：`C` 档的长尾比中位数更能说明问题

| 方案 | 中位 | 均值 | 均值/中位 |
|---|---|---|---|
| A | 84 | 100 | 1.2× |
| C | 11 | 97 | **8.8×** |
| D | 15 | 37 | 2.5× |
| **E** | **7** | **28** | **4.0×** |

**C 档的中位 11 px 已经优于生产（18 px），但均值 97 px 说明分布是双峰的：算对除法的那批非常准，忘掉的那批全错。**

**生产链路是「hint 框 → 工具在框内 re-ground」，框偏到别的元素上时 re-ground 救不回来。所以长尾是致命的，中位数好看没用。**

**E 档把 >200px 压到 4 例（A/B 档未统计，C 档 25 例，D 档 2 例）。**

### 结论三：框尺寸不是瓶颈

| 方案 | 框尺寸（中位） | 相对真值 |
|---|---|---|
| A | 53×51 px | 1.33× |
| C | 53×46 px | 1.33× |
| 真值 | 40×43 px | 1.0× |

**各档的框大小都接近真值（1.3–1.5 倍），差别全在「框在哪」。**

### 对产品的含义（已改 prompt，未改工具层）

**生产 `AGENT.md` 原来写的是：**

> `bbox` is 0–1000 normalized … **Never convert to screen pixels yourself** — the tool does that, and your pixel arithmetic is the least reliable step in the chain.

**已改成两步契约（`AGENT.md` 是唯一权威）：**

```markdown
`bbox` is [x1, y1, x2, y2] in pixels of the screenshot you were given.
The `[CUR_SCREEN]` preamble states that screenshot's exact width and height.

1. Locate on a normalized grid (1000×1000). Relative position is what you see.
2. Convert to pixels for `bbox`: x_pixel = x_norm / 1000 × width.
```

**其余两处只做简要描述 + 引用：**

| 文件 | 改动 |
|---|---|
| `AGENT.md` → `## Locating targets` | **唯一权威**：两步契约全文 |
| `prompts/mouse.md` | 「`[x1,y1,x2,y2]` in **pixels**（see **Locating targets**）」 |
| `tools/prompts/mouse.schema.yaml` | 9 处参数描述改成 pixels，不再写归一化 |
| `extension_hooks/screen_inject.rs` | `build_cur_screen_preamble(cap)` 从 `cap.monitor` 取尺寸，运行时告知 |

**图像尺寸走 inject 而不是写进 prompt**：它是运行时数据，换显示器就会变；写死在 prompt 里必然过期。

**工具层换算（`locate/engine.rs:730` 的 `cx / 1000.0 * w`）尚未改。** 生产上模型现在会按
`AGENT.md` 输出像素，而工具仍按归一化换算——**这条链现在是断的，不能直接上线**。

### 复现

**两套最小 prompt 由 `POINTER_E2E_BBOX_UNIT` 切换，其他条件完全相同。**

```bash
# A. 归一化，不声明尺寸（基线）
POINTER_E2E_MINIMAL_PROMPT=1 POINTER_E2E_BBOX_UNIT=norm \
POINTER_E2E_MODE=hint POINTER_E2E_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test computer_e2e -- --ignored --nocapture

# C. 归一化 + 声明尺寸 + 换算说明
POINTER_E2E_MINIMAL_PROMPT=1 POINTER_E2E_BBOX_UNIT=norm POINTER_E2E_IMAGE_SIZE=1 \
POINTER_E2E_MODE=hint POINTER_E2E_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test computer_e2e -- --ignored --nocapture

# D. 原始像素（尺寸自动声明，无需 IMAGE_SIZE）
POINTER_E2E_MINIMAL_PROMPT=1 POINTER_E2E_BBOX_UNIT=pixel \
POINTER_E2E_MODE=hint POINTER_E2E_CONCURRENCY=8 \
  cargo test --release -p pointer-core --test computer_e2e -- --ignored --nocapture
```

**B 档（只说尺寸、不说算法）是 `IMAGE_SIZE` 的早期实现。当前代码里
`IMAGE_SIZE` 走的是 C 档文案，B 档只能靠历史日志复现。**

**每次运行都会在 stderr 打出当前档位：**

```
[harness] bbox_unit=pixel text_len=67 reasoning_len=656 tool_calls=1 prompt_tokens=Some(3047)
```

### 两套最小 prompt 全文

**`BBOX_UNIT=norm`（A / C 档）：**

```
You control a macOS desktop through screenshots.
Given the screenshot and a goal, find the element the goal names and report the box it occupies.

Coordinates are 0-1000 normalized over the whole screen:
(0,0) is the top-left corner, (1000,1000) the bottom-right.
x scales with image width, y with image height.

Call `mouse_click_at` with `target_desc` and `bbox`.
```

**`BBOX_UNIT=pixel`（E 档）：**

```
You control a macOS desktop through screenshots.
Given the screenshot and a goal, find the element the goal names and report the box it occupies.

Locate it on a 0-1000 normalized grid first: (0,0) is the top-left corner, (1000,1000) the bottom-right, x scales with image width, y with image height.
Then report `bbox` in raw pixels of the screenshot: [x1, y1, x2, y2].

Call `mouse_click_at` with `target_desc` and `bbox`.
```

**`[CUR_SCREEN]` preamble 追加段（`BBOX_UNIT=pixel` 时）：**

```
The screenshot is 1920×1080 pixels.
Decide the position on a 0-1000 normalized grid first: (0,0) is the top-left corner, (1000,1000) the bottom-right.
Then convert for `bbox`: x_pixel = x_norm / 1000 * 1920, y_pixel = y_norm / 1000 * 1080.
`bbox` is [x1, y1, x2, y2] in raw pixels of this screenshot.
```

**两档的差别不只是「输出单位」——`pixel` 档明确要求「先归一化定位，再转像素」，这是 80% vs 59.5% 的关键。**

### 模型对比（2026-10-29，各 3 次）

**同一套 E 档 prompt、同一批 105 例、8 并发，只换模型。**

| 模型 | provider | hit（3 次） | 均值 | 中位偏差 | <20px | >200px |
|---|---|---|---|---|---|---|
| **kimi-k3.1-pro-highspeed** | Moonshot | 84.8 / 88.6 / 87.6 | **87.0%** | **5 px** | 92 | 1.0 |
| **qwen3.8-flash** | DashScope | 73.3 / 81.9 / 81.9 | **79.0%** | **8 px** | 83 | 1.3 |
| **qwen3.8-flash**（提示词重构后） | DashScope | 82.2（107 例，1 次） | — | — | — | — |
| deepseek-flash | DeepSeek | 44.8 / 44.8 / 36.2 | **41.9%** | **25 px** | 40 | 5.3 |
| qwen3.8-27b | DashScope | 22.9 / 21.0 / 26.7 | **23.5%** | **99 px** | 20 | 4.7 |
| qwen3.7-flash | DashScope | 0.0 / 2.9 / 1.9 | **1.6%** | **137 px** | 1 | 4.3 |

**单次波动：kimi 3.8 点，qwen3.8-flash 8.6 点，deepseek-flash 8.6 点，qwen3.8-27b 5.7 点，qwen3.7-flash 2.9 点。**

**五个模型的服务端都确认了模型名（harness 打印 `[model] served=…`），不是路由错配。**

### 提示词重构后的回归（qwen3.8-flash）

**2026-10-29 把 `AGENT.md` 从 165 行压到 36 行（只留身份 + 5 条铁律）、把 prompt 顺序改成「身份 → 通用规则 → 专属细节 → 输出格式」、重写了 `bbox` 规则。下面这轮验证定位没有退化。**

| 档 | n | hit | cover macro | cover micro |
|---|---|---|---|---|
| **ALL** | 107 | **82.2%**（88/107） | 11% | 71.3% |
| 小（<40px） | 14 | 71.4%（10/14） | 14% | 64.1% |
| 中（40–120px） | 89 | 83.1%（74/89） | 11% | 71.9% |
| 大（>120px） | 4 | 100%（4/4） | 0% | 84.4% |

**sample 档（21 例）先跑过三次：81.0 / 81.0 / 85.7，均值 82.6%。**

**和上面 79.0% 不能直接比：** 数据集从 105 例变成 107 例（`e2e_truth.json` 在今天 05:19 之后加了 2 例），而且这次只跑 1 次。82.2% 落在历史三次的波动区间（73.3–81.9）内且偏上，**没有下降迹象**。

**小目标仍是弱项（71.4%，比中档低 12 点）。** 失败的 4 例都是「数格子数错」——目标 C3 框到 B5、目标 C5 框到 C8，不是单位换算错，是行列号读错。40px 的目标在 1000×1000 网格上只占 4 个单位，量化误差本身就够推到相邻元素。

### 跑 kimi 的三个前置条件（踩过的坑）

1. **key 必须 51 字符**（`sk-` + 48）。多 5 个字符就是 401 `Invalid Authentication`，
   而且 `/v1/models` 也是 401 —— 和模型名无关，纯认证失败。
2. **`temperature` 只能是 1.0**。harness 默认 `0.0`，会得到
   `400 invalid temperature: only 1 is allowed for this model`。
   用 `POINTER_E2E_TEMPERATURE=1.0` 覆盖。
3. **组织 RPM 上限 100**。8 并发会在 30 秒内打满配额，之后所有请求 429。
   `POINTER_E2E_CONCURRENCY=1` 跑一轮约 8 分钟，全程无 429。

### kimi 的 reasoning 风格和其它模型完全不同

**qwen3.8-flash 会写完整推理链（"Column 5 label is at x≈245. Row C y≈341…"），
kimi 的 reasoning 是极简的电报体：**

> I need infer click C5 cell. Should calculate screen coords 1920x1080. Sudoku grid
> left x88-458, columns width41.1. C5 is row C third y342? Rows: A262 B302 C342 etc.
> C5 x275? Grid columns 1 x109,2 150,3 191,4 233,5 274,6 316. Click blank C5 x274 y342.

**但结果更准（5 px vs 8 px）。这说明 reasoning 长度和定位质量无关——它在内部把网格
坐标表直接列出来，然后查表，而不是描述性推理。**

### 参数量不等于定位能力：27B 只有 flash 的三分之一

**`qwen3.8-27b` 的中位偏差 99 px，是 `qwen3.8-flash`（8 px）的 12 倍；hit 23.5% vs 79.0%。**

**看它的 reasoning（`0069815c`，真值 283,343）：**

> 网格大约从 x=80 到 x=460，9 列，每列约 42 px
> 列 5 大约在 x=80 + 4*42 + 21 = 269
> 列 1 中心 ~100，列 2 中心 ~140，列 3 ~180，列 4 ~220，列 5 …

**它确实在做正确的算术（列宽 42 px、列 5 中心 269），但前提「网格从 x=80 到 x=460」错了——实际网格宽约 380 px，列宽约 40 px，列 5 中心在 224 附近。**

**也就是说：它的推理链是对的，读图的像素估计偏了。这和 qwen3.7-flash「只看到 5 个按键」是同类问题——视觉分辨率/定位精度不足，不是坐标契约问题。**

**结论：这个任务对模型的要求是「能准确读出屏幕像素位置」，不是「参数多」。**

### qwen3.7-flash 为什么几乎全错：看错了画面

**`016d6f5d`（真值 y=727）的 reasoning：**

> 用户要求我点击数独游戏底部数字键盘中的按键4。…
> 整个数独窗口大约在 x=30-390, y=65-600
> 数字键盘行大约在 y=580-650

**实际数独窗口延伸到 y≈780，数字键盘在 y≈700-760——它把窗口高度低估约 180 px。**

**`07210a65` 的 reasoning：**

> 底部数字键盘从左到右是：2(计数1), 3(计数2), 6(计数1), 8(计数2), 9(计数1)

**实际是完整的 1-9 九个键，它只看到 5 个。**

**这不是坐标契约的问题**——它确实按像素输出了（反算 `norm` 得像素 `[184,585,225,650]`），是**视觉分辨率/定位能力不足**。

### deepseek-flash 差在哪：位置对了但精度不够

**中位 25 px、`<50px` 有 74/105——大部分样本方向正确，但达不到 20 px 以内。** 和 qwen3.8-flash 的 8 px 差一个档次。

**结论：模型选择比 prompt 调整影响更大（79% vs 42% vs 2%），但 prompt 调整能把好模型从 30% 推到 79%。**

### 这一节还没做的事

| 项 | 状态 |
|---|---|
| E 档 + 完整生产 prompt（非最小 prompt） | ✅ 已测：73.3 / 81.9 / 81.9，均值 **79.0%**，中位 8 px——与最小 prompt 无实质差异 |
| E 档 + 680 条历史 | **未测**——历史注入开关存在（`POINTER_E2E_HISTORY_DUMP`），但注入的历史来自另一段会话，会引入噪声 |
| 剩余 1–2 例 >200px 的失败原因 | 未分析 |
| `agent` 全链路（hint → locate → verify）在 E 契约下的表现 | **未测**——工具层换算仍是归一化，链路现在是断的 |
| 图片是否被 API 侧缩放 | **未验证**——`prompt_tokens≈10500` 反推接近原图，但没有直接确认 |
| 其他模型（deepseek-flash / qwen3.7-flash） | ✅ 已测，见上节 |

**注意：E 档的数字是「最小 prompt + 无历史」条件下的，不能直接和生产 18 px 对比——生产有 680 条历史。但 E 档 7 px 的中位已经优于生产，这个差距值得进一步查。**


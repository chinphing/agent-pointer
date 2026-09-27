# Computer Use Agent 实现计划

> 基于视觉大模型的计算机操作智能体（Vision-Based Computer Use Agent）
>
> 目标：将既有 Computer Use Agent 的 Python 实现迁移至 `pointer-app` Rust 框架
>
> **核心设计：两套定位体系**
> 1. **基于标注序号（Index-based）**：模型使用标注覆盖图上的整数索引（1, 2, 3...），工具内部通过 `index_map` 解析为屏幕像素坐标。优先推荐，精度高。
> 2. **基于规范化坐标（Coordinate-based）**：模型使用归一化坐标（如 qwen 0-1000），工具内部通过坐标转换解析为屏幕像素坐标。用于无标注索引的目标。
>
> 坐标系统默认：`qwen`（0-1000 归一化）
> 标注服务：外部 HTTP 服务（`COMPUTER_ANNOTATE_API_BASE`，默认 `http://127.0.0.1:8000`）
> 动作底层库：`enigo`（跨平台封装，未来可扩展）
> **桌面工具 → 下一轮截图复核前间隔**：`timing.rs` 中 `POST_DESKTOP_ACTION_DELAY_MS`（未传 **`wait`** 时默认 1000ms）。**`mouse` / `hotkey` / `input` / `modified_click`** 的 **`tool_args`** 可传可选 **`wait`**（秒，运行时钳位 **1–5**）；规范见各工具 `prompts/*.md` 与 **`crates/pointer-core/src/agents/computer/prompts/tiers/primary/communication.md`**；各工具 `prompts/*.md` 仅保留一句引用与启发式。
> **组合操作子步骤间隔**（如定位后输入、全选后输入、定位后滚动）：`timing.rs` 的 `COMPOSITE_ACTION_STEP_GAP_MS`（默认 50ms）
> 排除范围：shell 执行、文件操作（已有现有工具覆盖）

---

## 目录

1. [现状分析](#1-现状分析)
2. [总体架构](#2-总体架构)
3. [第一期：基础视觉-行动-验证循环](#3-第一期基础视觉-行动-验证循环)
4. [第二期：验证阶段思维链](#4-第二期验证阶段思维链)
5. [第三期：高级功能与完善](#5-第三期高级功能与完善)
6. [模块详细设计](#6-模块详细设计)
7. [数据流与时序图](#7-数据流与时序图)
8. [测试策略](#8-测试策略)
9. [风险与依赖](#9-风险与依赖)
10. [附录：参考代码映射表](#10-附录参考代码映射表)

---

## 1. 现状分析

### 1.1 pointer-app 框架现状

`pointer-app` 是一个基于 Rust + Tauri + Vue 的 AI 助手应用，核心架构如下：

```
crates/pointer-core/src/
├── lib.rs              # 模块导出
├── chat_service/     # 对话服务核心：消息循环、工具调用、历史管理（`session.rs`、`session_inner.rs` 等）
├── models.rs           # 数据模型：ChatMessage, ToolCall, Conversation, AgentProfile
├── agents.rs           # Agent 定义与注册：AgentDef, AgentRegistry, AgentProfile
├── provider.rs         # LLM 提供商抽象：OpenAIProvider, ProviderEvent
├── tools/              # 工具系统
│   ├── mod.rs          # ToolRegistry, ToolDef, ToolHandler, parse_tool_call_arguments
│   ├── builtin.rs      # 注册内置工具
│   ├── general.rs      # 通用工具
│   ├── terminal.rs     # 终端命令执行
│   ├── math.rs         # 数学计算
│   ├── text.rs         # 文本处理
│   ├── file.rs         # 工作区 file 工具（prompts/file.md）
│   └── skill.rs        # skill 工具（prompts/skill.md）
├── skills/             # 技能系统
│   ├── mod.rs          # SkillRegistry, SkillDef
│   ├── builtin.rs      # 内置技能注册
│   └── external.rs     # 外部技能加载
├── storage.rs          # 持久化存储
└── context_compression.rs  # 上下文压缩
```

关键特性：
- **Agent 系统**：`AgentProfile` 枚举区分角色（General, Coder, Supervisor 等），支持系统 prompt 和工具白名单
- **工具系统**：`ToolRegistry` 注册 `ToolDef` + `ToolHandler`，支持 JSON 参数解析和代码围栏剥离
- **对话循环**：`ChatService` 处理消息流、工具调用、历史管理、预算控制
- **多模态**：`ChatMessage` 支持文本 + 工具调用，图片支持需确认/扩展

### 1.2 参考代码（Python）核心能力

既有 Computer Use Agent（Python）是一个成熟实现：

```
agents/computer/
├── screen.py           # 屏幕捕获：mss + pyautogui，截图当前显示器
├── som_util.py         # UI 标注客户端：HTTP POST /api/v1/annotate/all
├── actions.py          # 底层动作：pyautogui/pynput 点击、输入、滚动
├── coord_convert.py    # 坐标转换：qwen/kimi/pixel 归一化 ↔ 像素
├── mouse_move.py       # 鼠标移动：人性化轨迹
├── mouse_path.py       # 贝塞尔曲线路径生成
├── screen_overlay.py   # 屏幕覆盖图绘制：指针、焦点、大区域框
├── scroll_heatmap.py   # 滚动热图
├── focus_position.py   # 焦点位置检测
├── storage_paths.py    # 存储路径解析
├── task_data_memory.py # 任务数据内存管理
├── credential_store.py # 凭据存储
├── os_prompts.py       # OS 特定 prompt 加载
├── agent.json          # Agent 元数据
├── actions.py          # ActionTools：底层操作封装
├── extensions/
│   └── message_loop_prompts_after/
│       └── _10_computer_screen_inject.py  # 屏幕注入扩展：每轮截图→标注→注入消息
├── prompts/            # 系统 Prompt 文件
│   ├── agent.system.main.role.md
│   ├── agent.system.main.communication.md
│   ├── agent.system.main.computer_usage.md
│   ├── agent.system.os.macos.md
│   ├── agent.system.os.windows.md
│   ├── agent.system.os.linux.md
│   └── agent.system.tool.*.md
└── tools/              # 视觉工具
    ├── vision_common.py    # 共享：index_map、坐标解析、滚动限制、after_execution 钩子
    ├── mouse.py            # 鼠标工具：click_index、click_at、scroll 等
    ├── hotkey.py           # 热键工具
    ├── modified_click.py   # 修饰键点击
    ├── composite_action.py # 复合动作：type_text_at_index、scroll_at_index 等
    ├── wait.py             # 等待工具
    ├── screen_reader.py    # 屏幕阅读/提取
    ├── account_login.py    # 自动登录
    ├── clipboard.py        # 剪贴板操作
    └── checkpoint.py       # 检查点
```

核心循环：
```
User message → screen inject (capture + annotate + zoom)
            → build multimodal messages → LLM
            → model returns tool_name + tool_args
            → resolve_index / get_coord_pos → execute action
            → tool result with verify hint → next loop
```

---

## 2. 总体架构

### 2.1 目标架构

在 `pointer-app` 框架中新增 `computer` 模块，保持与现有 `tools`、`agents`、`chat_service` 的松耦合集成：

```
crates/pointer-core/src/
├── lib.rs
├── chat_service/                 # 屏幕注入扩展点调用（`single_agent.rs` / `agent_stream_round.rs` / `sub_agent_stream.rs` 等）
├── models.rs                       # AgentProfile::Computer, 多模态消息等
├── extensions/                     # 通用扩展注册表（trait + ExtensionRegistry）
├── agents/
│   ├── mod.rs                      # Agent 注册表 + `pub mod computer`
│   └── computer/                   # Computer Agent：清单 + 基础能力 + tools 子目录
│       ├── AGENT.md
│       ├── prompts/tiers/{primary,intermediate,advanced}/
│       ├── prompts/os/
│       ├── author/                 # not loaded at runtime
│       ├── mod.rs                  # re-exports
│       ├── state/                  # ComputerState, capture pipeline
│       ├── input/                  # actions, enigo, mouse_move, timing
│       ├── vision/                 # screen, annotate, coord, overlays, vision_state
│       ├── tier/
│       ├── extension_hooks/
│       ├── verify.rs
│       └── tools/
│           ├── mod.rs              # register_all
│           ├── args_util.rs
│           ├── tool_*.rs
│           ├── prompts/
│           └── schemas/
├── tools/
│   ├── mod.rs
│   └── builtin.rs                  # register_computer_tools → agents::computer::tools::register_all
└── storage.rs
```

### 2.2 设计原则

1. **单一职责**：每个模块只负责一个明确职责，函数不超过 40 行
2. **跨平台抽象**：`actions.rs` 定义 trait，底层实现可替换（当前 `enigo`，未来可扩展）
3. **错误处理**：定义明确异常类，不吞异常，错误信息包含上下文
4. **可测试性**：关键逻辑可单元测试，外部依赖（HTTP、屏幕、输入）可 mock
5. **类型安全**：完整类型注解（Rust 类型系统），禁止 `Any` 等价物（如过度使用 `dyn`）
6. **常量提取**：所有魔法数字、配置项提取为常量或配置
7. **日志规范**：使用 `log` crate，禁止 `println!`

---

## 3. 第一期：基础视觉-行动-验证循环

### 3.1 目标

实现最核心的截图 → 标注 → LLM 决策 → 执行 → 验证闭环。使 Computer Agent 能够：
1. 每轮对话前自动捕获屏幕并标注 UI 元素
2. 模型通过索引或坐标调用鼠标/热键/复合动作工具
3. 工具执行后返回验证提示，驱动下一轮循环

### 3.2 任务清单

#### 3.2.1 屏幕捕获模块 (`agents/computer/screen.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `MonitorInfo` struct | 显示器信息：left, top, width, height | 高 |
| `screenshot_current_monitor()` | 捕获鼠标所在显示器的截图 | 高 |
| `encode_image_to_base64()` | PNG → base64 编码 | 高 |
| `MonitorSelector` | 根据坐标选择对应显示器 | 中 |
| 跨平台测试 | macOS/Windows/Linux 截图一致性 | 中 |

**技术方案**：
- macOS：`core-graphics` / `screenshot` crate / `screencapture` 命令
- Windows：`windows` crate (GDI) / `screenshot` crate
- Linux：`x11` / `wayland` / `screenshot` crate
- 优先使用 `screenshot` crate（跨平台封装），不足时补充平台特定代码

#### 3.2.2 UI 标注模块 (`agents/computer/annotate.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `AnnotateClient` struct | HTTP 客户端，配置 base_url、timeout | 高 |
| `annotate_image()` | 调用 `POST /api/v1/annotate/all`，返回标注图 + boxes | 高 |
| `BoxInfo` struct | 标注框信息：index, x, y, width, height, confidence | 高 |
| `IndexMap` type | `HashMap<u32, BoxInfo>`，索引到屏幕坐标的映射 | 高 |
| 错误处理 | `AnnotateError`：网络错误、维度不匹配、服务错误 | 高 |
| 异步支持 | 使用 `reqwest` + `tokio`，标注调用不阻塞事件循环 | 中 |

**接口契约**（与现有标注服务兼容）：
```rust
pub struct AnnotateRequest {
    pub image: Vec<u8>,        // PNG bytes
    pub threshold: f32,        // default 0.1
    pub overlap_threshold: f32, // default 0.1
    pub padding: i32,          // default 3
}

pub struct AnnotateResponse {
    pub image: Vec<u8>,        // Annotated PNG
    pub boxes: Vec<BoxInfo>,   // Detected boxes
}
```

#### 3.2.3 坐标转换模块 (`agents/computer/coord.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `CoordinateSystem` enum | `Qwen`, `Kimi`, `Pixel` | 高 |
| `normalized_to_screen()` | 归一化坐标 → 屏幕像素 | 高 |
| `screen_to_normalized()` | 屏幕像素 → 归一化坐标 | 中 |
| `CoordConverter` struct | 封装 bbox + system 的转换器 | 高 |

**坐标系统定义**：
- `Qwen`：模型输出 0-1000，映射到屏幕 bbox
- `Kimi`：模型输出 0-1000（可能不同映射方式，需确认）
- `Pixel`：直接使用屏幕像素坐标

#### 3.2.4 底层动作模块 (`agents/computer/actions.rs` + `action_enigo.rs`)

**抽象层 (`actions.rs`)**：

| Trait / Struct | 说明 |
|----------------|------|
| `ActionBackend` trait | 跨平台动作后端抽象 |
| `ActionExecutor` struct | 封装执行逻辑，持有 `Box<dyn ActionBackend>` |
| `ActionResult` | 动作执行结果 |

```rust
pub trait ActionBackend: Send + Sync {
    fn click(&self) -> Result<ActionResult>;
    fn double_click(&self) -> Result<ActionResult>;
    fn right_click(&self) -> Result<ActionResult>;
    fn move_to(&self, x: i32, y: i32) -> Result<ActionResult>;
    fn scroll(&self, lines: i32) -> Result<ActionResult>;
    fn type_text(&self, text: &str) -> Result<ActionResult>;
    fn hotkey(&self, keys: &[&str]) -> Result<ActionResult>;
    fn get_position(&self) -> Result<(i32, i32)>;
}
```

**enigo 实现层 (`action_enigo.rs`)**：

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `EnigoBackend` struct | 实现 `ActionBackend` | 高 |
| 键名映射 | 统一键名（`command`/`ctrl`/`alt`/`shift`）→ enigo 键码 | 高 |
| 平台修饰键 | macOS `command` vs Windows/Linux `ctrl` | 高 |
| 粘贴输入 | 大文本使用剪贴板 + 粘贴快捷键 | 中 |
| 人性化延迟 | 操作间添加随机延迟模拟人类 | 低 |

**鼠标移动（`mouse_move.rs` + `EnigoBackend::move_to`）** — 详见 [`computer-mouse-movement-roadmap.md`](computer-mouse-movement-roadmap.md)：

- 路径与时间分开规划；默认直线 **14px** 步进，**最后一段**再按 **5px** 加密，便于目标处触发 hover。
- 总移动时长 **0.5s**，**ease-out** 分配到每个路点（先快后慢，靠近目标更慢）。
- 点击类动作：`move_to` 后 **100ms** settle（`SETTLE_AFTER_ABSOLUTE_MOVE_MS`），再点击；`hover_*` 仅移动、无 settle。
- 工具参数 `human_like` 尚未接入 Rust；常量集中在 `timing.rs`。

#### 3.2.5 视觉状态管理 (`agents/computer/vision_state.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `VisionState` struct | 每轮视觉状态：index_map、screen_info、coordinate_system | 高 |
| `RecentAction` struct | 最近动作记录：tool、method、args、timestamp | 高 |
| `ActionHistory` | 循环队列存储最近 N 个动作 | 中 |
| 状态生命周期 | 每轮注入前更新，工具执行后追加 | 高 |

#### 3.2.6 视觉工具实现

**两套定位体系**

所有视觉动作工具都支持两套互斥的定位方式：

1. **基于标注序号（Index-based）**：模型使用标注覆盖图上显示的整数索引（1, 2, 3...）。工具内部通过 `vision_state.index_map` 将序号解析为屏幕像素坐标。这是最优先推荐的方式，精度高且不受坐标系统影响。
2. **基于规范化坐标（Coordinate-based）**：模型使用归一化坐标（如 qwen 系统的 0-1000）。工具内部通过 `coord::normalized_to_screen()` 转换为像素坐标。用于没有标注索引的目标（如空白区域、自定义位置）。

**mouse 工具 (`tool_mouse.rs`)**：

| Method | 定位方式 | 说明 |
|--------|----------|------|
| `click_index` | 序号 | 点击标注索引对应的元素中心 |
| `double_click_index` | 序号 | 双击标注索引对应的元素中心 |
| `right_click_index` | 序号 | 右键点击标注索引对应的元素中心 |
| `click_at` | 坐标 | 点击指定归一化/像素坐标 |
| `double_click_at` | 坐标 | 双击指定归一化/像素坐标 |
| `right_click_at` | 坐标 | 右键指定归一化/像素坐标 |
| `click_current` | 无 | 在当前鼠标位置点击 |
| `move_offset` | 偏移 | 相对当前位置移动 dx, dy（像素） |
| `scroll_at_current` | 无 | 在当前鼠标位置滚动 |
| `hover_index` | 序号 | 悬停在标注索引对应的元素中心 |

**hotkey 工具 (`tool_hotkey.rs`)**：

| Method | 定位方式 | 说明 |
|--------|----------|------|
| `hotkey` | 无 | 执行热键组合（如 `command+c`），不涉及定位 |

**composite_action 工具 (`tool_composite.rs`)**：

| Method | 定位方式 | 说明 |
|--------|----------|------|
| `type_text_at_index` | 序号 | 移动到索引位置 → 点击 → 输入文本 |
| `type_text_at` | 坐标 | 移动到坐标 → 点击 → 输入文本 |
| `type_text_at_focused` | 无 | 在当前焦点处直接输入文本 |
| `scroll_at_index` | 序号 | 移动到索引位置 → 滚动 |
| `scroll_at` | 坐标 | 移动到坐标 → 滚动 |
| `modified_click` | 序号/坐标 | 修饰键（Ctrl/Alt/Shift/Cmd）+ 点击 |

**wait 工具 (`tool_wait.rs`)**：

| Method | 说明 |
|--------|------|
| `wait` | 等待指定秒数 |

#### 3.2.7 屏幕注入集成 (`chat_service` 扩展)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 注入时机 | 在 `prepare_messages` 阶段，检测到 `profile == Computer` 时触发 | 高 |
| 注入内容 | 原始截图 + 标注覆盖图 + 可选 zoom 图 | 高 |
| 历史管理 | 保留最近 2 张原始截图用于对比，旧图降级为文本占位 | 高 |
| 消息格式 | 图片作为 base64 嵌入 `ChatMessage.content`（需扩展模型支持多模态） | 高 |
| 环境信息 | 注入单行 Environment（OS、时间、时区、语言） | 中 |

**注入流程**：
```rust
async fn inject_computer_vision(
    &self,
    messages: &mut Vec<ChatMessage>,
    agent_profile: &AgentProfile,
) -> Result<()> {
    if agent_profile != AgentProfile::Computer {
        return Ok(());
    }

    // 1. 截图
    let (screenshot, monitor) = screen::screenshot_current_monitor()?;

    // 2. 标注
    let (annotated, boxes) = annotate::annotate_image(&screenshot).await?;

    // 3. 构建 index_map
    let index_map = build_index_map(&boxes, &monitor);

    // 4. 保存状态
    self.vision_state.set_index_map(index_map);
    self.vision_state.set_coordinate_system(CoordinateSystem::Qwen);

    // 5. 注入消息
    messages.push(build_vision_message(screenshot, annotated));

    Ok(())
}
```

#### 3.2.8 Agent 定义 (`agents/computer/AGENT.md`)

创建 Computer Agent 的 manifest 文件，定义：
- 名称、描述、角色
- 可用工具列表：mouse、hotkey、composite_action、wait
- 系统 prompt 引用

#### 3.2.9 模型扩展 (`models.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `AgentProfile::Computer` | 新增变体 | 高 |
| `ChatMessage` 图片支持 | 扩展 content 支持图片 URL/base64（或新增 `images` 字段） | 高 |

### 3.3 第一期交付标准

- [ ] 屏幕捕获跨平台可用
- [ ] 标注服务 HTTP 客户端正常工作
- [ ] 坐标转换 qwen 系统正确
- [ ] enigo 底层动作可执行（点击、输入、热键、滚动）
- [ ] mouse/hotkey/composite_action/wait 工具注册并可调用
- [ ] 屏幕注入在每轮 Computer Agent 对话前触发
- [ ] 模型返回工具调用后，动作正确执行并返回验证提示
- [ ] 单元测试覆盖坐标转换、状态管理、工具参数解析
- [ ] 集成测试验证完整循环（mock 标注服务 + 假动作后端）

---

## 4. 第二期：验证阶段思维链

### 4.1 目标

增强模型在行动后的推理和验证能力，优化历史管理以支持长任务。

### 4.2 任务清单

#### 4.2.1 验证框架 (`agents/computer/verify.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `VerifyHintGenerator` | 根据工具类型生成验证提示 | 高 |
| 滚动验证 | "Runtime does not detect movement. Compare [Previous] vs [Current]..." | 高 |
| 点击验证 | "Action executed; Verify result on next screenshot." | 高 |
| 拖拽验证 | "Verify on next screenshot: expected drop target..." | 中 |
| 失败警告 | 连续失败 N 次后生成警告提示 | 中 |

#### 4.2.2 历史管理优化 (`chat_service`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 原始截图保留策略 | 保留最近 2 轮全屏截图作为图片，更早的降级为简短 `[CUR_SCREEN]` 占位文本 | 高 |
| 标注图保留策略 | 仅保留最新标注图，旧的降级为文本 | 高 |
| Zoom 图策略 | 仅保留最新 zoom 图 | 中 |
| Token 节省计算 | 估算图片 token 消耗，触发降级阈值 | 低 |

#### 4.2.3 系统 Prompt 体系 (`agents/computer/tools/prompts/`)

| 文件 | 内容 | 优先级 |
|------|------|--------|
| `role.md` | 角色定义：视觉操作、索引使用、坐标使用规则 | 高 |
| `communication.md` | 响应格式：XML（thoughts / tool_name / tool_args）、工具优先级 | 高 |
| `computer_usage.md` | 计算机使用指南：截图解读、索引映射、操作顺序 | 高 |
| `os_macos.md` | macOS 快捷键参考 | 中 |
| `os_windows.md` | Windows 快捷键参考 | 中 |
| `os_linux.md` | Linux 快捷键参考 | 中 |
| `tool_mouse.md` | mouse 工具规格 | 高 |
| `tool_hotkey.md` | hotkey 工具规格 | 高 |
| `tool_composite_action.md` | composite_action 工具规格 | 高 |
| `tool_wait.md` | wait 工具规格 | 中 |

#### 4.2.4 检查点机制 (`agents/computer/checkpoint.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `CheckpointManager` | 定期合并历史、截断上下文 | 中 |
| 触发条件 | N 轮对话后自动触发，或 screen_reader:load 时 | 中 |
| 历史合并 | 将多轮工具调用结果合并为摘要 | 中 |
| 状态保存 | 保存 plans/progress/experience 到 execution_checkpoint | 中 |

#### 4.2.5 思维链引导

在系统 prompt 中明确要求模型在每次响应的 `thoughts` 标签中：
1. 分析当前截图状态
2. 对比预期 vs 实际结果
3. 说明下一步行动理由
4. 如果验证失败，分析原因并调整策略

### 4.3 第二期交付标准

- [ ] 每种工具类型返回合适的验证提示
- [ ] 历史图片降级策略正常工作，token 使用可控
- [ ] 系统 prompt 完整，模型行为符合预期
- [ ] 检查点机制可触发并正确截断历史
- [ ] 模型在 thoughts 中展示明确推理链

---

## 5. 第三期：高级功能与完善

### 5.1 目标

补齐参考代码的剩余能力，提升鲁棒性和用户体验。

### 5.2 任务清单

#### 5.2.1 屏幕阅读 (`tool_screen_reader.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `extract` | 从当前截图提取文本片段 | 中 |
| `load` | 加载并合并已提取的片段 | 中 |
| 滚动阅读工作流 | 引导模型 scroll → extract → scroll → extract 循环 | 中 |
| OCR 集成 | 复用标注服务的 OCR 能力或集成 Tesseract | 低 |

#### 5.2.2 自动登录 (`tool_account_login.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `fill_at_indices` | 在指定索引处填充用户名/密码 | 低 |
| `fill_at_coordinates` | 在指定坐标处填充 | 低 |
| 凭据存储 | 集成系统 keychain 或加密文件存储 | 低 |
| 凭据检索 | 根据 system + user_label 查找凭据 | 低 |

#### 5.2.3 象限放大 (`agents/computer/quadrant_zoom.rs`)

| 任务 | 说明 | 优先级 |
|------|------|--------|
| `top_left`, `top_right`, `bottom_left`, `bottom_right` | 2×2 裁剪并 2× 放大 | 中 |
| 自动检测 | 根据模型提及的象限词自动裁剪 | 中 |
| 顶部/底部条放大 | 100px 全宽条带 2× 放大（工具栏/状态栏） | 中 |
| 鼠标周围放大 | 300px 区域 3× 放大 | 中 |

#### 5.2.5 鼠标移动 (`agents/computer/mouse_move.rs`)

**已实现（见 [`computer-mouse-movement-roadmap.md`](computer-mouse-movement-roadmap.md)）：** 14px 直线 + 末段 5px 加密；0.5s ease-out；路径/时间分离规划。

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 贝塞尔曲线路径 | Python `mouse_path.py`  parity | 低 |
| `human_like` 工具参数接入 | 映射到 `MouseMoveConfig` | 低 |
| 随机扰动 | 可选 jitter，带上限 | 低 |

#### 5.2.6 数据持久化 (`storage.rs` 扩展)

| 路径 | 用途 |
|------|------|
| `{workdir}/computer/snapshots/<context_id>/` | 截图保存 |
| `{workdir}/computer/screen_reader/<context_id>/` | 提取的文本片段 |
| `{workdir}/computer/checkpoint/<context_id>/` | 检查点数据 |
| `{workdir}/computer/execution_checkpoint/<context_id>/` | 执行状态 |

#### 5.2.7 配置项扩展

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `computer_annotate_api_base` | `http://127.0.0.1:8000` | 标注服务地址 |
| `computer_annotate_timeout` | `120` | 标注超时（秒） |
| `computer_vision_coordinate_system` | `qwen` | 坐标系统 |
| `computer_human_like` | `false` | 人性化鼠标移动 |
| `computer_screen_preview_auto_refresh` | `true` | 实时预览自动刷新 |
| `computer_screen_preview_interval_sec` | `5` | 预览刷新间隔 |
| `computer_task_done_reminder_after_turns` | `10` | 检查点提醒轮数 |
| `computer_recent_actions_in_prompt` | `5` | 最近动作历史数 |

#### 5.2.8 Web UI 扩展

| 功能 | 说明 | 优先级 |
|------|------|--------|
| 设置面板 | Settings → Agent → Computer 配置页 | 低 |
| 实时预览 | 右侧截图预览面板 | 低 |
| 模型输入条 | 可选显示当前注入的模型输入摘要 | 低 |

### 5.3 第三期交付标准

- [ ] 屏幕阅读工作流可用
- [ ] 象限放大帮助识别小元素
- [ ] 数据按 context 隔离持久化
- [ ] 配置项在 UI 中可编辑
- [ ] 所有功能有对应测试覆盖

---

## 6. 模块详细设计

### 6.1 屏幕捕获模块 (`screen.rs`)

```rust
use image::{DynamicImage, RgbaImage};
use std::io;

/// Monitor bounds in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorBounds {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl MonitorBounds {
    pub fn right(&self) -> i32 {
        self.left + self.width as i32 - 1
    }
    pub fn bottom(&self) -> i32 {
        self.top + self.height as i32 - 1
    }
}

/// Capture a screenshot of the monitor containing the given point.
/// If the point is outside all monitors, falls back to the primary monitor.
pub fn screenshot_monitor_at(x: i32, y: i32) -> io::Result<(DynamicImage, MonitorBounds)> {
    // Platform-specific implementation
}

/// Capture the monitor currently under the mouse cursor.
pub fn screenshot_current_monitor() -> io::Result<(DynamicImage, MonitorBounds)> {
    let (x, y) = get_mouse_position()?;
    screenshot_monitor_at(x, y)
}

/// Encode an image to base64 PNG string.
pub fn encode_image_base64(img: &DynamicImage) -> io::Result<String> {
    // PNG encoding + base64
}

/// Get current mouse position.
pub fn get_mouse_position() -> io::Result<(i32, i32)> {
    // Platform-specific
}
```

### 6.2 标注客户端模块 (`annotate.rs`)

```rust
use reqwest;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnnotateError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Dimension mismatch: expected {expected:?}, got {got:?}")]
    DimensionMismatch { expected: (u32, u32), got: (u32, u32) },
    #[error("Service error: {message}")]
    ServiceError { message: String },
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, Clone)]
pub struct BoxInfo {
    pub index: u32,
    pub x: f32,        // center x in pixels
    pub y: f32,        // center y in pixels
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
}

pub type IndexMap = std::collections::HashMap<u32, BoxInfo>;

pub struct AnnotateClient {
    client: reqwest::Client,
    base_url: String,
    timeout: Duration,
}

impl AnnotateClient {
    pub fn new(base_url: Option<String>, timeout: Option<Duration>) -> Self {
        // Default from env or constants
    }

    pub async fn annotate(
        &self,
        image: &[u8],
        threshold: f32,
        overlap_threshold: f32,
        padding: i32,
    ) -> Result<(Vec<u8>, Vec<BoxInfo>), AnnotateError> {
        // Multipart POST /api/v1/annotate/all
        // Validate returned image dimensions match input
    }
}
```

### 6.3 坐标转换模块 (`coord.rs`)

```rust
use std::fmt;

/// Coordinate system used by the vision model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoordinateSystem {
    /// Qwen-style: 0-1000 normalized coordinates.
    Qwen,
    /// Kimi-style: 0-1000 normalized coordinates (potentially different mapping).
    Kimi,
    /// Raw pixel coordinates.
    Pixel,
}

impl fmt::Display for CoordinateSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoordinateSystem::Qwen => write!(f, "qwen"),
            CoordinateSystem::Kimi => write!(f, "kimi"),
            CoordinateSystem::Pixel => write!(f, "pixel"),
        }
    }
}

impl std::str::FromStr for CoordinateSystem {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "qwen" => Ok(CoordinateSystem::Qwen),
            "kimi" => Ok(CoordinateSystem::Kimi),
            "pixel" => Ok(CoordinateSystem::Pixel),
            _ => Err(format!("Unknown coordinate system: {}", s)),
        }
    }
}

/// Screen bounding box for coordinate conversion.
#[derive(Debug, Clone, Copy)]
pub struct ScreenBbox {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// Convert normalized coordinates to screen pixels.
pub fn normalized_to_screen(
    position: (f32, f32),
    bbox: &ScreenBbox,
    system: CoordinateSystem,
) -> (i32, i32) {
    match system {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let x = (position.0 / 1000.0) * bbox.width as f32;
            let y = (position.1 / 1000.0) * bbox.height as f32;
            (bbox.left + x.round() as i32, bbox.top + y.round() as i32)
        }
        CoordinateSystem::Pixel => (position.0 as i32, position.1 as i32),
    }
}

/// Convert screen pixels to normalized coordinates.
pub fn screen_to_normalized(
    position: (i32, i32),
    bbox: &ScreenBbox,
    system: CoordinateSystem,
) -> (f32, f32) {
    match system {
        CoordinateSystem::Qwen | CoordinateSystem::Kimi => {
            let x = ((position.0 - bbox.left) as f32 / bbox.width as f32) * 1000.0;
            let y = ((position.1 - bbox.top) as f32 / bbox.height as f32) * 1000.0;
            (x.clamp(0.0, 1000.0), y.clamp(0.0, 1000.0))
        }
        CoordinateSystem::Pixel => (position.0 as f32, position.1 as f32),
    }
}
```

### 6.4 动作后端抽象 (`actions.rs`)

```rust
use anyhow::Result;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("Move out of bounds: ({x}, {y})")]
    OutOfBounds { x: i32, y: i32 },
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Stopped")]
    Stopped,
}

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub action: String,
    pub position: Option<(i32, i32)>,
    pub detail: String,
}

/// Cross-platform action backend abstraction.
pub trait ActionBackend: Send + Sync {
    /// Click at the current mouse position.
    fn click(&self) -> Result<ActionResult, ActionError>;

    /// Double-click at the current mouse position.
    fn double_click(&self) -> Result<ActionResult, ActionError>;

    /// Right-click at the current mouse position.
    fn right_click(&self) -> Result<ActionResult, ActionError>;

    /// Move mouse to absolute screen coordinates.
    fn move_to(&self, x: i32, y: i32) -> Result<ActionResult, ActionError>;

    /// Scroll at the current position.
    /// Positive lines = scroll up, negative = scroll down.
    fn scroll(&self, lines: i32) -> Result<ActionResult, ActionError>;

    /// Type text at the current position.
    fn type_text(&self, text: &str) -> Result<ActionResult, ActionError>;

    /// Execute a hotkey combination.
    fn hotkey(&self, keys: &[String]) -> Result<ActionResult, ActionError>;

    /// Get current mouse position.
    fn get_position(&self) -> Result<(i32, i32), ActionError>;

    /// Set whether this is a dry run (no actual input).
    fn set_dry_run(&mut self, dry_run: bool);
}

/// Action executor that holds a backend and provides higher-level operations.
pub struct ActionExecutor {
    backend: Box<dyn ActionBackend>,
    dry_run: bool,
}

impl ActionExecutor {
    pub fn new(backend: Box<dyn ActionBackend>) -> Self {
        Self { backend, dry_run: false }
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self.backend.set_dry_run(dry_run);
        self
    }

    /// Click at the given screen coordinates (already resolved to pixels).
    /// Used by the **coordinate-based** positioning path.
    pub fn click_at(&self, x: i32, y: i32) -> Result<ActionResult, ActionError> {
        self.backend.move_to(x, y)?;
        self.backend.click()
    }

    /// Click at an annotated element index.
    /// The caller must resolve index → (x, y) via `vision_state.resolve_index()` first.
    /// Used by the **index-based** positioning path.
    pub fn click_index(&self, x: i32, y: i32) -> Result<ActionResult, ActionError> {
        self.click_at(x, y)
    }

    /// Type text at the given screen coordinates (already resolved to pixels).
    /// Used by the **coordinate-based** positioning path.
    pub fn type_text_at(&self, x: i32, y: i32, text: &str) -> Result<ActionResult, ActionError> {
        self.backend.move_to(x, y)?;
        self.backend.click()?;
        self.backend.type_text(text)
    }

    /// Type text at an annotated element index.
    /// The caller must resolve index → (x, y) via `vision_state.resolve_index()` first.
    /// Used by the **index-based** positioning path.
    pub fn type_text_at_index(&self, x: i32, y: i32, text: &str) -> Result<ActionResult, ActionError> {
        self.type_text_at(x, y, text)
    }

    // ... more composite operations
}
```

### 6.5 视觉状态管理 (`vision_state.rs`)

```rust
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};

/// Information about a single detected UI element.
#[derive(Debug, Clone)]
pub struct ElementInfo {
    pub index: u32,
    pub center_x: i32,
    pub center_y: i32,
    pub width: f32,
    pub height: f32,
}

/// Per-turn vision state.
#[derive(Debug, Clone, Default)]
pub struct VisionState {
    /// Map from element index to screen coordinates.
    pub index_map: HashMap<u32, ElementInfo>,
    /// Current screen bounding box.
    pub screen_bbox: Option<coord::ScreenBbox>,
    /// Active coordinate system.
    pub coordinate_system: CoordinateSystem,
    /// Recent action history.
    pub recent_actions: VecDeque<RecentAction>,
    /// Maximum actions to keep in history.
    pub max_recent_actions: usize,
}

#[derive(Debug, Clone)]
pub struct RecentAction {
    pub timestamp: DateTime<Utc>,
    pub tool: String,
    pub method: String,
    pub args: serde_json::Value,
    pub result_summary: String,
}

impl VisionState {
    pub fn new(max_recent: usize) -> Self {
        Self {
            max_recent_actions: max_recent,
            ..Default::default()
        }
    }

    pub fn set_index_map(&mut self, boxes: &[annotate::BoxInfo], bbox: &coord::ScreenBbox) {
        self.index_map.clear();
        for box_info in boxes {
            self.index_map.insert(
                box_info.index,
                ElementInfo {
                    index: box_info.index,
                    center_x: box_info.x.round() as i32,
                    center_y: box_info.y.round() as i32,
                    width: box_info.width,
                    height: box_info.height,
                },
            );
        }
        self.screen_bbox = Some(*bbox);
    }

    /// Resolve an annotated element index to screen pixel coordinates.
    /// This is the **index-based** positioning path.
    pub fn resolve_index(&self, index: u32) -> Option<(i32, i32)> {
        self.index_map.get(&index).map(|e| (e.center_x, e.center_y))
    }

    /// Resolve normalized coordinates to screen pixel coordinates.
    /// This is the **coordinate-based** positioning path.
    pub fn resolve_coordinate(&self, x: f32, y: f32) -> Option<(i32, i32)> {
        let bbox = self.screen_bbox?;
        let pos = coord::normalized_to_screen((x, y), &bbox, self.coordinate_system);
        Some(pos)
    }

    pub fn record_action(&mut self, tool: &str, method: &str, args: serde_json::Value, result: &str) {
        self.recent_actions.push_back(RecentAction {
            timestamp: Utc::now(),
            tool: tool.to_string(),
            method: method.to_string(),
            args,
            result_summary: result.chars().take(200).collect(),
        });
        while self.recent_actions.len() > self.max_recent_actions {
            self.recent_actions.pop_front();
        }
    }
}
```

### 6.6 工具注册结构

```rust
// agents/computer/tools/mod.rs — ToolRegistry 条目（mouse / hotkey / composite_action / modified_click / wait / clipboard）
use crate::agents::computer::ComputerState;
use crate::tools::{ToolEntry, ToolRegistry};
// include_str!("schemas/..."), include_str!("prompts/...")

pub fn register_all(reg: &ToolRegistry, state: Arc<ComputerState>) { /* ToolEntry::new + handler */ }

// agents/computer/mod.rs — ComputerState、屏幕/标注、ActionExecutor、VisionState（不直接注册工具）
```

---

## 7. 数据流与时序图

### 7.1 单轮对话数据流

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐     ┌──────────────┐
│   User      │     │  ChatService │     │   Computer  │     │     LLM      │
│  Message    │     │              │     │   Module    │     │   Provider   │
└──────┬──────┘     └──────┬───────┘     └──────┬──────┘     └──────┬───────┘
       │                   │                    │                   │
       │ ─────────────────>│                    │                   │
       │   user message    │                    │                   │
       │                   │ ─────────────────>│                    │
       │                   │   inject_vision()  │                   │
       │                   │                    │ ──screenshot()───>│
       │                   │                    │<─(PNG bytes)─────│
       │                   │                    │ ──annotate()────>│
       │                   │                    │<─(overlay+boxes)─│
       │                   │                    │                   │
       │                   │<─(vision messages)─│                   │
       │                   │                    │                   │
       │                   │ ─────────────────────────────────────>│
       │                   │   multimodal messages (text + images) │
       │                   │                    │                   │
       │                   │<─────────────────────────────────────│
       │                   │   tool_call: mouse:click_index(3)     │
       │                   │                    │                   │
       │                   │ ─────────────────>│                    │
       │                   │   execute tool     │                   │
       │                   │                    │ ──move_to(x,y)──>│
       │                   │                    │<─success─────────│
       │                   │                    │ ──click()───────>│
       │                   │                    │<─success─────────│
       │                   │                    │                   │
       │                   │<─(tool result +   │                    │
       │                   │    verify hint)   │                    │
       │                   │                    │                   │
       │                   │ ─────────────────────────────────────>│
       │                   │   tool result message                 │
       │                   │                    │                   │
       │                   │<─────────────────────────────────────│
       │                   │   next action or response             │
       │                   │                    │                   │
```

### 7.2 屏幕注入时序

```
ChatService::prepare_messages()
    │
    ├─> detect profile == Computer
    │
    ├─> screen::screenshot_current_monitor()
    │   ├─> get_mouse_position()
    │   └─> capture monitor at (x, y)
    │
    ├─> annotate::annotate_image(screenshot)
    │   └─> HTTP POST /api/v1/annotate/all
    │
    ├─> build_index_map(boxes, monitor_bounds)
    │       ├─> index 1 → (x1, y1) screen pixels
    │       ├─> index 2 → (x2, y2) screen pixels
    │       └─> ...
    │
    ├─> vision_state.set_index_map(index_map)    ← index-based positioning
    ├─> vision_state.set_screen_bbox(monitor)     ← coordinate-based positioning
    ├─> vision_state.set_coordinate_system(Qwen)  ← coordinate system context
    │
    ├─> build_vision_messages(raw, annotated, zooms)
    │   ├─> [Screen before action] (if exists; prior unmarked + current pointer)
    │   ├─> [Zoom pointer before action] (if exists; ±50px crop, 4× from before frame)
    │   ├─> [Screen after action]
    │   ├─> [Annotated after action]
    │   ├─> [Zoom top after action]
    │   ├─> [Zoom bottom after action]
    │   └─> [Zoom pointer after action]
    │
    └─> inject into message history
```

### 7.3 工具执行时的定位解析时序

**Index-based path:**
```
LLM calls "mouse:click_index" with {"index": 3}
    │
    ├─> tool_mouse::execute()
    │   ├─> vision_state.resolve_index(3) → (x, y) pixels
    │   └─> action_executor.click_index(x, y)
    │       └─> backend.move_to(x, y) → backend.click()
    │
    └─> return verify hint
```

**Coordinate-based path:**
```
LLM calls "mouse:click_at" with {"x": 500, "y": 500}
    │
    ├─> tool_mouse::execute()
    │   ├─> vision_state.resolve_coordinate(500.0, 500.0) → (x, y) pixels
    │   └─> action_executor.click_at(x, y)
    │       └─> backend.move_to(x, y) → backend.click()
    │
    └─> return verify hint
```

---

## 8. 测试策略

### 8.1 单元测试

| 模块 | 测试内容 | 工具 |
|------|----------|------|
| `coord.rs` | 坐标转换边界值、各种 system | `cargo test` |
| `vision_state.rs` | index_map CRUD、历史队列、resolve | `cargo test` |
| `annotate.rs` | HTTP mock、错误处理、维度校验 | `mockall` + `wiremock` |
| `actions.rs` | MockBackend 验证调用顺序 | `mockall` |
| `verify.rs` | 各种工具类型的验证提示生成 | `cargo test` |
| 工具参数解析 | JSON 解析、索引解析、坐标解析 | `cargo test` |
| 两套定位路径 | index-based 和 coordinate-based 工具调用流程 | `cargo test` |

### 8.2 集成测试

| 场景 | 说明 |
|------|------|
| 完整循环 mock | Mock 标注服务 + Mock 动作后端，验证消息流 |
| 屏幕捕获 | 在 CI 中跳过（需要显示器），本地手动验证 |
| 标注服务对接 | 使用真实标注服务测试 HTTP 客户端 |
| enigo 动作 | 本地测试实际鼠标移动（注意安全） |

### 8.3 测试示例

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_coord_conversion() {
        let bbox = ScreenBbox { left: 0, top: 0, width: 1920, height: 1080 };
        let (x, y) = normalized_to_screen((500.0, 500.0), &bbox, CoordinateSystem::Qwen);
        assert_eq!(x, 960);
        assert_eq!(y, 540);
    }

    #[test]
    fn test_clamp_out_of_bounds() {
        let bbox = ScreenBbox { left: 0, top: 0, width: 1000, height: 1000 };
        let (x, y) = normalized_to_screen((1500.0, -100.0), &bbox, CoordinateSystem::Qwen);
        assert_eq!(x, 1500); // Or clamped based on implementation decision
        assert_eq!(y, -100);
    }

    #[tokio::test]
    async fn test_annotate_client_mock() {
        // Use wiremock to mock annotate service
    }

    #[test]
    fn test_index_based_resolution() {
        let mut state = VisionState::new(10);
        state.index_map.insert(1, ElementInfo { index: 1, center_x: 100, center_y: 200, width: 50.0, height: 30.0 });

        let pos = state.resolve_index(1);
        assert_eq!(pos, Some((100, 200)));

        let missing = state.resolve_index(99);
        assert_eq!(missing, None);
    }

    #[test]
    fn test_coordinate_based_resolution() {
        let mut state = VisionState::new(10);
        state.screen_bbox = Some(ScreenBbox { left: 0, top: 0, width: 1920, height: 1080 });
        state.coordinate_system = CoordinateSystem::Qwen;

        let pos = state.resolve_coordinate(500.0, 500.0);
        assert_eq!(pos, Some((960, 540)));
    }
}
```

---

## 9. 风险与依赖

### 9.1 技术风险

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Rust 跨平台截图库不成熟 | 高 | 优先使用 `screenshot` crate，必要时写平台特定代码（macOS `screencapture`，Windows GDI） |
| enigo 在 macOS 上的权限问题 | 中 | 文档说明需要 Accessibility 权限，提供权限检测和友好错误提示 |
| 多模态消息格式不确定 | 中 | 先确认 `ChatMessage` 当前图片支持方式，必要时扩展模型 |
| 标注服务延迟高 | 中 | 异步调用 + timeout 配置，支持降级（无标注模式） |
| 长任务历史爆炸 | 中 | 第二期实现检查点和历史压缩 |

### 9.2 外部依赖

| 依赖 | 用途 | 版本要求 |
|------|------|----------|
| `enigo` | 跨平台输入模拟 | 最新稳定版 |
| `reqwest` | HTTP 客户端（标注服务） | 已存在（确认） |
| `image` | 图像处理 | 最新稳定版 |
| `base64` | base64 编码 | 已存在（确认） |
| `screenshot` | 跨平台截图（可选） | 评估后决定 |
| `core-graphics` | macOS 原生 API（备选） | 仅 macOS |

### 9.3 与现有代码的兼容风险

| 集成点 | 风险 | 缓解措施 |
|--------|------|----------|
| `ChatMessage` 扩展 | 可能影响序列化 | 使用 `#[serde(default)]` 保持向后兼容 |
| `AgentProfile` 扩展 | 可能影响匹配逻辑 |  exhaustive match 检查 |
| `ToolRegistry` 注册 | 无风险 | 新增注册调用即可 |

---

## 10. 附录：参考代码映射表

| Python 参考文件 | Rust 目标文件 | 说明 |
|-----------------|---------------|------|
| `screen.py` | `agents/computer/screen.rs` | 屏幕捕获 |
| `som_util.py` | `agents/computer/annotate.rs` | UI 标注客户端 |
| `coord_convert.py` | `agents/computer/coord.rs` | 坐标转换 |
| `actions.py` | `agents/computer/actions.rs` + `action_enigo.rs` | 动作抽象 + 实现 |
| `mouse_move.py` | `agents/computer/mouse_move.rs` | 鼠标移动（路径 + 时间规划；见 `computer-mouse-movement-roadmap.md`） |
| `screen_overlay.py` | `agents/computer/screen.rs`（扩展） | 覆盖图绘制 |
| `focus_position.py` | `agents/computer/screen.rs`（扩展） | 焦点位置检测 |
| `storage_paths.py` | `storage.rs`（扩展） | 存储路径 |
| `task_data_memory.py` | `agents/computer/vision_state.rs` | 任务数据管理 |
| `credential_store.py` | `agents/computer/credential.rs`（第三期） | 凭据存储 |
| `os_prompts.py` | `agents/computer/tools/prompts/` + 加载逻辑 | OS prompt 加载 |
| `extensions/.../_10_computer_screen_inject.py` | `agents/computer/extension_hooks/screen_inject.rs` + `chat_service/single_agent_prompt.rs` / `sub_agent_prompt.rs`（`run_message_loop_prompts_after`）调用扩展点 | 屏幕注入钩子（Computer 专用） |
| `tools/vision_common.py` | `agents/computer/vision_state.rs` | 共享视觉状态 |
| `tools/mouse.py` | `agents/computer/tools/tool_mouse.rs` | mouse 工具 |
| `tools/hotkey.py` | `agents/computer/tools/tool_hotkey.rs` | hotkey 工具 |
| `tools/composite_action.py` | `agents/computer/tools/tool_composite.rs` | composite_action 工具 |
| `tools/wait.py` | `agents/computer/tools/tool_wait.rs` | wait 工具 |
| `tools/screen_reader.py` | `agents/computer/tools/tool_screen_reader.rs`（第三期） | screen_reader 工具 |
| `tools/account_login.py` | `agents/computer/tools/tool_account_login.rs`（第三期） | account_login 工具 |
| `tools/checkpoint.py` | `agents/computer/checkpoint.rs`（第二期） | checkpoint 工具 |
| `prompts/*.md` | `agents/computer/tools/prompts/*.md` | 系统 prompt 文件 |
| `agent.json` | `agents/computer/AGENT.md` | Agent 定义 |

---

## 变更日志

| 日期 | 版本 | 变更内容 |
|------|------|----------|
| 2026-05-10 | v1.0 | 初始计划文档 |

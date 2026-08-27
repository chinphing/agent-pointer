# 千问显式 Context Cache

应用在向阿里云百炼 **OpenAI 兼容** `POST …/chat/completions` 发请求时，对符合条件的千问模型打显式缓存标记（`cache_control.type = ephemeral`）。

官方文档：[千问模型的 Context Cache 功能](https://help.aliyun.com/zh/model-studio/context-cache)

组装顺序见 **[`../internals/llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md)**。

---

## 何时启用

`models::qwen_explicit_system_cache_enabled(settings)`：DashScope 兼容 `baseUrl` + 模型 id 以 `qwen` 开头。

---

## 两个标记（前缀随对话增长）

单次请求最多 4 个标记。当前打 **2** 个（无历史时只有 system 这一个）：

| 标记 | 位置 | 前缀覆盖 |
|------|------|----------|
| **1. system cacheable** | 稳定 system 第一段 text | 人设 / 工具附录 / `[Environment]` 等 |
| **2. 历史末条** | 持久化对话历史转成 API 后的**最后一条**（`content` 改成数组） | 从开头到该条，含 system + 不断增长的对话 |

**标记之后（不进这两段前缀）：** 每轮 `injected_tail`（`[CUR_SCREEN]`、任务板 user 注入等）。system **dynamic**（Computer `[LOCKED GOAL]`）仍接在 cacheable 后面、无独立标记。

**设计意图**：system 人设单独一块，便于跨轮命中；历史再单独一块，随会话变长而增长。每轮动态注入放在第二个标记之后，避免屏幕/任务板拖垮历史前缀。

---

## cacheable / dynamic 划分（system 内）

| 分区 | 内容 | 说明 |
|------|------|------|
| **cacheable** | `COMMUNICATION_PUBLIC`、Agent/Skills、工具 `## Tools`、`[Environment]`、`[USER RULES]`、`# Project Context` | 同一天、同会话配置下多轮不变；**`[Environment]` 仅日期按自然日变**；**`# Project Context`** 随工作区 / `AGENTS.md` 变 |
| **dynamic** | **`[LOCKED GOAL]`**（Computer 锁定 `tool_args.goal` 时） | 锁定 goal 每轮可能变 |

task board 走 user 注入路径（有内容时）；Computer **操作历史**在 `[CUR_SCREEN]` user 消息中。

---

## 请求体形态

- 启用且 cacheable 非空：system `content` 为数组，第一段带 `cache_control`。
- 另有对话历史：历史最后一条 `content` 改为数组并带 `cache_control`。
- 然后追加 `injected_tail`（不带标记）。
- 未启用：cacheable + dynamic 合并为单条字符串，历史也不打标记。

`content` 必须是数组才能带标记（字符串 content 无效）。官方限制：最少约 1024 Token；从标记向前最多看 20 个 content 块；有效期 5 分钟。

---

## 命中与计费（摘要）

显式缓存与隐式缓存互斥。创建块约输入单价 125%，命中约 10%。详见官方文档。

请求结束时的 `LLM token summary`（info）会打印会话累计 `cache_hit` / `cache_miss`（来自 `usage.prompt_tokens_details.cached_tokens`）。见 [`llm-token-usage-logging.md`](./llm-token-usage-logging.md)。

---

## 相关代码

| 符号 | 文件 |
|------|------|
| `SystemPromptSections` | `models.rs` |
| `push_openai_system_messages` / `attach_ephemeral_cache_control` | `openai_convert.rs` |
| `push_env_to_cacheable` | `chat_service/prompts.rs` |
| `prepare_single_agent_round_prompts` | `chat_service/single_agent_prompt.rs` |
| `CommonUserDynamicInjectHook` | `extensions/common_user_dynamic_inject_hook.rs` |
| `ComputerTierDynamicHook` | `agents/computer/extension_hooks/tier_dynamic.rs` |

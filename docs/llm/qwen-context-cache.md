# 千问显式 Context Cache（system 提示）

应用在向阿里云百炼 **OpenAI 兼容** `POST …/chat/completions` 发请求时，对符合条件的千问模型在 **稳定 system 前缀** 上启用显式缓存（`cache_control.type = ephemeral`）。

官方文档：[千问模型的 Context Cache 功能](https://help.aliyun.com/zh/model-studio/context-cache)

组装顺序见 **[`../internals/llm-prompt-assembly-order.md`](../internals/llm-prompt-assembly-order.md)**。

---

## 何时启用

`models::qwen_explicit_system_cache_enabled(settings)`：DashScope 兼容 `baseUrl` + 模型 id 以 `qwen` 开头。

---

## cacheable / dynamic 划分

| 分区 | 内容 | 说明 |
|------|------|------|
| **cacheable** | `COMMUNICATION_PUBLIC`、Agent/Skills、工具 `## Tools`、`[Environment]` | 同一天、同会话配置下多轮不变；**`[Environment]` 仅日期按自然日变**，不算「每轮动态」 |
| **dynamic** | **`[LOCKED GOAL]`**（Computer 锁定 `tool_args.goal` 时） | 锁定 goal 每轮可能变 |

**设计意图**：仅把真正每轮变的内容放在 `cache_control` 之后，避免拖垮 cacheable 前缀命中。task board 走 user 注入路径（有内容时）；Computer **操作历史**在 `[CUR_SCREEN]` user 消息中，不占 system 缓存。

---

## 请求体形态

- **cacheable** 非空且启用显式缓存：`content` 为两段数组（cacheable 带 `cache_control`，dynamic 不带）。
- 否则：cacheable + dynamic 合并为单条字符串。

---

## 命中与计费（摘要）

显式缓存与隐式缓存互斥。创建块约输入单价 125%，命中约 10%，最少约 1024 Token，有效期 5 分钟。详见官方文档。

请求结束时的 `LLM token summary`（info）会打印会话累计 `cache_hit` / `cache_miss`（来自 `usage.prompt_tokens_details.cached_tokens`）。见 [`llm-token-usage-logging.md`](./llm-token-usage-logging.md)。

---

## 相关代码

| 符号 | 文件 |
|------|------|
| `SystemPromptSections` | `models.rs` |
| `push_env_to_cacheable` | `chat_service/prompts.rs` |
| `prepare_single_agent_round_prompts` | `chat_service/single_agent_prompt.rs` |
| `CommonUserDynamicInjectHook` | `extensions/common_user_dynamic_inject_hook.rs` |
| `ComputerTierDynamicHook` | `agents/computer/extension_hooks/tier_dynamic.rs` |

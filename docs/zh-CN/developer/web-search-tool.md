# 联网搜索工具（`web_search`）

[English](../../en/developer/web-search-tool.md) | 简体中文

Hosted web search for **external** facts. Implemented in `pointer-core` as a first-class tool. The chat stack stays on Chat Completions; only this tool calls the search API.

## What it is

| Backend | When | API | Model |
|---------|------|-----|-------|
| **DeepSeek** | Active chat provider is DeepSeek, or the web-search tier / `POINTER_WEB_SEARCH_MODEL` / `webSearchModel` names a `deepseek*` model | `POST {origin}/responses` with `tools: [{ "type": "web_search" }]`. A trailing `/v1` on the provider base URL is removed | `deepseek-v4-pro`. `deepseek-flash` ignores the tool, so a flash id is rewritten and logged |
| **DashScope** | Active chat provider is Qwen, or the tier names a Qwen model and that key exists | Native generation + `enable_search` + SSE. Text models use `text-generation`; Qwen 3.5+ multimodal ids use `multimodal-generation` | Tier map → legacy `webSearchModel` → first DashScope catalog model. Tool default `pro_max` maps to `max`; multimodal ids force `agent` |

If the chosen backend has no API key, the other configured key is used and a warning is logged. With neither key, the call fails.

The model searches the public web and returns:

- A synthesized **answer**
- **Sources** — DashScope `search_results`, or DeepSeek `url_citation` annotations and `web_search_call` sources

This is **not** a free keyword/SERP API. DashScope bills LLM tokens plus search usage ([Alibaba Cloud web search billing](https://help.aliyun.com/zh/model-studio/web-search)). DeepSeek bills the Responses call on `deepseek-v4-pro` ([Responses API](https://api-docs.deepseek.com/zh-cn/guides/responses_api)).

The chat runtime exposes in-flight UI events (APP + Web):

| Stream event | Purpose |
|--------------|---------|
| `web_search_output_delta` | Incremental answer text while the search round runs |
| `web_search_sources_ready` | First non-empty source list (often from the first SSE chunk) |

History still receives the final **`WebSearchResult` JSON** on the tool message when the round completes (same contract as before).

## Architecture

- **`agent_tool_pass`** delegates to **`web_search::dispatch`** (single entry). The route is chosen before the HTTP call.
- **DeepSeek** sends `stream: true` and reads Responses SSE (`response.output_text.delta`, `response.web_search_call.*`, `response.completed`). `searchStrategy`, `forcedSearch`, and `enableVerticalSearch` are not sent. `enableThinking` maps to `reasoning.effort = high`.
- **DashScope** uses native generation SSE (`X-DashScope-SSE: enable`, `incremental_output`). Text models send string `content` and `prepend_search_result`. Multimodal models (`qwen3.5-plus`, `qwen3.6-plus`, …) send `content: [{ "text": query }]` and `search_strategy: agent`.
- Implementation lives under `tools/web_search/*` (does not modify `provider.rs` / `agent_stream_round.rs`).

## Configuration

1. Configure **DeepSeek** (`https://api.deepseek.com/v1`) or **Qwen** (`https://dashscope.aliyuncs.com/compatible-mode/v1`). The search call reuses that provider's API key.
2. **Settings UI**：智能体 → 场景档位 → 媒体理解 → **更多** → 联网搜索。
   - 选快速 / 标准 / 高级（写入 `agentPerformanceModes.web_search`）
   - 点「模型」配置三档模型（写入 `agentModeLlm.web_search`）。列表含千问模型，以及 DeepSeek 的 `deepseek-v4-pro`
3. No tier and no legacy `webSearchModel`: the **active chat provider** decides. DeepSeek → Responses search. Otherwise DashScope when that key exists.
4. `POINTER_WEB_SEARCH_MODEL` overrides the tier. A `deepseek*` id selects DeepSeek; any other id selects DashScope.
5. DashScope fallback when the tier is empty: legacy **`webSearchModel`**, then the first DashScope catalog model, then `DEFAULT_WEB_SEARCH_MODEL`.

International Qwen accounts: use a provider base URL on `dashscope-intl.aliyuncs.com`; the client derives the matching native API host.

## Agents

| Agent / tool | Role |
|--------------|------|
| **coder** / **general** | May call **`web_search`** directly (Tool mode) |
| **`explore`** worker | Codebase only — **no** web search |

`research` 深度研究子智能体已下线；联网搜索档位在媒体列「更多」配置，不再走独立 agent。

**When to use what**

- Repo symbols / call chains → **`explore`**
- External facts / news / docs → **`web_search`**
- Fetch a specific URL body → **`web_fetch`** (separate tool)

## Token usage reporting

Token accounting follows the same path as normal chat LLM rounds:

| Path | What is metered | How it is recorded |
|------|-----------------|-------------------|
| **Main agent** LLM turns | Chat/completions `usage` on each stream round | `record_llm_round` → SQLite `usage_accum` → `finalize_run` → platform `token-usage` upload |
| **`web_search` tool** | DashScope or DeepSeek `usage` (`input_tokens` / `output_tokens` / `total_tokens`) | Direct `token_usage_store::record_round` on the **lead** `agent_instance_id` |

At end of `run_chat`, `token_usage_store::finalize_run` enqueues one platform report per agent instance (with optional conversation archive zip). Tool results still include **`usage`** / **`searchCount`** in JSON for the model; platform reporting uses the accum path above.

Serial and parallel searches use the same owned `AgentInstanceScope` accounting path. Search usage does not update the agent-loop in-memory `last_round_prompt_tokens`, because that value is reserved for context-compression decisions based on the agent conversation prompt.

**Not included:** DashScope search-plugin surcharges (`usage.plugins.search.count`) are logged locally but not sent as a separate billing field to the platform today.

## Tool parameters

| Field | Description |
|-------|-------------|
| `query` | Required search brief — self-contained goal, scope, format, and citation rules |
| `searchStrategy` | DashScope only. Tool default `pro_max`; also `max`, `turbo`, `agent`, `agent_max`. DeepSeek ignores it |
| `enableThinking` | Tool default `false`. DeepSeek sends `reasoning.effort = high` when true |
| `forcedSearch` | DashScope: force web search instead of a model skip. DeepSeek always sends the `web_search` tool |
| `enableVerticalSearch` | DashScope only (weather, stocks, and similar). DeepSeek ignores it |

Callers send **`query` only** (no SearchAgent system injection). Tool risk: **medium**, **requires approval** (external API cost).

## Manual smoke test

1. Set a DeepSeek or Qwen API key in settings; optionally set 联网搜索档位 under 媒体理解 → 更多.
2. As **coder**, ask: “What is the latest stable Rust edition?” and approve **`web_search`**.
3. While running, confirm the tool card shows streaming **answer** text (and **sources** when available).
4. Confirm tool result JSON has **`sources`** with URLs and **`sourcesForReply`** (single linked Sources block for the agent to paste).

## Tests

- Unit: `parse_search_sse_chunk`, `SearchSseAccumulator`, stream request body — `cargo test -p pointer-core web_search`
- DeepSeek route, request body, citation parse, and wiremock Responses call — `cargo test -p pointer-core deepseek`
- Integration: wiremock SSE fixture — `crates/pointer-core/tests/web_search_integration.rs`
- Tier resolve: `cargo test -p pointer-core effective_web_search_model_tests`

# Web search tool (`web_search`)

DashScope hosted web search for **external** facts. Implemented in `pointer-core` as a first-class tool plus an optional **`research`** sub-agent.

## What it is

Two DashScope paths depending on caller:

| Caller | API | Default model | Default strategy |
|--------|-----|---------------|------------------|
| **Generic tool** (coder, default, …) | Generation `POST …/text-generation/generation` with `enable_search` + SSE | `qwen3-max` | `pro_max` |
| **Research sub-agent** | Responses `POST …/compatible-mode/v1/responses` with agent tools | `qwen3-max-2026-01-23` | `max` + thinking |

The model searches the public web and returns:

- A synthesized **answer**
- **Sources** — URLs from search results (Generation) or `web_search_call` actions (Responses)

This is **not** a free keyword/SERP API — it bills as LLM tokens plus DashScope search usage (see [Alibaba Cloud web search billing](https://help.aliyun.com/zh/model-studio/web-search)).

The chat runtime exposes in-flight UI events (APP + Web):

| Stream event | Purpose |
|--------------|---------|
| `web_search_output_delta` | Incremental answer text while the search round runs |
| `web_search_sources_ready` | First non-empty source list (often from the first SSE chunk) |

History still receives the final **`WebSearchResult` JSON** on the tool message when the round completes (same contract as before).

## Architecture

- **`agent_tool_pass`** delegates to **`web_search::dispatch`** (single entry).
- **Tool mode** (coder, default, research as lead): Generation API SSE (`X-DashScope-SSE: enable`, `incremental_output`, `prepend_search_result`); `input.messages` = `[{ role: user, content: query }]`.
- **ResearchSubAgent mode** (`run_subagent` + `agentId=research`): Responses API with agent tools; SearchAgent system = `research/AGENT.md` **SearchAgent** section + sub-agent **local history** + `query`.
- Implementation lives under `tools/web_search/*` and `agents/research/web_search/*` (does not modify `provider.rs` / `agent_stream_round.rs`).

## Configuration

1. Configure the **Qwen** provider in settings (default base URL: `https://dashscope.aliyuncs.com/compatible-mode/v1`).
2. Set the **Qwen provider API key** — web search reuses this key directly (no separate search key or env var).
3. Optional: **`webSearchModel`** in settings. When empty, use the first model on a DashScope-compatible provider; compile-time `WEB_SEARCH_MODEL` / `DEFAULT_WEB_SEARCH_MODEL` is last resort. Any configured model id is passed through (with Generation API fallback routing when needed).
4. Optional env: **`POINTER_WEB_SEARCH_MODEL`** overrides the search model.

International accounts: use a provider base URL on `dashscope-intl.aliyuncs.com`; the client derives the matching native API host.

## Token usage reporting

Token accounting follows the same path as normal chat LLM rounds:

| Path | What is metered | How it is recorded |
|------|-----------------|-------------------|
| **Main agent** LLM turns | Chat/completions `usage` on each stream round | `record_llm_round` → SQLite `usage_accum` → `finalize_run` → platform `token-usage` upload |
| **`web_search` tool** | DashScope native response `usage` (`input_tokens` / `output_tokens` / `total_tokens`) | Direct `token_usage_store::record_round` on the **lead** `agent_instance_id` |
| **`research` sub-agent** orchestration | Sub-agent chat rounds | `record_llm_round` on the **sub-agent** `agent_instance_id` |
| **`research` → `web_search` calls** | Each tool’s DashScope `usage` | Direct `token_usage_store::record_round` on the **same sub-agent** `agent_instance_id` |

At end of `run_chat`, `token_usage_store::finalize_run` enqueues one platform report per agent instance (with optional conversation archive zip). Tool results still include **`usage`** / **`searchCount`** in JSON for the model; platform reporting uses the accum path above.

Serial and parallel searches use the same owned `AgentInstanceScope` accounting path. Search usage does not update the agent-loop in-memory `last_round_prompt_tokens`, because that value is reserved for context-compression decisions based on the agent conversation prompt.

**Not included:** DashScope search-plugin surcharges (`usage.plugins.search.count`) are logged locally but not sent as a separate billing field to the platform today.

## Tool parameters

| Field | Description |
|-------|-------------|
| `query` | Required search brief — self-contained goal, scope, format, and citation rules |
| `searchStrategy` | Tool default `pro_max`; also `max`, `turbo`, `agent`, `agent_max` |
| `enableThinking` | Tool default `false`; research sub-agent default `true` |
| `forcedSearch` | Force web search instead of model skip |
| `enableVerticalSearch` | Weather, stocks, etc. |

**Context:** only the **research sub-agent** path injects SearchAgent system rules and conversation history. All other callers send **`query` only**.

Tool risk: **medium**, **requires approval** (external API cost).

## Agents

| Agent | Role |
|-------|------|
| **coder** / **default** | May call **`web_search`** directly (Tool mode) |
| **`research`** worker | Web-only deep research via **`run_subagent`**; multiple **`web_search`** rounds (ResearchSubAgent mode); Markdown digest with **`## Sources`** |
| **`explore`** worker | Codebase only — **no** web search |

**When to use what**

- Repo symbols / call chains → **`explore`**
- Quick external fact → **`web_search`**
- Multi-query doc/version/news research → **`research`**
- Fetch a specific URL body → not in scope (future roadmap: HTTP/MCP)

## Manual smoke test

1. Set Qwen API key in settings.
2. As **coder**, ask: “What is the latest stable Rust edition?” and approve **`web_search`**.
3. While running, confirm the tool card shows streaming **answer** text (and **sources** when available).
4. Confirm tool result JSON has **`sources`** with URLs and **`sourcesForReply`** (single linked Sources block for the agent to paste).
5. Delegate: **`run_subagent`** `agentId=research` with a multi-part doc question; confirm parent receives Markdown with **`## Sources`** (linked titles, no duplicate plain-title + numbered list).

## Tests

- Unit: `parse_search_sse_chunk`, `SearchSseAccumulator`, stream request body — `cargo test -p pointer-core web_search`
- Integration: wiremock SSE fixture — `crates/pointer-core/tests/web_search_integration.rs`

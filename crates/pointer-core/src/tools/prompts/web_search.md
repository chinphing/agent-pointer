### `web_search`

Search the **public web** via DashScope **Generation API** (`enable_search` + `search_strategy: pro_max`). Each call uses **SSE streaming**: sources appear first, then the answer streams incrementally.

Use for **external** facts: API docs, release notes, news, prices, weather, library versions. For **codebase** mapping, use **`file`** (read-only **`method`** values) or delegate **`explore`**. For deep multi-query research, delegate **`research`** (uses Responses API with agent tools).

**Requires** a configured **Qwen provider API key** in settings. Search calls use **`webSearchModel`** (default **`qwen3-max`** on Generation API); the research sub-agent uses Responses API separately.

#### Parameters

- **`query`** — Required. A **self-contained search brief** (sent as the user message).
- **`searchStrategy`** — Optional. Default **`pro_max`**. Also accepts `max`, `turbo`. Alias **`search_strategy`**.
- **`enableThinking`** — Optional. Default **`false`**. Alias **`enable_thinking`**.
- **`forcedSearch`** — Optional. Force web search. Alias **`forced_search`**.
- **`enableVerticalSearch`** — Optional. Weather, stocks, etc. Alias **`enable_vertical_search`**.

**Context modes:**

- **Default (coder, default, research lead, etc.)** — Generation API, **`query`** only.
- **Research sub-agent** — Responses API with **`web_search`**, **`web_extractor`**, **`code_interpreter`**; SearchAgent system + history + **`query`**.

#### Response shape

JSON string with fields such as:

- **`ok`**, **`query`**, **`answer`** — `answer` has `[N]` linkified to `[title](url)` when `sources` are available
- **`sources`** — `{ index, title, url, siteName? }`; **`index` matches inline `[N]`**
- **`sourcesForReply`** — **paste this once** as your Sources section: **`N. [siteName · title](url)`** per line when `siteName` is present, else **`N. [title](url)`** (`N` = `sources[].index`, matches inline `[N]`)
- **`citationBaseIndex`** — offset applied this call (prior max index in the same user turn); multi-search **`[N]`** values are globally unique
- **`sourcesCitationMarkdown`** — index map for `[N]` in `answer` (agent reference, not a second Sources block)
- **`citationGuide`** — how to use the fields above
- **`searchCount`**, **`usage`**, **`model`**, **`searchStrategy`**

While running, the UI shows **sources** as soon as search completes, then **answer** text incrementally via SSE.

#### Usage discipline

- Write **`query`** as a self-contained brief.
- Prefer **one focused search task** per call; split broad topics into multiple calls or delegate **`research`**.
- **Single search** in your reply: append **`sourcesForReply` verbatim** — **`N. [title](url)`** matches **`[N]`** in that call's **`answer`**.
- **Multiple searches** in the same user turn: each call shifts indices by **`citationBaseIndex`** — **`[N]`** stays unique across calls; concatenate all **`sourcesForReply`** under one **`## Sources`** (dedupe URLs if needed).
- **Do not** paste plain titles without **`N.`** or without markdown links.
- **Do not** rebuild Sources from **`sources[]`** by hand — use **`sourcesForReply`** (or merge per **`multiSearchGuide`**).

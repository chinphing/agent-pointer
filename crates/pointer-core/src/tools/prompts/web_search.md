### `web_search`

Search the **public web** via DashScope **Generation API** (`enable_search` + `search_strategy: pro_max`). Each call uses **SSE streaming**: sources appear first, then the answer streams incrementally.

Use for **external** facts: API docs, release notes, news, prices, weather, library versions. For **codebase** mapping, use **`file:*`** or delegate **`explore`**. For deep multi-query research, delegate **`research`** (uses Responses API with agent tools).

**Requires** a configured **Qwen provider API key** in settings. Search calls use **`webSearchModel`** (default **`qwen3-max`** on Generation API); the research sub-agent uses Responses API separately.

#### Parameters

All keys are JSON properties on the root **`tool_args`** object.

- **`query`** — Required. A **self-contained search brief** (sent as the user message).
- **`searchStrategy`** — Optional. Default **`pro_max`**. Also accepts `max`, `agent_max`, `turbo`, `agent`. Alias **`search_strategy`**.
- **`enableThinking`** — Optional. Default **`false`**. Alias **`enable_thinking`**.
- **`forcedSearch`** — Optional. Force web search. Alias **`forced_search`**.
- **`enableVerticalSearch`** — Optional. Weather, stocks, etc. Alias **`enable_vertical_search`**.

**Context modes:**

- **Default (coder, default, research lead, etc.)** — Generation API, **`query`** only.
- **Research sub-agent** — Responses API with **`web_search`**, **`web_extractor`**, **`code_interpreter`**; SearchAgent system + history + **`query`**.

#### Response shape

JSON string with fields such as:

- **`ok`**, **`query`**, **`answer`**
- **`sources`** — URLs from search results (deduped)
- **`searchCount`**, **`usage`**, **`model`**, **`searchStrategy`**

While running, the UI shows **sources** as soon as search completes, then **answer** text incrementally via SSE.

#### Usage discipline

- Write **`query`** as a self-contained brief.
- Prefer **one focused search task** per call; split broad topics into multiple calls or delegate **`research`**.
- Cite URLs from **`sources`** when reporting facts to the user.

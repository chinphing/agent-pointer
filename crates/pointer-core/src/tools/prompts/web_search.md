---
schema:
  type: object
  properties:
    query:
      type: string
  required:
    - query
  additionalProperties: false
---

### `web_search`

Fetch **live public web** evidence. **Most turns should not call this tool.**

**Call with `query` only** — do not pass any other arguments.

#### Default (answer first)

For ordinary questions, answer from **this conversation** and **your own
knowledge** first.

`web_search` is **not** a substitute for thinking. Stable facts, concepts,
how-things-work, classic APIs, and general tutoring **do not** need a search.

#### Decision order

1. Can you answer **confidently** from the **thread** and **general knowledge**
   without needing **today's** web?
   → **Do not** search. Reply directly.
2. Does the user **explicitly** want online lookup, citations, or verification?
   → Search.
3. Is the gap **live / time-sensitive** (news, current price, weather, policy
   today, release **after** your knowledge may be stale)?
   → Search with one focused **`query`**.

#### When to use

- User says search / look up / verify online / need sources
- Live or post-cutoff facts (today's news, current market price, weather now)
- External official docs or release notes not stable in your knowledge
- **One focused question** per call

#### When **not** to use

- Normal Q&A, explanation, writing, brainstorming, summarization
- Facts you already know well and freshness does not matter
- Answer already in the thread or **`[TASK_BOARD]`**
- **Do not** search to double-check every step or every subtopic

#### `query`

Required. A **self-contained search brief**.

Add the current year from **`[Environment]`** → **`Local date`** **only** when
freshness matters and the user did not specify a year.
**Do not** append a year to every query by default.

#### Citations in user-facing replies

When you **did** search and cite external facts:

- **One call** — paste **`sourcesForReply`** verbatim (linked titles).
- **Multiple calls** in one user turn — merge all **`sourcesForReply`** under one
  **`## Sources`** section; indices stay unique via **`citationBaseIndex`**.
- **Do not** rebuild Sources from raw **`sources[]`** by hand.

Read **`citationGuide`** in the tool result when unsure.

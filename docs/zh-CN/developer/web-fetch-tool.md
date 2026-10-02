# 联网抓取工具（`web_fetch`）

[English](../../en/developer/web-fetch-tool.md) | 简体中文

HTTP GET for **known public URLs**, inspired by Hermes **`web_extract`** and OpenClaw **`web_fetch`**.
Returns readable text (HTML stripped; JSON pretty-printed). Does **not** execute JavaScript.

## Parameters

| Field | Description |
|-------|-------------|
| `url` | Single URL |
| `urls` | Up to **5** URLs per call (batch) |
| `maxChars` | Truncate each page (default **50000**, max **100000**) |
| `extractMode` | `markdown` (default) or `text` |

Provide **`url`** and/or **`urls`** (at least one).

## Behavior

- **http/https only**; embedded credentials rejected
- **SSRF**: blocks localhost, private/link-local/metadata addresses; re-checks redirect targets (max **5** redirects)
- Timeout **30s**; download cap **2MB**
- Chrome-like `User-Agent`
- Result JSON: `pages[]` with `ok`, `url`, `finalUrl`, `status`, `contentType`, `contentKind`, `content`, `truncated`

## Agents

Enabled by default on **`general`** and **`coder`**. Prefer **`web_search`** for discovery, then **`web_fetch`** on specific links.

## Implementation

`crates/pointer-core/src/tools/web_fetch/` — registered from `tools/builtin.rs`.
Tool prompt: `tools/prompts/web_fetch.md`.

HTTP uses `reqwest::blocking` on a **dedicated OS thread** (not the Tokio worker).
Calling blocking reqwest directly inside the async chat loop panics with
`Cannot drop a runtime in a context where blocking is not allowed`.

## Related

- [web-search-tool.md](web-search-tool.md) — DashScope hosted search
- Hermes `web_extract` (vendor scrape backends) — Pointer uses direct HTTP instead of Firecrawl/Tavily

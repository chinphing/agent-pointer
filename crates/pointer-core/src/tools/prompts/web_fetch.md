---
schema:
  type: object
  properties:
    url:
      type: string
    urls:
      type: array
      items:
        type: string
      maxItems: 5
    maxChars:
      type: integer
      minimum: 1000
      maximum: 100000
    extractMode:
      type: string
      enum:
        - markdown
        - text
  additionalProperties: false
---

### `web_fetch`

Fetch **one or more public http(s) URLs** and return readable text
(HTML stripped to plain text; JSON pretty-printed). Does **not** run JavaScript.

Use when you already know the URL (docs, release notes, issue pages).
For open-ended discovery, use **`web_search`** first, then **`web_fetch`** on
promising links.

#### Parameters

- **`url`** — single URL, or
- **`urls`** — up to **5** URLs per call (Hermes-style batch)
- **`maxChars`** — optional truncate length (default **50000**, max **100000**)
- **`extractMode`** — `markdown` (default) or `text` (same extractor today)

Provide **`url`** and/or **`urls`** (at least one).

#### When to use

- Read a specific documentation / README / changelog / RFC URL
- Pull the body of a page found via **`web_search`**
- Compare a few known URLs in one call (`urls`)

#### When **not** to use

- You do not have a concrete URL → **`web_search`**
- Local files → **`file_read`**
- Pages that need login or heavy JS → browser skill / **`computer`**, not this tool
- Private/internal hosts (blocked)

#### Result

JSON with **`pages[]`**: each entry has **`ok`**, **`url`**, **`finalUrl`**,
**`status`**, **`contentType`**, **`contentKind`**, **`content`**, **`truncated`**.

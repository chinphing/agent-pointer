# Rust string truncation

English | [简体中文](../../zh-CN/developer/rust-text-truncation.md)

When `pointer-core` needs to truncate user-visible text or a log preview, **never** slice by byte with `&s[..n]` / `s[..s.len().min(n)]` — it may cut in the middle of a multi-byte character such as CJK or emoji and **panic**.

Always use [`crates/pointer-core/src/text_util.rs`](../../../crates/pointer-core/src/text_util.rs):

| Function | Purpose |
|------|------|
| `floor_char_boundary` / `ceil_char_boundary` | Clamp a byte index to a valid UTF-8 boundary |
| `slice_bytes(s, start, end)` | Slice safely by byte index (clamps boundaries internally) |
| `split_at_byte(s, mid)` | Split safely by byte index |
| `take_chars(s, n)` | Take the first n Unicode chars, no ellipsis (log previews, etc.) |
| `truncate_chars(s, n)` | Truncate by char count, appending `…` when over |
| `truncate_chars_fit(s, n)` | Keep the total result length (including `…`) within n chars (conversation titles, etc.) |
| `truncate_bytes(s, n)` | Truncate by a UTF-8 **byte** budget (HTTP error bodies, provider logs, etc.) |
| `truncate_for_log(s, n)` | For logs; `…(+N chars)` when over |
| `match_centered_snippet(text, query, radius, …)` | Cut a preview centered on the query term (sidebar search, etc.); do not use FTS5 `snippet()` for CJK UI previews |
| `match_centered_excerpt(text, query, max_chars)` | At most `max_chars` around the hit (outbound tool body; start of the text when there is no hit) |

## Example

```rust
use crate::text_util::{take_chars, truncate_chars, truncate_bytes};

log::info!("goal={}", take_chars(&goal, 80));
let title = truncate_chars_fit(&raw_title, 24);
anyhow::bail!("HTTP {}: {}", status, truncate_bytes(&body, 400));
```

## Why this is mandatory

This rule also applies to model/user text **written into persisted documents** (task-board findings, Office text extraction, etc.), not just logs and UI previews. Such a panic does not stay confined to the current function: it propagates up through the tool call and the agent loop, killing the whole run's task, which shows up as the conversation being stuck in "running" forever. See the Panic containment section of [Chat run errors](../../zh-CN/developer/chat-run-errors.md).

## Migration notes

Historical code still has local `fn truncate(...)` (e.g. in `context_compression`, `tools/display`). New code should use `text_util` directly; when you touch the old implementations you may change them to delegate to `text_util`.

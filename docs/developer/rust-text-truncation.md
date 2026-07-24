# Rust 字符串截断

`pointer-core` 中需要截断用户可见文本或日志预览时，**禁止**使用 `&s[..n]` / `s[..s.len().min(n)]` 按字节切片 —— 可能在 CJK、emoji 等多字节字符中间切断并 **panic**。

统一使用 [`crates/pointer-core/src/text_util.rs`](../../crates/pointer-core/src/text_util.rs)：

| 函数 | 用途 |
|------|------|
| `floor_char_boundary` / `ceil_char_boundary` | 把字节下标钳到合法 UTF-8 边界 |
| `slice_bytes(s, start, end)` | 按字节下标安全切片（内部钳边界） |
| `split_at_byte(s, mid)` | 按字节下标安全拆分 |
| `take_chars(s, n)` | 取前 n 个 Unicode 字符，不加省略号（日志预览等） |
| `truncate_chars(s, n)` | 按字符数截断，超出时追加 `…` |
| `truncate_chars_fit(s, n)` | 结果总长度（含 `…`）不超过 n 个字符（会话标题等） |
| `truncate_bytes(s, n)` | 按 UTF-8 **字节**预算截断（HTTP 错误体、provider 日志等） |
| `truncate_for_log(s, n)` | 日志用，超出时 `…(+N chars)` |
| `match_centered_snippet(text, query, radius, …)` | 以查询词为中心截取预览（侧边栏搜索等）；勿用 FTS5 `snippet()` 做 CJK UI 预览 |

## 示例

```rust
use crate::text_util::{take_chars, truncate_chars, truncate_bytes};

log::info!("goal={}", take_chars(&goal, 80));
let title = truncate_chars_fit(&raw_title, 24);
anyhow::bail!("HTTP {}: {}", status, truncate_bytes(&body, 400));
```

## 迁移说明

历史代码里仍有局部 `fn truncate(...)`（如 `context_compression`、`tools/display`）。新代码请直接用 `text_util`；触到旧实现时可顺手改为委托 `text_util`。

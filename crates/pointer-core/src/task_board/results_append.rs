//! Incremental append for `validate_results` / `extract_results` (dedupe lines, coalesce cumulative dumps).

use super::model::{parse_string_array, push_snippet};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendWarning {
    pub code: &'static str,
    pub item_id: Option<String>,
}

fn normalize_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_normalized_lines(results: &[String]) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for entry in results {
        for line in entry.lines() {
            let n = normalize_line(line);
            if !n.is_empty() {
                set.insert(n);
            }
        }
    }
    set
}

fn split_nonempty_lines(snippet: &str) -> Vec<String> {
    snippet
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Append-only merge: one new evidence line per item when possible; drop duplicates and coalesce cumulative blocks.
pub fn append_results_incremental(
    prev: &[String],
    patch_value: Option<&Value>,
) -> (Vec<String>, Vec<AppendWarning>) {
    let Some(val) = patch_value else {
        return (prev.to_vec(), Vec::new());
    };
    let mut out = prev.to_vec();
    let mut warnings = Vec::new();
    let incoming = parse_string_array(val);
    if incoming.is_empty() {
        return (out, warnings);
    }

    for snippet in incoming {
        let lines = split_nonempty_lines(&snippet);
        if lines.is_empty() {
            continue;
        }

        let mut existing = collect_normalized_lines(&out);

        if lines.len() == 1 {
            let line = &lines[0];
            let norm = normalize_line(line);
            if existing.contains(&norm) {
                warnings.push(AppendWarning {
                    code: "validate_results_duplicate_line",
                    item_id: None,
                });
                log::info!("task_board: validate_results duplicate line skipped");
                continue;
            }
            if let Some(last) = out.last() {
                let nlast = normalize_line(last);
                if norm.starts_with(&nlast) && norm.len() > nlast.len() {
                    out.pop();
                    push_snippet(&mut out, line);
                    warnings.push(AppendWarning {
                        code: "validate_results_cumulative_replaced",
                        item_id: None,
                    });
                    log::info!("task_board: validate_results cumulative block replaced last entry");
                    continue;
                }
            }
            push_snippet(&mut out, line);
            continue;
        }

        let mut added = 0usize;
        for line in &lines {
            let norm = normalize_line(line);
            if norm.is_empty() || existing.contains(&norm) {
                continue;
            }
            push_snippet(&mut out, line);
            existing.insert(norm);
            added += 1;
        }
        if added == 0 {
            warnings.push(AppendWarning {
                code: "validate_results_duplicate_block",
                item_id: None,
            });
            log::info!("task_board: validate_results block had no new lines");
        } else if added < lines.len() {
            warnings.push(AppendWarning {
                code: "validate_results_partial_dedup",
                item_id: None,
            });
            log::info!(
                "task_board: validate_results kept {added} new line(s) from {} line block",
                lines.len()
            );
        }
    }

    (out, warnings)
}

pub fn append_warning_to_json(w: &AppendWarning, item_id: &str) -> Value {
    serde_json::json!({
        "code": w.code,
        "item_id": item_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_single_new_line() {
        let (out, w) = append_results_incremental(
            &[],
            Some(&serde_json::json!("1/10: 138 - Not found")),
        );
        assert_eq!(out.len(), 1);
        assert!(w.is_empty());
    }

    #[test]
    fn dedupes_duplicate_line() {
        let prev = vec!["1/10: 138 - Not found".into()];
        let (out, w) = append_results_incremental(
            &prev,
            Some(&serde_json::json!("1/10: 138 - Not found")),
        );
        assert_eq!(out.len(), 1);
        assert!(w.iter().any(|x| x.code == "validate_results_duplicate_line"));
    }

    #[test]
    fn multiline_block_keeps_only_new_lines() {
        let prev = vec![
            "1/10: a - Not found\n2/10: b - Not found\n3/10: c - Not found\n4/10: d - Not found"
                .into(),
        ];
        let block = "1/10: a - Not found\n2/10: b - Not found\n3/10: c - Not found\n4/10: d - Not found\n5/10: e - Found";
        let (out, w) = append_results_incremental(&prev, Some(&serde_json::json!(block)));
        assert!(out.iter().any(|s| s.contains("5/10")));
        assert!(w.iter().any(|x| x.code == "validate_results_partial_dedup"));
    }

    #[test]
    fn cumulative_single_line_replaces_last_entry() {
        let prev = vec!["1/10: a".into()];
        let (out, w) = append_results_incremental(
            &prev,
            Some(&serde_json::json!("1/10: a | 2/10: b")),
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("2/10"));
        assert!(w.iter().any(|x| x.code == "validate_results_cumulative_replaced"));
    }
}

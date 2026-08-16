//! Shared line-oriented text diff used by file tools and turn baseline review.

use similar::{ChangeTag, TextDiff};

/// Collapse threshold: consecutive unchanged lines > this → folded.
const COLLAPSE_THRESHOLD: usize = 6;

/// Build diff lines from old/new content, folding long unchanged runs.
pub fn compute_diff_lines(old: &str, new: &str) -> (Vec<serde_json::Value>, serde_json::Value) {
    let diff = TextDiff::from_lines(old, new);
    let mut all: Vec<serde_json::Value> = Vec::new();
    let mut adds = 0usize;
    let mut dels = 0usize;

    for c in diff.iter_all_changes() {
        let text = c.value().to_string().trim_end_matches('\n').to_string();
        match c.tag() {
            ChangeTag::Equal => {
                all.push(serde_json::json!({"type": "unchanged", "text": text}));
            }
            ChangeTag::Insert => {
                adds += 1;
                all.push(serde_json::json!({"type": "ins", "text": text}));
            }
            ChangeTag::Delete => {
                dels += 1;
                all.push(serde_json::json!({"type": "del", "text": text}));
            }
        }
    }

    // Post-process: fold consecutive unchanged runs > COLLAPSE_THRESHOLD
    let mut folded: Vec<serde_json::Value> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        if all[i]["type"] == "unchanged" {
            let start = i;
            while i < all.len() && all[i]["type"] == "unchanged" {
                i += 1;
            }
            let count = i - start;
            if count > COLLAPSE_THRESHOLD {
                for j in start..start + 3 {
                    folded.push(all[j].clone());
                }
                let mut hidden: Vec<String> = Vec::new();
                for j in start + 3..i - 3 {
                    hidden.push(all[j]["text"].as_str().unwrap_or("").to_string());
                }
                folded.push(serde_json::json!({
                    "type": "collapse",
                    "text": hidden.len().to_string(),
                    "hidden": hidden,
                }));
                for j in i - 3..i {
                    folded.push(all[j].clone());
                }
            } else {
                for j in start..i {
                    folded.push(all[j].clone());
                }
            }
        } else {
            folded.push(all[i].clone());
            i += 1;
        }
    }

    let stats = serde_json::json!({ "adds": adds, "dels": dels });
    (folded, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_long_unchanged_runs() {
        let old = (0..20)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let mut new_lines: Vec<String> = (0..20).map(|i| format!("L{i}")).collect();
        new_lines[10] = "CHANGED".into();
        let new = new_lines.join("\n") + "\n";
        let (lines, stats) = compute_diff_lines(&old, &new);
        assert_eq!(stats["adds"].as_u64().unwrap(), 1);
        assert_eq!(stats["dels"].as_u64().unwrap(), 1);
        assert!(lines.iter().any(|l| l["type"] == "collapse"));
    }
}

//! Codex-aligned app list entries and text rendering.

use chrono::{DateTime, Duration, Local, NaiveDate};

/// One row returned by [`super::list_apps`], rendered like Codex Computer Use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedApp {
    pub name: String,
    pub identifier: String,
    pub running: bool,
    pub frontmost: bool,
    pub last_used: Option<NaiveDate>,
    pub uses: Option<i32>,
    /// Windows/Linux window title when running.
    pub window_title: Option<String>,
    pub pid: Option<u32>,
}

impl ListedApp {
    /// macOS Codex format: `Name — com.example.app [frontmost, running, last-used=..., uses=...]`
    pub fn render_macos_line(&self) -> String {
        let mut markers: Vec<String> = Vec::new();
        if self.frontmost {
            markers.push("frontmost".into());
        }
        if self.running {
            markers.push("running".into());
        }
        if let Some(day) = self.last_used {
            markers.push(format!("last-used={day}"));
        }
        if let Some(uses) = self.uses {
            markers.push(format!("uses={uses}"));
        }
        format!(
            "{} — {} [{}]",
            self.name,
            self.identifier,
            markers.join(", ")
        )
    }

    /// Windows/Linux Codex format: `name -- identifier [running, pid=..., window=..., last-used=..., uses=...]`
    pub fn render_process_line(&self) -> String {
        let mut markers: Vec<String> = Vec::new();
        if self.running {
            markers.push("running".into());
        }
        if let Some(pid) = self.pid {
            markers.push(format!("pid={pid}"));
        }
        if let Some(title) = self
            .window_title
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "untitled")
        {
            markers.push(format!("window={title}"));
        }
        if let Some(day) = self.last_used {
            markers.push(format!("last-used={day}"));
        }
        if let Some(uses) = self.uses {
            markers.push(format!("uses={uses}"));
        }
        format!(
            "{} -- {} [{}]",
            self.name,
            self.identifier,
            markers.join(", ")
        )
    }
}

pub fn render_list(lines: &[ListedApp], macos: bool) -> String {
    lines
        .iter()
        .map(|app| {
            if macos {
                app.render_macos_line()
            } else {
                app.render_process_line()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn recent_usage_cutoff_days(days: i64) -> NaiveDate {
    let today = Local::now().date_naive();
    today - Duration::days(days)
}

pub fn parse_mdls_date(raw: &str) -> Option<NaiveDate> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.date_naive());
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").ok()
}

pub fn compare_listed_apps(a: &ListedApp, b: &ListedApp) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.frontmost != b.frontmost {
        return if a.frontmost { Ordering::Less } else { Ordering::Greater };
    }
    if a.running != b.running {
        return if a.running { Ordering::Less } else { Ordering::Greater };
    }
    match (a.last_used, b.last_used) {
        (Some(l), Some(r)) if l != r => return r.cmp(&l),
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        _ => {}
    }
    match (a.uses, b.uses) {
        (Some(l), Some(r)) if l != r => return r.cmp(&l),
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        _ => {}
    }
    a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_line_includes_markers() {
        let app = ListedApp {
            name: "WeChat".into(),
            identifier: "com.tencent.xinWeChat".into(),
            running: true,
            frontmost: true,
            last_used: NaiveDate::from_ymd_opt(2026, 6, 1),
            uses: Some(12),
            window_title: None,
            pid: None,
        };
        let line = app.render_macos_line();
        assert!(line.contains("WeChat — com.tencent.xinWeChat"));
        assert!(line.contains("frontmost"));
        assert!(line.contains("running"));
    }

    #[test]
    fn process_line_includes_recent_markers() {
        let app = ListedApp {
            name: "WeChat".into(),
            identifier: "WeChat.exe".into(),
            running: true,
            frontmost: false,
            last_used: NaiveDate::from_ymd_opt(2026, 6, 1),
            uses: Some(5),
            window_title: None,
            pid: Some(1234),
        };
        let line = app.render_process_line();
        assert!(line.contains("running"));
        assert!(line.contains("pid=1234"));
        assert!(line.contains("last-used="));
    }
}

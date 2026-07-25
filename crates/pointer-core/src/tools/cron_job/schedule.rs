//! Friendly schedule strings → recurring cron or one-shot fire time (local TZ).

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};

const WEEKDAY_ZH: &[(&str, u8)] = &[
    ("周日", 0),
    ("周天", 0),
    ("周一", 1),
    ("周二", 2),
    ("周三", 3),
    ("周四", 4),
    ("周五", 5),
    ("周六", 6),
];

const WEEKDAY_EN: &[(&str, u8)] = &[
    ("sunday", 0),
    ("monday", 1),
    ("tuesday", 2),
    ("wednesday", 3),
    ("thursday", 4),
    ("friday", 5),
    ("saturday", 6),
];

/// Placeholder stored in `cron_jobs.cron_expr` for one-shot jobs (never passed to
/// `cron::Schedule`).
pub const ONCE_CRON_PLACEHOLDER: &str = "@once";

/// Parsed create-time schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedSchedule {
    Recurring { cron_expr: String },
    Once { fire_at_ms: i64 },
}

/// Parse a user- or model-supplied schedule into recurring cron or one-shot.
///
/// One-shot forms (Hermes-aligned):
/// - Relative: `30m`, `2h`, `1d` (case-insensitive)
/// - Absolute: RFC3339 or `YYYY-MM-DDTHH:MM:SS` (no offset → local)
///
/// Recurring: existing friendly presets and raw 6-field cron.
pub fn parse_schedule(input: &str) -> Result<ParsedSchedule> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(anyhow!("schedule must not be empty"));
    }

    if let Some(once) = try_parse_relative_once(raw)? {
        return Ok(once);
    }
    if let Some(once) = try_parse_iso_once(raw)? {
        return Ok(once);
    }

    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() == 6 && parts[0] == "0" {
        validate_cron_expr(raw)?;
        return Ok(ParsedSchedule::Recurring {
            cron_expr: raw.to_string(),
        });
    }

    let lower = raw.to_lowercase();
    let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();

    if matches!(
        compact.as_str(),
        "everyminute" | "every_minute" | "每分钟" | "每分钟执行"
    ) {
        return Ok(ParsedSchedule::Recurring {
            cron_expr: "0 * * * * *".into(),
        });
    }

    if let Some(n) = parse_every_n(&compact, "minutes", "分钟") {
        let n = clamp(n, 1, 59);
        return Ok(ParsedSchedule::Recurring {
            cron_expr: format!("0 */{n} * * * *"),
        });
    }
    if let Some(n) = parse_every_n(&compact, "hours", "小时") {
        let n = clamp(n, 1, 23);
        return Ok(ParsedSchedule::Recurring {
            cron_expr: format!("0 0 */{n} * * *"),
        });
    }

    if let Some((h, m)) = parse_daily(raw, &compact) {
        return Ok(ParsedSchedule::Recurring {
            cron_expr: format!("0 {} {} * * *", m, h),
        });
    }

    if let Some((dow, h, m)) = parse_weekly(raw, &compact) {
        return Ok(ParsedSchedule::Recurring {
            cron_expr: format!("0 {} {} * * {dow}", m, h),
        });
    }

    if let Some((dom, h, m)) = parse_monthly(raw, &compact) {
        return Ok(ParsedSchedule::Recurring {
            cron_expr: format!("0 {} {} {dom} * *", m, h),
        });
    }

    Err(anyhow!(
        "could not parse schedule {:?}. Examples: 30m, 2026-07-22T09:00:00, daily@9:30, every_5_minutes, or 0 30 9 * * *",
        input
    ))
}

fn try_parse_relative_once(raw: &str) -> Result<Option<ParsedSchedule>> {
    let compact: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    let bytes = compact.as_bytes();
    if bytes.is_empty() {
        return Ok(None);
    }
    let unit = *bytes.last().unwrap();
    if !matches!(unit, b'm' | b'h' | b'd') {
        return Ok(None);
    }
    let num_part = &compact[..compact.len() - 1];
    if num_part.is_empty() || !num_part.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    // Avoid treating bare "0m" etc. as once with zero delay.
    let n: i64 = num_part
        .parse()
        .map_err(|_| anyhow!("invalid relative delay: {raw}"))?;
    if n <= 0 {
        return Err(anyhow!("relative delay must be > 0: {raw}"));
    }
    let dur = match unit {
        b'm' => Duration::minutes(n),
        b'h' => Duration::hours(n),
        b'd' => Duration::days(n),
        _ => return Ok(None),
    };
    let fire = Local::now() + dur;
    Ok(Some(ParsedSchedule::Once {
        fire_at_ms: fire.timestamp_millis(),
    }))
}

fn try_parse_iso_once(raw: &str) -> Result<Option<ParsedSchedule>> {
    // Require a date-like shape so we do not steal friendly presets.
    if !raw.contains('-') || !raw.contains('T') {
        // Also allow space-separated `YYYY-MM-DD HH:MM:SS`.
        if !(raw.contains('-')
            && raw.contains(':')
            && raw.chars().filter(|c| *c == '-').count() >= 2)
        {
            return Ok(None);
        }
    }

    let fire: DateTime<Local> = if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        dt.with_timezone(&Local)
    } else if let Ok(ndt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S") {
        Local
            .from_local_datetime(&ndt)
            .single()
            .ok_or_else(|| anyhow!("ambiguous local datetime: {raw}"))?
    } else if let Ok(ndt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        Local
            .from_local_datetime(&ndt)
            .single()
            .ok_or_else(|| anyhow!("ambiguous local datetime: {raw}"))?
    } else if let Ok(ndt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M") {
        Local
            .from_local_datetime(&ndt)
            .single()
            .ok_or_else(|| anyhow!("ambiguous local datetime: {raw}"))?
    } else {
        return Ok(None);
    };

    let now = Local::now();
    if fire.timestamp_millis() <= now.timestamp_millis() {
        return Err(anyhow!("one-shot time must be in the future (got {raw})"));
    }
    Ok(Some(ParsedSchedule::Once {
        fire_at_ms: fire.timestamp_millis(),
    }))
}

fn validate_cron_expr(expr: &str) -> Result<()> {
    use crate::conversation_store::cron_jobs::next_run_ms_now;
    if next_run_ms_now(expr).is_none() {
        return Err(anyhow!("invalid cron expression: {expr}"));
    }
    Ok(())
}

fn parse_every_n(compact: &str, en_unit: &str, zh_unit: &str) -> Option<u32> {
    if let Some(rest) = compact.strip_prefix("every") {
        let rest = rest.trim_start_matches('_');
        if rest.ends_with(en_unit) {
            let num = rest.trim_end_matches(en_unit).trim_end_matches('_');
            return num.parse().ok();
        }
    }
    if compact.starts_with('每') && compact.ends_with(zh_unit) {
        let mid = compact.trim_start_matches('每').trim_end_matches(zh_unit);
        return mid.parse().ok();
    }
    None
}

fn parse_daily(raw: &str, compact: &str) -> Option<(u32, u32)> {
    if let Some(rest) = compact.strip_prefix("daily@") {
        return parse_hm(rest);
    }
    if let Some(rest) = compact.strip_prefix("dailyat") {
        return parse_hm(rest);
    }
    if compact.starts_with("每天") {
        let rest = raw.trim().trim_start_matches("每天").trim();
        return parse_hm_colon(rest);
    }
    None
}

fn parse_weekly(raw: &str, compact: &str) -> Option<(u32, u32, u32)> {
    if let Some(rest) = compact.strip_prefix("weekly@") {
        let segs: Vec<&str> = rest.split('@').collect();
        if segs.len() == 2 {
            let dow = segs[0].parse().ok()?;
            let (h, m) = parse_hm(segs[1])?;
            return Some((dow, h, m));
        }
    }
    if raw.contains('周') {
        for (label, dow) in WEEKDAY_ZH {
            if let Some(tail) = raw.split(label).nth(1) {
                let (h, m) = parse_hm_colon(tail.trim())?;
                return Some((u32::from(*dow), h, m));
            }
        }
    }
    for (label, dow) in WEEKDAY_EN {
        if let Some(rest) = compact.strip_prefix(label) {
            if let Some(tail) = rest.strip_prefix("at") {
                let (h, m) = parse_hm(tail)?;
                return Some((u32::from(*dow), h, m));
            }
        }
    }
    None
}

fn parse_monthly(raw: &str, compact: &str) -> Option<(u32, u32, u32)> {
    if let Some(rest) = compact.strip_prefix("monthly@") {
        let segs: Vec<&str> = rest.split('@').collect();
        if segs.len() == 2 {
            let dom = segs[0].parse().ok()?;
            let (h, m) = parse_hm(segs[1])?;
            return Some((dom, h, m));
        }
    }
    if compact.starts_with("每月") {
        let rest = raw.trim().trim_start_matches("每月").trim();
        if let Some(day_part) = rest.split('日').next() {
            let dom: u32 = day_part.trim().parse().ok()?;
            let time_part = rest.split('日').nth(1).unwrap_or("").trim();
            let (h, m) = parse_hm_colon(time_part)?;
            return Some((dom, h, m));
        }
    }
    None
}

fn parse_hm(s: &str) -> Option<(u32, u32)> {
    parse_hm_colon(s.trim().trim_start_matches('@'))
}

fn parse_hm_colon(s: &str) -> Option<(u32, u32)> {
    let t = s.trim().trim_start_matches('@').trim();
    if t.is_empty() {
        return None;
    }
    let t = t.replace('：', ":");
    let mut parts = t.split(':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next().unwrap_or("0").parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some((h, m))
}

fn clamp(v: u32, lo: u32, hi: u32) -> u32 {
    v.max(lo).min(hi)
}

/// Short human-readable schedule summary for a stored job.
pub fn describe_job(
    schedule_kind: &str,
    cron_expr: &str,
    schedule_raw: Option<&str>,
    next_run_at_ms: Option<i64>,
) -> String {
    if schedule_kind == "once" {
        if let Some(ms) = next_run_at_ms {
            let local: DateTime<Local> = Local
                .timestamp_millis_opt(ms)
                .single()
                .unwrap_or_else(Local::now);
            return format!("一次性 · {}", local.format("%Y-%m-%d %H:%M"));
        }
        if let Some(raw) = schedule_raw.filter(|s| !s.trim().is_empty()) {
            return format!("一次性 · 已完成（{raw}）");
        }
        return "一次性 · 已完成".into();
    }
    describe_cron_expr(cron_expr)
}

/// Short human-readable schedule summary (Chinese) for a 6-field cron expr.
pub fn describe_schedule(cron_expr: &str) -> String {
    if cron_expr.trim() == ONCE_CRON_PLACEHOLDER {
        return "一次性".into();
    }
    describe_cron_expr(cron_expr)
}

fn describe_cron_expr(cron_expr: &str) -> String {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    if parts.len() != 6 {
        return format!("自定义：{cron_expr}");
    }
    let [s, m, h, dom, mon, dow] = [parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]];
    if s != "0" {
        return format!("自定义：{cron_expr}");
    }
    let star = |x: &str| x == "*";
    if star(m) && star(h) && star(dom) && star(mon) && star(dow) {
        return "每分钟执行".into();
    }
    if let Some(n) = m.strip_prefix("*/") {
        if star(h) && star(dom) && star(mon) && star(dow) {
            return format!("每 {n} 分钟执行");
        }
    }
    if m == "0" {
        if let Some(n) = h.strip_prefix("*/") {
            if star(dom) && star(mon) && star(dow) {
                return format!("每 {n} 小时执行");
            }
        }
    }
    if let (Ok(mm), Ok(hh)) = (m.parse::<u32>(), h.parse::<u32>()) {
        if star(mon) {
            if let Ok(dd) = dom.parse::<u32>() {
                if star(dow) && !star(dom) {
                    return format!("每月 {} 日 {:02}:{:02} 执行", dd, hh, mm);
                }
            }
            if star(dom) && star(dow) {
                return format!("每天 {:02}:{:02} 执行", hh, mm);
            }
            if star(dom) {
                if let Ok(d) = dow.parse::<u32>() {
                    let labels = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
                    let label = labels.get(d as usize).unwrap_or(&"周?");
                    return format!("每{label} {:02}:{:02} 执行", hh, mm);
                }
            }
        }
    }
    format!("自定义：{cron_expr}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_every_minute() {
        assert_eq!(
            parse_schedule("every_minute").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 * * * * *".into()
            }
        );
        assert_eq!(
            parse_schedule("每分钟").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 * * * * *".into()
            }
        );
    }

    #[test]
    fn parse_every_n_minutes() {
        assert_eq!(
            parse_schedule("every_5_minutes").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 */5 * * * *".into()
            }
        );
        assert_eq!(
            parse_schedule("每10分钟").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 */10 * * * *".into()
            }
        );
    }

    #[test]
    fn parse_daily() {
        assert_eq!(
            parse_schedule("daily@9:30").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 30 9 * * *".into()
            }
        );
        assert_eq!(
            parse_schedule("每天 09:00").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 0 9 * * *".into()
            }
        );
    }

    #[test]
    fn parse_weekly() {
        assert_eq!(
            parse_schedule("weekly@1@9:30").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 30 9 * * 1".into()
            }
        );
        assert_eq!(
            parse_schedule("每周一 9:30").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 30 9 * * 1".into()
            }
        );
    }

    #[test]
    fn parse_raw_cron() {
        assert_eq!(
            parse_schedule("0 15 8 * * *").unwrap(),
            ParsedSchedule::Recurring {
                cron_expr: "0 15 8 * * *".into()
            }
        );
    }

    #[test]
    fn parse_relative_once() {
        let before = Local::now().timestamp_millis();
        match parse_schedule("30m").unwrap() {
            ParsedSchedule::Once { fire_at_ms } => {
                let after = Local::now().timestamp_millis();
                assert!(fire_at_ms >= before + 29 * 60_000);
                assert!(fire_at_ms <= after + 31 * 60_000);
            }
            other => panic!("expected Once, got {other:?}"),
        }
        assert!(matches!(
            parse_schedule("1d").unwrap(),
            ParsedSchedule::Once { .. }
        ));
        assert!(matches!(
            parse_schedule("2H").unwrap(),
            ParsedSchedule::Once { .. }
        ));
        assert!(parse_schedule("0m").is_err());
    }

    #[test]
    fn parse_iso_once_future() {
        let future = (Local::now() + Duration::hours(2))
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();
        assert!(matches!(
            parse_schedule(&future).unwrap(),
            ParsedSchedule::Once { .. }
        ));
    }

    #[test]
    fn parse_iso_once_rejects_past() {
        assert!(parse_schedule("2020-01-01T09:00:00").is_err());
    }

    #[test]
    fn describe_known_presets() {
        assert_eq!(describe_schedule("0 * * * * *"), "每分钟执行");
        assert_eq!(describe_schedule("0 30 9 * * *"), "每天 09:30 执行");
    }
}

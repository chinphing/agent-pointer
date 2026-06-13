use anyhow::Result;
use parking_lot::Mutex;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PENDING_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRecord {
    sender_id: String,
    #[serde(default)]
    issued_at: i64,
}

#[derive(Default)]
pub struct PairingStore {
    approved: Mutex<HashMap<String, HashSet<String>>>,
    pending: Mutex<HashMap<String, HashMap<String, PendingRecord>>>,
}

impl PairingStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn dir() -> Result<PathBuf> {
        let dir = pointer_core::storage::app_data_dir()?.join("channel_pairing");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    fn path(channel: &str, account_id: &str) -> Result<PathBuf> {
        Ok(Self::dir()?.join(format!("{channel}_{account_id}.json")))
    }

    fn pending_path(channel: &str, account_id: &str) -> Result<PathBuf> {
        Ok(Self::dir()?.join(format!("{channel}_{account_id}.pending.json")))
    }

    fn now_ts() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn parse_pending_file(raw: &str) -> HashMap<String, PendingRecord> {
        let value: serde_json::Value = serde_json::from_str(raw).unwrap_or(serde_json::Value::Null);
        let Some(obj) = value.as_object() else {
            return HashMap::new();
        };
        let mut out = HashMap::new();
        for (code, entry) in obj {
            if let Some(sender_id) = entry.as_str() {
                out.insert(
                    code.clone(),
                    PendingRecord {
                        sender_id: sender_id.to_string(),
                        issued_at: 0,
                    },
                );
                continue;
            }
            if let Ok(record) = serde_json::from_value::<PendingRecord>(entry.clone()) {
                if !record.sender_id.is_empty() {
                    out.insert(code.clone(), record);
                }
            }
        }
        out
    }

    fn is_expired(record: &PendingRecord, now: i64) -> bool {
        if record.issued_at <= 0 {
            return true;
        }
        now.saturating_sub(record.issued_at) > PENDING_TTL.as_secs() as i64
    }

    fn prune_pending_map(
        map: &mut HashMap<String, PendingRecord>,
        approved_senders: &HashSet<String>,
    ) -> usize {
        let now = Self::now_ts();
        let before = map.len();
        map.retain(|_, record| {
            !approved_senders.contains(&record.sender_id) && !Self::is_expired(record, now)
        });
        before.saturating_sub(map.len())
    }

    pub fn load(&self, channel: &str, account_id: &str) -> Result<()> {
        let path = Self::path(channel, account_id)?;
        if path.exists() {
            let raw = fs::read_to_string(path)?;
            let approved: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
            let key = format!("{channel}:{account_id}");
            self.approved
                .lock()
                .insert(key, approved.into_iter().collect());
        }
        self.load_pending(channel, account_id)
    }

    pub fn load_pending(&self, channel: &str, account_id: &str) -> Result<()> {
        let path = Self::pending_path(channel, account_id)?;
        if !path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&path)?;
        let mut pending = Self::parse_pending_file(&raw);
        let key = format!("{channel}:{account_id}");
        let approved = self
            .approved
            .lock()
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let removed = Self::prune_pending_map(&mut pending, &approved);
        if removed > 0 {
            log::info!(
                "pairing pending pruned channel={channel} account={account_id} removed={removed}"
            );
        }
        if pending.is_empty() {
            self.pending.lock().remove(&key);
            if path.exists() {
                let _ = fs::remove_file(path);
            }
            return Ok(());
        }
        self.pending.lock().insert(key, pending);
        Ok(())
    }

    pub fn save(&self, channel: &str, account_id: &str) -> Result<()> {
        let key = format!("{channel}:{account_id}");
        let set = self.approved.lock().get(&key).cloned().unwrap_or_default();
        let list: Vec<_> = set.into_iter().collect();
        fs::write(Self::path(channel, account_id)?, serde_json::to_string_pretty(&list)?)?;
        Ok(())
    }

    fn save_pending(&self, channel: &str, account_id: &str) -> Result<()> {
        let key = format!("{channel}:{account_id}");
        let pending = self
            .pending
            .lock()
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let path = Self::pending_path(channel, account_id)?;
        if pending.is_empty() {
            if path.exists() {
                fs::remove_file(path)?;
            }
            return Ok(());
        }
        fs::write(path, serde_json::to_string_pretty(&pending)?)?;
        Ok(())
    }

    pub fn is_allowed(
        &self,
        channel: &str,
        account_id: &str,
        sender_id: &str,
        dm_policy: &str,
        allow_from: &[String],
    ) -> PairingDecision {
        if dm_policy == "open" || allow_from.iter().any(|x| x == "*") {
            return PairingDecision::Allow;
        }
        if dm_policy == "disabled" {
            return PairingDecision::Deny;
        }
        let key = format!("{channel}:{account_id}");
        let approved = self.approved.lock();
        if let Some(set) = approved.get(&key) {
            if set.contains(sender_id) || allow_from.iter().any(|x| x == sender_id) {
                return PairingDecision::Allow;
            }
        }
        if allow_from.iter().any(|x| x == sender_id) {
            return PairingDecision::Allow;
        }
        if dm_policy == "allowlist" {
            return PairingDecision::Deny;
        }
        PairingDecision::NeedPairing
    }

    pub fn issue_code(&self, channel: &str, account_id: &str, sender_id: &str) -> String {
        let code: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();
        let key = format!("{channel}:{account_id}");
        let now = Self::now_ts();
        let approved = self.approved.lock().get(&key).cloned().unwrap_or_default();
        {
            let mut guard = self.pending.lock();
            let map = guard.entry(key).or_default();
            Self::prune_pending_map(map, &approved);
            map.retain(|_, record| record.sender_id != sender_id);
            map.insert(
                code.clone(),
                PendingRecord {
                    sender_id: sender_id.to_string(),
                    issued_at: now,
                },
            );
        }
        if let Err(e) = self.save_pending(channel, account_id) {
            log::warn!("pairing pending save failed channel={channel} account={account_id}: {e:#}");
        }
        log::info!(
            "pairing issued channel={channel} account={account_id} sender={sender_id} code={code}"
        );
        code
    }

    pub fn approve(&self, channel: &str, account_id: &str, code: &str) -> Result<bool> {
        self.load_pending(channel, account_id)?;
        let key = format!("{channel}:{account_id}");
        let normalized = code.trim();
        let matched = {
            let pending = self.pending.lock();
            pending.get(&key).and_then(|m| {
                m.iter()
                    .find(|(c, _)| c.as_str() == normalized || c.eq_ignore_ascii_case(normalized))
                    .map(|(c, r)| (c.clone(), r.sender_id.clone()))
            })
        };
        let Some((matched_code, sender)) = matched else {
            let count = self
                .pending
                .lock()
                .get(&key)
                .map(|m| m.len())
                .unwrap_or(0);
            log::warn!(
                "pairing approve miss channel={channel} account={account_id} code={normalized} pending_count={count}"
            );
            return Ok(false);
        };
        self.approved
            .lock()
            .entry(key.clone())
            .or_default()
            .insert(sender.clone());
        {
            let mut guard = self.pending.lock();
            if let Some(map) = guard.get_mut(&key) {
                map.retain(|_, record| record.sender_id != sender);
            }
        }
        self.save(channel, account_id)?;
        if let Err(e) = self.save_pending(channel, account_id) {
            log::warn!("pairing pending save failed channel={channel} account={account_id}: {e:#}");
        }
        log::info!(
            "pairing approved channel={channel} account={account_id} sender={sender} code={matched_code}"
        );
        Ok(true)
    }

    pub fn list_pending(&self, channel: &str, account_id: &str) -> Vec<(String, String, i64)> {
        let key = format!("{channel}:{account_id}");
        let approved = self.approved.lock().get(&key).cloned().unwrap_or_default();
        let mut guard = self.pending.lock();
        let map = guard.entry(key).or_default();
        let removed = Self::prune_pending_map(map, &approved);
        if removed > 0 {
            log::info!(
                "pairing pending pruned on list channel={channel} account={account_id} removed={removed}"
            );
            let _ = self.save_pending(channel, account_id);
        }
        map.iter()
            .map(|(code, record)| (code.clone(), record.sender_id.clone(), record.issued_at))
            .collect()
    }
}

pub enum PairingDecision {
    Allow,
    Deny,
    NeedPairing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_pending_without_timestamp_is_expired() {
        let record = PendingRecord {
            sender_id: "u1".into(),
            issued_at: 0,
        };
        assert!(PairingStore::is_expired(&record, PairingStore::now_ts()));
    }

    #[test]
    fn fresh_pending_is_not_expired() {
        let now = PairingStore::now_ts();
        let record = PendingRecord {
            sender_id: "u1".into(),
            issued_at: now,
        };
        assert!(!PairingStore::is_expired(&record, now));
    }
}

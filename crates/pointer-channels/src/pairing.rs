use anyhow::Result;
use parking_lot::Mutex;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
#[derive(Default)]
pub struct PairingStore {
    approved: Mutex<HashMap<String, HashSet<String>>>,
    pending: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl PairingStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn dir() -> Result<PathBuf> {
        let base = dirs::data_dir().context("data dir")?;
        let dir = base
            .join(pointer_core::storage::APP_DATA_SUBDIR)
            .join("channel_pairing");
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
        let raw = fs::read_to_string(path)?;
        let pending: HashMap<String, String> = serde_json::from_str(&raw).unwrap_or_default();
        let key = format!("{channel}:{account_id}");
        if !pending.is_empty() {
            self.pending.lock().insert(key, pending);
        }
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
        self.pending
            .lock()
            .entry(key)
            .or_default()
            .insert(code.clone(), sender_id.to_string());
        if let Err(e) = self.save_pending(channel, account_id) {
            log::warn!("pairing pending save failed channel={channel} account={account_id}: {e:#}");
        }
        log::info!(
            "pairing issued channel={channel} account={account_id} sender={sender_id} code={code}"
        );
        code
    }

    pub fn approve(&self, channel: &str, account_id: &str, code: &str) -> Result<bool> {
        let key = format!("{channel}:{account_id}");
        let normalized = code.trim();
        let matched = {
            let pending = self.pending.lock();
            pending.get(&key).and_then(|m| {
                m.iter()
                    .find(|(c, _)| c.as_str() == normalized || c.eq_ignore_ascii_case(normalized))
                    .map(|(c, s)| (c.clone(), s.clone()))
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
        self.pending
            .lock()
            .get_mut(&key)
            .map(|m| m.remove(&matched_code));
        self.save(channel, account_id)?;
        if let Err(e) = self.save_pending(channel, account_id) {
            log::warn!("pairing pending save failed channel={channel} account={account_id}: {e:#}");
        }
        log::info!(
            "pairing approved channel={channel} account={account_id} sender={sender} code={matched_code}"
        );
        Ok(true)
    }

    pub fn list_pending(&self, channel: &str, account_id: &str) -> Vec<(String, String)> {
        let key = format!("{channel}:{account_id}");
        self.pending
            .lock()
            .get(&key)
            .map(|m| m.iter().map(|(c, s)| (c.clone(), s.clone())).collect())
            .unwrap_or_default()
    }
}

use anyhow::Context;

pub enum PairingDecision {
    Allow,
    Deny,
    NeedPairing,
}

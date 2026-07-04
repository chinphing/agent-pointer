//! Thread-local `SESSION_USER_ID` for subprocess environment injection.

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Command;

thread_local! {
    static SESSION_USER_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub struct SessionUserIdGuard {
    previous: Option<String>,
}

impl SessionUserIdGuard {
    pub fn enter(user_id: String) -> Self {
        let trimmed = user_id.trim();
        let next = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        let previous = SESSION_USER_ID.with(|slot| {
            let prev = slot.borrow().clone();
            *slot.borrow_mut() = next;
            prev
        });
        Self { previous }
    }
}

impl Drop for SessionUserIdGuard {
    fn drop(&mut self) {
        SESSION_USER_ID.with(|slot| {
            *slot.borrow_mut() = self.previous.clone();
        });
    }
}

pub fn current_session_user_id() -> Option<String> {
    SESSION_USER_ID.with(|slot| slot.borrow().clone())
}

pub fn apply_session_user_id(env: &mut HashMap<String, String>) {
    if let Some(uid) = current_session_user_id() {
        if !uid.trim().is_empty() {
            env.insert("SESSION_USER_ID".into(), uid);
        }
    }
}

pub fn apply_session_user_id_to_command(cmd: &mut Command) {
    if let Some(uid) = current_session_user_id() {
        if !uid.trim().is_empty() {
            cmd.env("SESSION_USER_ID", uid);
        }
    }
}

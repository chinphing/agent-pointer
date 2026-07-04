//! Thread-local `WORKING_DIR` for subprocess environment injection.

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Command;

thread_local! {
    static SESSION_WORK_DIR: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub struct SessionWorkDirGuard {
    previous: Option<String>,
}

impl SessionWorkDirGuard {
    pub fn enter(work_dir: String) -> Self {
        let trimmed = work_dir.trim();
        let next = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        let previous = SESSION_WORK_DIR.with(|slot| {
            let prev = slot.borrow().clone();
            *slot.borrow_mut() = next;
            prev
        });
        Self { previous }
    }
}

impl Drop for SessionWorkDirGuard {
    fn drop(&mut self) {
        SESSION_WORK_DIR.with(|slot| {
            *slot.borrow_mut() = self.previous.clone();
        });
    }
}

pub fn current_session_work_dir() -> Option<String> {
    SESSION_WORK_DIR.with(|slot| slot.borrow().clone())
}

pub fn apply_session_work_dir(env: &mut HashMap<String, String>) {
    if let Some(dir) = current_session_work_dir() {
        if !dir.trim().is_empty() {
            env.insert("WORKING_DIR".into(), dir);
        }
    }
}

pub fn apply_session_work_dir_to_command(cmd: &mut Command) {
    if let Some(dir) = current_session_work_dir() {
        if !dir.trim().is_empty() {
            cmd.env("WORKING_DIR", dir);
        }
    }
}

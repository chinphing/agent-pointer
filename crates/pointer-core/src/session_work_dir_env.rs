//! Thread-local `WORKING_DIR` for subprocess environment injection.

use crate::scoped_env_slot::scoped_env_slot;

scoped_env_slot! {
    slot: SESSION_WORK_DIR,
    guard: SessionWorkDirGuard,
    current: current_session_work_dir,
    apply: apply_session_work_dir,
    apply_to_command: apply_session_work_dir_to_command,
    env_key: "WORKING_DIR",
}

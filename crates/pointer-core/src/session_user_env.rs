//! Thread-local `SESSION_USER_ID` for subprocess environment injection.

use crate::scoped_env_slot::scoped_env_slot;

scoped_env_slot! {
    slot: SESSION_USER_ID,
    guard: SessionUserIdGuard,
    current: current_session_user_id,
    apply: apply_session_user_id,
    apply_to_command: apply_session_user_id_to_command,
    env_key: "SESSION_USER_ID",
}

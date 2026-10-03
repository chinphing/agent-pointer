//! Desktop input: action executor, enigo backend, mouse paths, post-action timing.

pub mod actions;
#[cfg(feature = "computer")]
pub mod enigo;
pub mod mouse_move;
pub mod mouse_path;
pub mod timing;

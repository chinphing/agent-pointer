//! 生成"线程局部字符串槽 + RAII guard + 子进程环境变量注入"三件套的宏。
//!
//! `SESSION_USER_ID`、`WORKING_DIR`、`POINTER_ACCESS_HOST` 三者的结构完全一致，
//! 只有类型名、函数名和键名不同。第三个槽位出现时，与其再复制一份 50 行样板，
//! 不如用一个宏生成，改一处即三处同步。
//!
//! 为什么用 thread-local 而不是顺着调用链传参：工具调用发生在 `tokio::spawn`
//! 出来的任务里，线程随时可能切换，thread-local 不会自动跟随。所以每个重新
//! 进入的工具分发点都必须重放 guard —— 见 `agent_tool_pass/dispatch/registry.rs`
//! 与 `dispatch/terminal.rs` 里的注释。
//!
//! 生成的公开 API 与手写版本逐字一致，调用方无需改动。

macro_rules! scoped_env_slot {
    (
        $(#[$meta:meta])*
        slot: $slot:ident,
        guard: $guard:ident,
        current: $current:ident,
        apply: $apply:ident,
        apply_to_command: $apply_cmd:ident,
        env_key: $env_key:literal,
    ) => {
        thread_local! {
            static $slot: ::std::cell::RefCell<Option<String>> =
                const { ::std::cell::RefCell::new(None) };
        }

        $(#[$meta])*
        pub struct $guard {
            previous: Option<String>,
        }

        impl $guard {
            /// 进入作用域。空字符串视为"没有值"，避免注入空的 `env_key`。
            pub fn enter(value: String) -> Self {
                let trimmed = value.trim();
                let next = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                let previous = $slot.with(|slot| {
                    let prev = slot.borrow().clone();
                    *slot.borrow_mut() = next;
                    prev
                });
                Self { previous }
            }
        }

        impl Drop for $guard {
            fn drop(&mut self) {
                $slot.with(|slot| {
                    *slot.borrow_mut() = self.previous.clone();
                });
            }
        }

        pub fn $current() -> Option<String> {
            $slot.with(|slot| slot.borrow().clone())
        }

        pub fn $apply(env: &mut ::std::collections::HashMap<String, String>) {
            if let Some(value) = $current() {
                if !value.trim().is_empty() {
                    env.insert($env_key.into(), value);
                }
            }
        }

        pub fn $apply_cmd(cmd: &mut ::std::process::Command) {
            if let Some(value) = $current() {
                if !value.trim().is_empty() {
                    cmd.env($env_key, value);
                }
            }
        }
    };
}

pub(crate) use scoped_env_slot;

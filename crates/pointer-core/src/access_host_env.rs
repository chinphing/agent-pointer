//! Thread-local `POINTER_ACCESS_HOST` for subprocess environment injection.
//!
//! 记录**本次请求**访问进来的域名（如 `pointer.hospital.internal`）。医院内网
//! 与外网共用一套服务、两个域名，技能脚本据此选择对应的财务系统链接。
//!
//! 之所以按请求而不是按会话记录：会话 cookie 有 30 天有效期，用户带着笔记本
//! 换网络后旧值就错了。这里每次请求都是真值。
//!
//! 没有 HTTP 请求的运行（定时任务、Webhook、IM 消息）不会设置这个变量，
//! 脚本读到空值时应自行回退，不要猜。

use crate::scoped_env_slot::scoped_env_slot;

scoped_env_slot! {
    slot: ACCESS_HOST,
    guard: AccessHostGuard,
    current: current_access_host,
    apply: apply_access_host,
    apply_to_command: apply_access_host_to_command,
    env_key: "POINTER_ACCESS_HOST",
}

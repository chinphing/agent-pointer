//! Pointer 企业控制面（管理端）领域层。
//!
//! 设计见 `docs/zh-CN/design/enterprise-admin-console.md`。
//! 本 crate 只做数据与规则，不含 HTTP；服务进程与 `http/`、`oauth/` 在后续提交里加。

pub mod crypto;
pub mod db;
pub mod grants;
pub mod password;
pub mod rbac;
pub mod resources;
pub mod schema;
pub mod sessions;
pub mod users;

pub use db::AdminStore;

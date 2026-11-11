//! Pointer 企业控制面（管理端）。
//!
//! 独立进程：server 与桌面客户端都是它的 OAuth 客户端，凭证由它统一下发。
//! 不依赖 `pointer-core`，只依赖 `pointer-contract`（纯类型 + 哈希）。

pub mod app;
pub mod audit;
pub mod billing;
pub mod captcha;
pub mod config;
pub mod credentials;
pub mod crypto;
pub mod db;
pub mod ext;
pub mod grants;
pub mod home;
pub mod http;
pub mod legal;
pub mod oauth;
pub mod password;
pub mod rbac;
pub mod releases;
pub mod resources;
pub mod schema;
pub mod seed;
pub mod sessions;
pub mod state;
pub mod usage;
pub mod users;
pub mod versions;

#[cfg(test)]
pub(crate) mod test_support;

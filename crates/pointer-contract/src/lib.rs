//! Pointer 控制面契约。
//!
//! admin（控制面）、pointer-server、桌面客户端三端共用的**数据结构与纯函数**。
//! 这里不依赖任何内部 crate、不做 IO——只保证「同一个 JSON 形状只定义一次」。
//!
//! 为什么单独一个 crate：`PlatformLoginCredentials` 这类形状如果三端各写一份，
//! 字段名一漂移就是「客户端永远不刷新凭证」这类难查的问题；放这里由编译器兜底。

pub mod credentials;
pub mod hash;
pub mod provider;
pub mod session;

pub use credentials::{
    credentials_have_llm_keys, PlatformLoginCredentials, PlatformMediaOssCredentials,
};
pub use hash::{key_fingerprint, model_catalog_hash};
pub use provider::{PlatformProviderModel, PlatformProviderTemplate};
pub use session::{PlatformSession, PlatformUserSummary};

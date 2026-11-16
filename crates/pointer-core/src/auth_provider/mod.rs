//! Third-party login providers: configuration model, registry, and drivers.
//!
//! Providers are declared in `pointer-server.toml` and consumed through
//! [`AuthProviderRegistry`]. A DingTalk entry looks like this:
//!
//! ```toml
//! [auth.providers.dingtalk]
//! kind = "oauth2_userinfo"
//! display_name = "钉钉"
//! issuer = "https://login.dingtalk.com"
//!
//! [auth.providers.dingtalk.token]
//! url = "https://api.dingtalk.com/v1.0/oauth2/userAccessToken"
//! client_id = "..."
//! client_secret = "..."
//! encoding = "json"
//! grant_type_param = "grantType"
//! client_id_param = "clientId"
//! client_secret_param = "clientSecret"
//! access_token_path = "/accessToken"
//!
//! [auth.providers.dingtalk.userinfo]
//! url = "https://api.dingtalk.com/v1.0/contact/users/me"
//! token_auth = "header"
//! token_name = "x-acs-dingtalk-access-token"
//! token_prefix = ""
//! subject_path = "/unionId"
//! ```
//!
//! Drivers only turn an authorization code into an [`Identity`]. They never
//! touch cookies, web sessions, or the access allowlist — that stays in the
//! server layer.
//!
//! Everything configurable is a closed enum or a restricted JSON pointer
//! (`/a/b/0` with `[A-Za-z0-9_.-]` segments). There is no expression engine and
//! no scripting, so a config typo can never execute code.

pub mod config;
mod identity;
mod json_path;
pub mod local_password;
pub mod oauth2_userinfo;
pub mod oidc;
mod query;
pub mod registry;
pub mod token_lookup;

pub use config::{
    default_callback_path, AuthProviderConfig, AuthProviderKind, AuthorizeEndpointConfig,
    ClientAuth, ErrorRule, HttpMethod, OidcConfig, ParamEncoding, TicketSigningConfig, TokenAuth,
    TokenEndpointConfig, TokenLookupConfig, TokenLookupEndpoint, UserinfoEndpointConfig,
    PLATFORM_CALLBACK_PATH,
};
pub use identity::{AuthCodeRequest, AuthProviderDriver, AuthorizeRequest, Identity, PasswordCredentials};
pub use local_password::LocalPasswordDriver;
pub use oauth2_userinfo::{Oauth2UserinfoDriver, TokenSet};
pub use oidc::OidcDriver;
pub use registry::{
    AuthProviderRegistry, LoginEntry, ProviderView, LOCAL_PASSWORD_PROVIDER_ID,
    PLATFORM_PROVIDER_ID,
};
pub use token_lookup::{mint_signed_ticket, TokenLookupDriver};

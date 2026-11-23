//! 会话环境。线程槽里只能放这个类型。
//!
//! 整机文件视图（`LocalEnv::new`、`host_env`）实现的是 `Environment`，
//! 不能放进槽。系统账本不经过这里。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};

use super::{Environment, LocalEnv};

/// 这份会话属于哪个用户树。容器里的 `/home/pointer` 与 `/data` 每个用户都相同，
/// 区分用户看 `id` 和宿主上的两棵目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionScope {
    pub id: String,
    pub home_host: PathBuf,
    pub data_host: PathBuf,
}

impl SessionScope {
    pub fn id_for(session_user_id: &str, conversation_id: &str) -> Result<String> {
        let uid = session_user_id.trim();
        if uid.is_empty() {
            let conv = conversation_id.trim();
            if conv.is_empty() {
                bail!("匿名会话缺少会话 id，不能绑定执行环境");
            }
            return Ok(format!("c:{conv}"));
        }
        Ok(format!("u:{}", crate::user_key::user_key(uid)))
    }
}

/// 已绑定到某个用户树的执行环境。
#[derive(Clone)]
pub struct SessionEnv {
    inner: Arc<dyn Environment>,
    scope: SessionScope,
}

impl SessionEnv {
    /// 只接受带路径表、且 `data_root` 非空的环境。恒等映射和未绑定占位会被拒绝。
    pub fn try_new(inner: Arc<dyn Environment>, scope: SessionScope) -> Result<Self> {
        if inner.is_denied() {
            bail!("会话环境未绑定");
        }
        if inner.path_map().is_host_passthrough() {
            bail!("会话环境不能使用整机文件视图");
        }
        if inner.data_root().as_os_str().is_empty() {
            bail!("会话环境缺少 data 根");
        }
        if scope.id.trim().is_empty() {
            bail!("会话环境缺少用户范围");
        }
        log::info!(
            "session env: scope={} kind={:?} home={} data={}",
            scope.id,
            inner.kind(),
            inner.home_root().display(),
            inner.data_root().display()
        );
        Ok(Self { inner, scope })
    }

    /// 本地档位：创建用户目录并只看见这两棵树。
    pub fn open_local(session_user_id: &str, conversation_id: &str) -> Result<Self> {
        let layout = crate::user_data_paths::ensure_user_layout(session_user_id, conversation_id)?;
        Self::open_local_layout(
            session_user_id,
            conversation_id,
            &layout,
            &layout.data.join("workspaces"),
        )
    }

    pub fn open_local_layout(
        session_user_id: &str,
        conversation_id: &str,
        layout: &crate::user_data_paths::UserLayout,
        workspace: &Path,
    ) -> Result<Self> {
        if let Err(error) = std::fs::create_dir_all(workspace) {
            bail!("create workspace {}: {error}", workspace.display());
        }
        let local = Arc::new(LocalEnv::for_user(
            layout.home.clone(),
            layout.data.clone(),
            workspace,
        ));
        let scope = SessionScope {
            id: SessionScope::id_for(session_user_id, conversation_id)?,
            home_host: layout.home.clone(),
            data_host: layout.data.clone(),
        };
        Self::try_new(local, scope)
    }

    /// 测试或已经备好两棵目录时使用。`home` 与 `data` 就是可见范围。
    pub fn open_roots(
        scope_id: &str,
        home: impl Into<PathBuf>,
        data: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
    ) -> Result<Self> {
        let home = home.into();
        let data = data.into();
        let workspace = workspace.into();
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&data)?;
        std::fs::create_dir_all(&workspace)?;
        Self::open_with(scope_id, home, data, workspace, None)
    }

    pub fn open_with(
        scope_id: &str,
        home: impl Into<PathBuf>,
        data: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
        capabilities: Option<super::Capabilities>,
    ) -> Result<Self> {
        let home = home.into();
        let data = data.into();
        let workspace = workspace.into();
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&data)?;
        std::fs::create_dir_all(&workspace)?;
        let mut local = LocalEnv::for_user(home.clone(), data.clone(), workspace);
        if let Some(capabilities) = capabilities {
            local = local.with_capabilities(capabilities);
        }
        Self::try_new(
            Arc::new(local),
            SessionScope {
                id: scope_id.to_string(),
                home_host: home,
                data_host: data,
            },
        )
    }

    /// 容器或远端环境已经带好路径表时包一层。宿主目录从挂载表取出。
    pub fn wrap(
        session_user_id: &str,
        conversation_id: &str,
        inner: Arc<dyn Environment>,
    ) -> Result<Self> {
        let home_guest = inner.home_root().to_path_buf();
        let data_guest = inner.data_root().to_path_buf();
        let home_host = host_backing(inner.as_ref(), &home_guest);
        let data_host = host_backing(inner.as_ref(), &data_guest);
        Self::try_new(
            inner,
            SessionScope {
                id: SessionScope::id_for(session_user_id, conversation_id)?,
                home_host,
                data_host,
            },
        )
    }

    pub fn environment(&self) -> Arc<dyn Environment> {
        self.inner.clone()
    }

    pub fn scope(&self) -> &SessionScope {
        &self.scope
    }

    pub fn same_scope(&self, other: &SessionEnv) -> bool {
        self.scope.id == other.scope.id
    }
}

impl std::ops::Deref for SessionEnv {
    type Target = dyn Environment;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

fn host_backing(env: &dyn Environment, guest: &Path) -> PathBuf {
    env.path_map()
        .mounts()
        .iter()
        .find(|mount| mount.guest.as_path() == guest)
        .map(|mount| mount.host.clone())
        .unwrap_or_else(|| guest.to_path_buf())
}

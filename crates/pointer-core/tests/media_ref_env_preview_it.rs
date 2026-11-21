//! 媒体引用（`MEDIA:` / 附件 ref）在**会话环境**里的预览 / 下载解析。
//!
//! 沙箱档位下引用是环境内路径（`/workspace/x.png`），宿主上没有这个文件：
//! 不带会话环境时必须维持原来的宿主错误（不 fail-closed、不静默回落），
//! 带环境时按环境读——字节来源仍由 `workspace_read::media_body_for_streaming`
//! 决定（宿主挂载 → 宿主流式；远端后端 → 整段读）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use base64::Engine;

use pointer_core::env::{
    Capabilities, ChildHandle, DirEntry, EnvKind, Environment, ExecOptions, ExecOutput,
    FileMetadata, Mount, PathMap, SpawnOptions, WalkEntry, WalkOptions,
};
use pointer_core::media::{
    media_ref_body_in_env, read_media_ref_preview_in_env, MEDIA_REF_ENV_DOWNLOAD_MAX_BYTES,
};
use pointer_core::workspace_read::WorkspaceMediaBody;

/// 沙箱风格环境：路径是环境内路径（`/workspace/...`），IO 走内存假文件系统。
///
/// `mounts` 由用例决定：容器后端有「guest `/workspace` → 宿主目录」的挂载表，
/// 云端 envd 的挂载点只是模板（宿主上没有文件）。
struct SandboxEnv {
    workspace: PathBuf,
    map: PathMap,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl SandboxEnv {
    fn new(mounts: Vec<Mount>) -> Self {
        Self {
            workspace: PathBuf::from("/workspace"),
            map: PathMap::new(mounts),
            files: Mutex::new(BTreeMap::new()),
        }
    }

    fn put(&self, guest: &str, bytes: &[u8]) {
        self.files
            .lock()
            .expect("files")
            .insert(guest.to_string(), bytes.to_vec());
    }

    fn lookup(&self, path: &Path) -> Option<Vec<u8>> {
        self.files
            .lock()
            .expect("files")
            .get(&path.to_string_lossy().into_owned())
            .cloned()
    }
}

impl Environment for SandboxEnv {
    fn kind(&self) -> EnvKind {
        EnvKind::Headless
    }
    fn workspace_root(&self) -> &Path {
        &self.workspace
    }
    fn skills_root(&self) -> &Path {
        Path::new("/skills")
    }
    fn home_root(&self) -> &Path {
        Path::new("/home/pointer")
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::headless()
    }
    fn path_map(&self) -> &PathMap {
        &self.map
    }
    fn exec(&self, _argv: &[String], _opts: ExecOptions) -> Result<ExecOutput> {
        Err(anyhow!("sandbox env: exec not supported"))
    }
    fn spawn(&self, _argv: &[String], _opts: SpawnOptions) -> Result<Box<dyn ChildHandle>> {
        Err(anyhow!("sandbox env: spawn not supported"))
    }
    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.lookup(path)
            .ok_or_else(|| anyhow!("sandbox env: not found: {}", path.display()))
    }
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        self.put(&path.to_string_lossy(), data);
        Ok(())
    }
    fn list_dir(&self, _path: &Path) -> Result<Vec<DirEntry>> {
        Ok(Vec::new())
    }
    fn metadata(&self, path: &Path) -> Result<FileMetadata> {
        let bytes = self
            .lookup(path)
            .ok_or_else(|| anyhow!("sandbox env: not found: {}", path.display()))?;
        Ok(FileMetadata {
            is_file: true,
            is_dir: false,
            is_symlink: false,
            len: bytes.len() as u64,
        })
    }
    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        Ok(path.to_path_buf())
    }
    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
    fn read_to_string(&self, path: &Path) -> Result<String> {
        Ok(String::from_utf8(self.read_file(path)?)?)
    }
    fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
        Ok(Box::new(std::io::Cursor::new(self.read_file(path)?)))
    }
    fn walk(&self, _root: &Path, _opts: WalkOptions) -> Result<Vec<WalkEntry>> {
        Ok(Vec::new())
    }
    fn remove_file(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
    fn remove_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
    fn set_executable(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
}

const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nfake-png";

fn decode(preview: &pointer_core::models::ChatMediaPreview) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(preview.data_base64.trim())
        .expect("decode preview base64")
}

#[test]
fn sandbox_media_ref_preview_reads_from_session_env() {
    // 云端 envd：宿主上没有 `/workspace/pic.png`，只有环境里有。
    let env = SandboxEnv::new(Vec::new());
    env.put("/workspace/pic.png", PNG_BYTES);

    let preview =
        read_media_ref_preview_in_env("/workspace/pic.png", Some(&env)).expect("env preview");

    assert_eq!(decode(&preview), PNG_BYTES);
    assert_eq!(preview.mime_type, "image/png");
    assert_eq!(preview.file_name, "pic.png");
}

#[test]
fn sandbox_media_ref_preview_strips_pointer_media_scheme() {
    let env = SandboxEnv::new(Vec::new());
    env.put("/workspace/pic.png", PNG_BYTES);

    let preview = read_media_ref_preview_in_env("pointer-media:///workspace/pic.png", Some(&env))
        .expect("env preview with scheme");

    assert_eq!(decode(&preview), PNG_BYTES);
}

#[test]
fn sandbox_media_ref_body_uses_host_mount_when_present() {
    // 容器后端：`/workspace` 是宿主目录的 bind mount → 宿主流式，不进内存。
    let host_ws = tempfile::tempdir().expect("temp workspace");
    let host_file = host_ws.path().join("pic.png");
    std::fs::write(&host_file, PNG_BYTES).expect("write host file");
    let env = SandboxEnv::new(vec![Mount::new(host_ws.path(), "/workspace", false)]);
    env.put("/workspace/pic.png", PNG_BYTES);

    let (body, mime, name) = media_ref_body_in_env(
        "/workspace/pic.png",
        Some(&env),
        MEDIA_REF_ENV_DOWNLOAD_MAX_BYTES,
    )
    .expect("env body");

    match body {
        WorkspaceMediaBody::HostPath(path) => {
            assert_eq!(
                path.canonicalize().expect("canonical host path"),
                host_file.canonicalize().expect("canonical file"),
                "映射成功就走宿主流式"
            );
        }
        WorkspaceMediaBody::Inline(_) => panic!("有宿主挂载时不该整段读进内存"),
    }
    assert_eq!(mime, "image/png");
    assert_eq!(name, "pic.png");
}

#[test]
fn sandbox_media_ref_body_reads_env_bytes_when_host_has_no_file() {
    let host_ws = tempfile::tempdir().expect("temp workspace");
    let env = SandboxEnv::new(vec![Mount::new(host_ws.path(), "/workspace", false)]);
    env.put("/workspace/pic.png", PNG_BYTES);

    let (body, _, _) = media_ref_body_in_env(
        "/workspace/pic.png",
        Some(&env),
        MEDIA_REF_ENV_DOWNLOAD_MAX_BYTES,
    )
    .expect("env body");

    assert_eq!(body, WorkspaceMediaBody::Inline(PNG_BYTES.to_vec()));
}

#[test]
fn media_ref_without_env_keeps_host_error() {
    // 没有会话环境（缺 conversationId / 环境没建）：维持宿主读取，报宿主错误。
    let err = read_media_ref_preview_in_env("/workspace/pic.png", None)
        .expect_err("host has no such file");
    assert!(
        err.to_string().contains("media file not found"),
        "unexpected error: {err:#}"
    );

    let err = media_ref_body_in_env("/workspace/pic.png", None, MEDIA_REF_ENV_DOWNLOAD_MAX_BYTES)
        .expect_err("host has no such file");
    assert!(
        err.to_string().contains("media file not found"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn relative_media_ref_is_not_resolved_inside_env() {
    // 存储 rel 路径是应用数据里的附件：宿主解析不到就是真没有，不拿到环境里猜。
    let env = SandboxEnv::new(Vec::new());
    env.put("conv/pic.png", PNG_BYTES);

    let err = read_media_ref_preview_in_env("conv/pic.png", Some(&env))
        .expect_err("storage rel must not be env-resolved");
    assert!(
        err.to_string().contains("media file not found"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn host_media_ref_still_previews_with_env() {
    // 宿主路径（附件本体 / local 档位工作区）优先，行为与以前一致。
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("shot.png");
    std::fs::write(&file, PNG_BYTES).expect("write file");
    let env = SandboxEnv::new(Vec::new());

    let preview =
        read_media_ref_preview_in_env(&file.to_string_lossy(), Some(&env)).expect("host preview");
    assert_eq!(decode(&preview), PNG_BYTES);
    assert_eq!(preview.file_name, "shot.png");
}

#[test]
fn reference_image_resolves_sandbox_workspace_path() {
    // `image_generate` / `video_generate` 的参考图可以是工作区路径。
    let env = SandboxEnv::new(Vec::new());
    env.put("/workspace/ref.png", PNG_BYTES);

    let out = pointer_core::media_generation::resolve_reference_image_for_api_in_env(
        "/workspace/ref.png",
        Some(&env),
    )
    .expect("env reference image");

    assert!(out.starts_with("data:image/png;base64,"), "got {out}");
    let encoded = out.trim_start_matches("data:image/png;base64,");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("decode data url"),
        PNG_BYTES
    );
}

#[test]
fn reference_image_passes_through_http_without_env() {
    let url = "https://example.com/a.png";
    assert_eq!(
        pointer_core::media_generation::resolve_reference_image_for_api_in_env(url, None)
            .expect("http passthrough"),
        url
    );
}

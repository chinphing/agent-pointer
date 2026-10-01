# App Auto-Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable Pointer desktop (Tauri 2) to silently check and download updates in the background, then prompt the user to restart — fully backed by the official website release system (`pointer-official` admin console).

**Architecture:** `tauri-plugin-updater` calls `GET /api/updates/latest` on the **same API host** as OAuth and `/download` (`platform_endpoints::api_base()`). Update binaries are served via existing `GET /api/downloads/files/{id}`. One admin **Publish** action activates both the website download page and in-app auto-update. Build artifacts are uploaded to the admin console after local builds.

**Tech Stack:** Tauri 2, `tauri-plugin-updater`, Vue 3, FastAPI, SQLAlchemy

**Design spec:** `pointer-app/docs/design/app-auto-update.md`

**Official release docs:** `pointer-official/docs/app-releases-deployment.md`

---

## File Map

| Area | Create | Modify |
|------|--------|--------|
| Keys & build | — | `pointer-app/src-tauri/tauri.conf.json`, `pointer-app/crates/pointer-core/build.rs` (inject API base), `pointer-app/docs/contributing/cross-platform-build.md` |
| Server model | migration in `pointer-official/apps/api/app/db.py` | `models.py`, `schemas.py`, `services/app_releases.py`, new `routers/updates.py` |
| Server admin | — | `routers/admin_app_releases.py`, `AppReleaseAdminPanel.tsx` |
| Server docs | — | `pointer-official/docs/app-releases-deployment.md` |
| Client Rust | `src-tauri/src/updater_commands.rs` | `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json` |
| Client Vue | `src/composables/useAppUpdater.ts`, `src/components/updater/UpdateReadyBanner.vue`, `src/components/settings/panels/AboutSettingsPanel.vue` | `SettingsDialog.vue`, `App.vue`, `package.json` |
| Client docs | — | `pointer-app/docs/design/app-auto-update.md` |

---

### Task 1: Generate updater signing keys and document build setup

**Files:**
- Modify: `pointer-app/docs/contributing/cross-platform-build.md`
- Modify: `pointer-official/docs/app-releases-deployment.md` (cross-link auto-update)

- [ ] **Step 1: Generate keypair**

Run on a secure machine:

```bash
cd agent-pointer
npm run tauri signer generate -w ~/.tauri/pointer-updater.key
```

Expected: prints public key content and writes private key file.

- [ ] **Step 2: Store private key securely**

Save private key in team password manager. For local builds, use env vars (never commit):

```bash
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/pointer-updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
```

- [ ] **Step 3: Document in cross-platform-build.md**

Add section **「Updater 签名密钥」** covering generation, backup, build env vars, and that published updates always go through the official admin console.

- [ ] **Step 4: Commit docs only**

```bash
git add pointer-app/docs/contributing/cross-platform-build.md pointer-official/docs/app-releases-deployment.md
git commit -m "docs: updater signing and official release integration"
```

---

### Task 2: Enable updater artifacts in Tauri config

**Files:**
- Modify: `pointer-app/src-tauri/tauri.conf.json`
- Modify: `pointer-app/package.json` (if `@tauri-apps/plugin-updater` needed)

- [ ] **Step 1: Add updater plugin via CLI**

```bash
cd agent-pointer
npm run tauri add updater
```

Expected: adds `tauri-plugin-updater` to `src-tauri/Cargo.toml` and `@tauri-apps/plugin-updater` to `package.json`.

- [ ] **Step 2: Update tauri.conf.json**

Set `bundle.createUpdaterArtifacts` to `true`. Add `nsis` to `bundle.targets` (keep existing targets). Add `plugins.updater` block — **endpoint must use the official API base**, same as `platform_endpoints::DEFAULT_API_BASE`:

```json
{
  "bundle": {
    "createUpdaterArtifacts": true,
    "targets": ["msi", "nsis", "deb", "appimage", "dmg"]
  },
  "plugins": {
    "updater": {
      "pubkey": "REPLACE_WITH_PUBLIC_KEY",
      "endpoints": [
        "https://pointer-api.readflowai.com/api/updates/latest?target={{target}}&arch={{arch}}&current_version={{current_version}}"
      ],
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

Replace `pubkey` with actual public key from Task 1.

Optional: inject endpoint host from `POINTER_API_BASE` in `build.rs` so dev/staging builds match OAuth domain.

- [ ] **Step 3: Skip updater on standalone builds**

In `lib.rs` plugin setup, only register updater when `!deployment_mode::is_standalone()` and `api_base()` is non-empty.

- [ ] **Step 4: Verify local build produces updater artifacts**

```bash
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/pointer-updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
npm run build:macos   # or one platform you can build
```

Expected under `src-tauri/target/release/bundle/`:

- macOS: `macos/*.app.tar.gz` + `*.sig`
- Windows: `msi/*.msi` + `*.sig`

- [ ] **Step 5: Commit**

```bash
git add pointer-app/src-tauri/tauri.conf.json pointer-app/src-tauri/Cargo.toml pointer-app/package.json pointer-app/package-lock.json pointer-app/src-tauri/src/lib.rs
git commit -m "feat: enable Tauri updater pointed at official API"
```

---

### Task 3: DB migration — signature column and new artifact kinds

**Files:**
- Modify: `pointer-official/apps/api/app/models.py`
- Modify: `pointer-official/apps/api/app/db.py`

- [ ] **Step 1: Extend AppReleaseArtifactKind enum**

In `models.py`, add:

```python
class AppReleaseArtifactKind(str, enum.Enum):
    windows = "windows"
    macos = "macos"
    macos_arm = "macos_arm"
    macos_intel = "macos_intel"
    linux_deb = "linux_deb"
    linux_appimage = "linux_appimage"
    windows_updater = "windows_updater"
    macos_updater = "macos_updater"
    linux_appimage_updater = "linux_appimage_updater"
```

- [ ] **Step 2: Add signature column to AppReleaseArtifact**

```python
signature: Mapped[str | None] = mapped_column(Text, nullable=True)
```

- [ ] **Step 3: Add schema migration helper in db.py**

Follow existing `_ensure_app_release_artifacts_schema` pattern:

```python
async def _ensure_app_release_artifact_signature_column(conn: AsyncConnection, dialect_name: str) -> None:
    # Add signature TEXT NULL if not exists (MySQL + PostgreSQL variants)
    ...
```

Call from both mysql and postgresql init paths.

- [ ] **Step 4: Run API locally and confirm migration**

```bash
cd pointer-official/apps/api
# start API or run tests — confirm no startup error
```

- [ ] **Step 5: Commit**

```bash
git add pointer-official/apps/api/app/models.py pointer-official/apps/api/app/db.py
git commit -m "feat(api): add updater artifact kinds and signature column"
```

---

### Task 4: Extend app_releases service for updater kinds

**Files:**
- Modify: `pointer-official/apps/api/app/services/app_releases.py`
- Modify: `pointer-official/apps/api/app/schemas.py`

- [ ] **Step 1: Add KIND_EXTENSIONS for updater kinds**

```python
AppReleaseArtifactKind.windows_updater: {".msi"},
AppReleaseArtifactKind.macos_updater: {".tar.gz"},
AppReleaseArtifactKind.linux_appimage_updater: {".appimage"},
```

- [ ] **Step 2: Add KIND_META labels (admin UI)**

```python
AppReleaseArtifactKind.windows_updater: {
    "label": "Windows Updater",
    "description": "MSI + 签名，供客户端自动升级",
    "format": ".msi",
},
# ... macos_updater, linux_appimage_updater
```

- [ ] **Step 3: Extend upsert_artifact to accept optional signature**

Add parameter `signature: str | None = None` and persist to row.

- [ ] **Step 4: Add build_tauri_update_manifest()**

New function returning Tauri JSON or `None` when no update needed:

```python
def build_tauri_update_manifest(
    *,
    version: str,
    notes: str | None,
    published_at: datetime | None,
    artifacts: list[AppReleaseArtifact],
    api_base: str,
    current_version: str | None,
) -> dict | None:
    if current_version and not semver_is_newer(version, current_version):
        return None
    platforms: dict[str, dict[str, str]] = {}
    # url MUST be official API download route — same as /download page
    # f"{api_base}/api/downloads/files/{artifact.id}"
    if not platforms:
        return None
    return {
        "version": version,
        "notes": notes or "",
        "pub_date": published_at.isoformat() if published_at else None,
        "platforms": platforms,
    }
```

Add small `semver_is_newer(latest: str, current: str) -> bool` using `packaging.version` or manual split — match Tauri semver rules.

- [ ] **Step 5: Extend AppReleaseArtifactOut schema**

Add `signature: str | None = None`.

- [ ] **Step 6: Commit**

```bash
git add pointer-official/apps/api/app/services/app_releases.py pointer-official/apps/api/app/schemas.py
git commit -m "feat(api): support updater artifact kinds and manifest builder"
```

---

### Task 5: Add GET /api/updates/latest endpoint

**Files:**
- Create: `pointer-official/apps/api/app/routers/updates.py`
- Modify: `pointer-official/apps/api/app/main.py` (register router)

- [ ] **Step 1: Create router**

```python
import logging
from fastapi import APIRouter, Depends, Query, Response
from fastapi.responses import JSONResponse
from sqlalchemy.ext.asyncio import AsyncSession

from app.db import get_session
from app.config import Settings, get_settings
from app.services.app_releases import build_tauri_update_manifest, get_published_release
from app.services.public_urls import public_base_url

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/api/updates", tags=["updates"])

@router.get("/latest")
async def get_latest_update(
    request: Request,
    response: Response,
    target: str = Query(..., pattern="^(darwin|windows|linux)$"),
    arch: str = Query(..., pattern="^(x86_64|aarch64|i686|armv7)$"),
    current_version: str | None = Query(None),
    session: AsyncSession = Depends(get_session),
    settings: Settings = Depends(get_settings),
):
    version, published_at, artifacts = await get_published_release(session)
    if not version:
        response.status_code = 204
        return None

    payload = build_tauri_update_manifest(
        version=version,
        notes=None,  # Task 9 if release notes added
        published_at=published_at,
        artifacts=artifacts,
        api_base=public_base_url(request, settings),
        current_version=current_version,
    )
    if payload is None:
        response.status_code = 204
        return None

    logger.info(
        "Update manifest served version=%s target=%s arch=%s current=%s",
        version, target, arch, current_version,
    )
    return JSONResponse(payload)
```

Note: Tauri expects full `platforms` map in static JSON; returning all platforms in one response is fine even if query params are present.

- [ ] **Step 2: Register router in main.py**

```python
from app.routers import updates
app.include_router(updates.router)
```

- [ ] **Step 3: Write API test**

Create `pointer-official/apps/api/tests/test_updates.py`:

```python
@pytest.mark.asyncio
async def test_updates_latest_no_published(client):
    r = await client.get("/api/updates/latest", params={"target": "darwin", "arch": "aarch64"})
    assert r.status_code == 204

@pytest.mark.asyncio
async def test_updates_latest_returns_manifest_when_newer(client, published_updater_artifacts):
    r = await client.get(
        "/api/updates/latest",
        params={"target": "darwin", "arch": "aarch64", "current_version": "0.1.0"},
    )
    assert r.status_code == 200
    body = r.json()
    assert body["version"] == "0.1.1"
    assert "darwin-aarch64" in body["platforms"]
    assert body["platforms"]["darwin-aarch64"]["signature"]
```

- [ ] **Step 4: Run tests**

```bash
cd pointer-official/apps/api
pytest tests/test_updates.py -v
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add pointer-official/apps/api/app/routers/updates.py pointer-official/apps/api/app/main.py pointer-official/apps/api/tests/test_updates.py
git commit -m "feat(api): add GET /api/updates/latest for Tauri updater"
```

---

### Task 6: Admin upload — signature field and updater slots

**Files:**
- Modify: `pointer-official/apps/api/app/routers/admin_app_releases.py`
- Modify: `pointer-official/apps/web/src/components/AppReleaseAdminPanel.tsx`

- [ ] **Step 1: Extend upload endpoint**

Accept optional form field `signature: str` on `POST /admin/app-releases/upload`. Pass to `upsert_artifact`.

Optional: accept companion `.sig` file upload; read text and store.

- [ ] **Step 2: Add admin upload slots**

In `AppReleaseAdminPanel.tsx`, extend `UPLOAD_SLOTS`:

```typescript
{ id: "windows_updater", label: "Windows · Updater（.msi + 签名）" },
{ id: "macos_updater", label: "macOS · Updater（.app.tar.gz + 签名）" },
{ id: "linux_appimage_updater", label: "Linux · AppImage Updater" },
```

Add textarea or second file input for `.sig` content per updater slot.

- [ ] **Step 3: Manual smoke test via official admin**

Upload test artifacts through **官网控制台 → 客户端发布**; confirm `signature` persisted. Publish and verify:

```bash
curl -s "https://<official-api>/api/updates/latest?target=darwin&arch=aarch64&current_version=0.0.1"
curl -s "https://<official-api>/api/downloads"
```

Both must reflect the same published version.

- [ ] **Step 4: Commit**

```bash
git add pointer-official/apps/api/app/routers/admin_app_releases.py pointer-official/apps/web/src/components/AppReleaseAdminPanel.tsx
git commit -m "feat: admin upload for updater artifacts with signature"
```

---

### Task 7: Client Rust — updater plugin and commands

**Files:**
- Create: `pointer-app/src-tauri/src/updater_commands.rs`
- Modify: `pointer-app/src-tauri/src/lib.rs`
- Modify: `pointer-app/src-tauri/capabilities/default.json`

- [ ] **Step 1: Register plugin in lib.rs**

```rust
#[cfg(desktop)]
{
    app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
}
```

- [ ] **Step 2: Create updater_commands.rs**

```rust
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Serialize)]
pub struct UpdateCheckResult {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub current_version: String,
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateCheckResult, String> {
    use tauri_plugin_updater::UpdaterExt;
    let current_version = app.package_info().version.to_string();
    let Some(update) = app.updater()?.check().await.map_err(|e| e.to_string())? else {
        return Ok(UpdateCheckResult {
            available: false,
            version: None,
            notes: None,
            current_version,
        });
    };
    Ok(UpdateCheckResult {
        available: true,
        version: Some(update.version.clone()),
        notes: update.body.clone(),
        current_version,
    })
}

#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let Some(update) = app.updater()?.check().await.map_err(|e| e.to_string())? else {
        return Err("no_update_available".into());
    };
    let mut downloaded = 0;
    update
        .download_and_install(
            |chunk_len, content_len| {
                downloaded += chunk_len;
                let _ = app.emit(
                    "updater://download-progress",
                    serde_json::json!({
                        "downloaded": downloaded,
                        "total": content_len,
                    }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("updater://status", serde_json::json!({ "phase": "ready" }));
    Ok(())
}

#[tauri::command]
pub async fn relaunch_app(app: AppHandle) -> Result<(), String> {
    app.restart();
}
```

- [ ] **Step 3: Register commands and capabilities**

Add to invoke handler and `capabilities/default.json` permissions for updater plugin.

- [ ] **Step 4: Commit**

```bash
git add pointer-app/src-tauri/src/updater_commands.rs pointer-app/src-tauri/src/lib.rs pointer-app/src-tauri/capabilities/default.json
git commit -m "feat(desktop): Tauri updater commands for check, download, relaunch"
```

---

### Task 8: Client Vue — updater composable and UI

**Files:**
- Create: `pointer-app/src/composables/useAppUpdater.ts`
- Create: `pointer-app/src/components/updater/UpdateReadyBanner.vue`
- Create: `pointer-app/src/components/settings/panels/AboutSettingsPanel.vue`
- Modify: `pointer-app/src/components/settings/SettingsDialog.vue`
- Modify: `pointer-app/src/App.vue`

- [ ] **Step 1: Create useAppUpdater.ts**

```typescript
import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isTauriRuntime } from '../lib/runtime'

const SKIPPED_VERSION_KEY = 'pointer.updater.skippedVersion'
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000
const STARTUP_DELAY_MS = 30_000

export function useAppUpdater() {
  const updateReady = ref(false)
  const updateVersion = ref<string | null>(null)
  const updateNotes = ref<string | null>(null)
  const checking = ref(false)
  const downloading = ref(false)
  const error = ref<string | null>(null)

  async function checkAndDownload() {
    if (!isTauriRuntime()) return
    // platform mode only — standalone has no official update server
    const skipped = localStorage.getItem(SKIPPED_VERSION_KEY)
    checking.value = true
    error.value = null
    try {
      const result = await invoke<{
        available: boolean
        version?: string
        notes?: string
      }>('check_for_update')
      if (!result.available || !result.version) return
      if (skipped === result.version) return
      updateVersion.value = result.version
      updateNotes.value = result.notes ?? null
      downloading.value = true
      await invoke('download_and_install_update')
      updateReady.value = true
    } catch (e) {
      error.value = String(e)
      console.warn('[updater] check/download failed', e)
    } finally {
      checking.value = false
      downloading.value = false
    }
  }

  async function relaunch() {
    await invoke('relaunch_app')
  }

  function dismiss() {
    updateReady.value = false
  }

  function skipVersion() {
    if (updateVersion.value) {
      localStorage.setItem(SKIPPED_VERSION_KEY, updateVersion.value)
    }
    updateReady.value = false
  }

  let startupTimer: ReturnType<typeof setTimeout> | undefined
  let intervalTimer: ReturnType<typeof setInterval> | undefined

  onMounted(async () => {
    if (!isTauriRuntime()) return
    await listen('updater://download-progress', () => {})
    startupTimer = setTimeout(() => void checkAndDownload(), STARTUP_DELAY_MS)
    intervalTimer = setInterval(() => void checkAndDownload(), CHECK_INTERVAL_MS)
  })

  onUnmounted(() => {
    if (startupTimer) clearTimeout(startupTimer)
    if (intervalTimer) clearInterval(intervalTimer)
  })

  return {
    updateReady,
    updateVersion,
    updateNotes,
    checking,
    downloading,
    error,
    checkAndDownload,
    relaunch,
    dismiss,
    skipVersion,
  }
}
```

- [ ] **Step 2: Create UpdateReadyBanner.vue**

Fixed bottom banner when `updateReady`:

- Title: `新版本 {{ version }} 已就绪`
- Buttons: 「立即重启」「稍后」「跳过此版本」

- [ ] **Step 3: Create AboutSettingsPanel.vue**

Show current version (from `invoke` or compile-time constant). Button 「检查更新」calls `checkAndDownload()` with visible loading/error states.

- [ ] **Step 4: Wire into SettingsDialog**

Add section `{ id: 'about', label: '关于', desc: '版本与更新' }` visible only when `isTauriRuntime()`.

- [ ] **Step 5: Mount banner in App.vue**

```vue
<UpdateReadyBanner v-if="isTauriRuntime()" />
```

Initialize composable once at app root.

- [ ] **Step 6: Manual test against official staging API**

Run platform-mode app with `POINTER_API_BASE` pointing at staging official API. Publish a newer version via admin console. Confirm banner after download.

- [ ] **Step 7: Commit**

```bash
git add pointer-app/src/composables/useAppUpdater.ts pointer-app/src/components/updater/ pointer-app/src/components/settings/panels/AboutSettingsPanel.vue pointer-app/src/components/settings/SettingsDialog.vue pointer-app/src/App.vue
git commit -m "feat(desktop): background update check and restart banner UI"
```

---

### Task 9: Document official release + auto-update workflow

**Files:**
- Modify: `pointer-official/docs/app-releases-deployment.md`

- [ ] **Step 1: Extend app-releases-deployment.md**

Add sections:

- **自动升级**：`GET /api/updates/latest` 与 `/download` 共用发布状态与存储
- **上传清单**：各平台安装包 + updater 包 + signature 字段
- **验收步骤**：curl 两个公开接口 + 旧版客户端端到端
- **明确说明**：仅控制台「发布」后用户才可见更新

- [ ] **Step 2: Commit**

```bash
git add pointer-official/docs/app-releases-deployment.md
git commit -m "docs: official release workflow includes auto-update"
```

---

### Task 10: End-to-end verification (official release path)

**Files:**
- Modify: `pointer-app/docs/design/app-auto-update.md` (check off test results if desired)

- [ ] **Step 1: Publish test release via official admin only**

Upload 0.1.2 installers + updater artifacts with signatures in **客户端发布**. Click **发布**.

- [ ] **Step 2: Verify public API parity**

```bash
# Same version on both endpoints
curl -s "$API/api/downloads" | jq .version
curl -s "$API/api/updates/latest?target=darwin&arch=aarch64&current_version=0.1.1" | jq .version
```

- [ ] **Step 3: Run old client (0.1.1)**

Confirm: silent check → download from `/api/downloads/files/{id}` → banner → restart → version 0.1.2.

- [ ] **Step 4: Test edge cases**

- Unpublished draft version → client sees no update
- Skip version → no prompt for same version
- Manual check in About panel
- Airplane mode → graceful failure; fallback link to `{web_base}/download`
- Linux deb-only publish → no auto-update prompt (204)
- Standalone deployment → updater disabled

- [ ] **Step 5: Platform matrix**

| Platform | Signed build | Pass |
|----------|--------------|------|
| macOS universal | Yes | |
| Windows x64 | Yes | |
| Linux AppImage | Yes | |

---

## Self-Review (spec coverage)

| Spec section | Task |
|--------------|------|
| Official release integration | Task 1, 9, 10 |
| Updater keys | Task 1 |
| createUpdaterArtifacts + official API endpoint | Task 2 |
| DB signature + kinds | Task 3–4 |
| GET /api/updates/latest | Task 5 |
| Admin upload (same console as /download) | Task 6 |
| Client plugin + commands | Task 7 |
| Vue UX (background + restart banner) | Task 8 |
| E2E via official publish | Task 10 |

---

## Execution Handoff

Plan complete and saved to `pointer-app/docs/design/app-auto-update-implementation-plan.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks
2. **Inline Execution** — implement tasks in this session with checkpoints

Which approach do you prefer?

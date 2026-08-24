# User vs platform configuration split

## Overview

Runtime configuration is split into two layers:

| Layer | Contents | Persistence | Editable by |
|-------|----------|-------------|-------------|
| **User** | Everything the user can edit: theme, coding rules, completion sound, providers (structure, no secrets), active provider/model/temperature/maxTokens/contextBudgetTokens, tool approval, agent mode, context compression switch, tool rounds, **scene tier LLM maps** (`agentModeLlm` / `mediaModeLlm` / `computerTierLlm`), Computer prefs, parallel limits. Debug-only toggles: completion dump, raw content, terminal env, etc. | `user_settings.json` — **full snapshot, no whitelist**. User-typed provider keys are encrypted into `provider_keys.enc` (AES-256-GCM, machine-bound) | All users for tier maps and regular prefs; debug toggles: `is_platform_admin` only |
| **Platform** | In-memory only: runtime provider list (with injected OAuth keys), platform model directory (`platformProviders` + `tierDefaults`), media OSS credentials, server-side DaTi CAPTCHA config | Directory is cached locally for offline restart; never copied into `user_settings.json` | Platform admin on the control plane |

Merged **`ModelSettings`** is built at runtime via `merge_user_platform(user, platform)` (user fields + platform runtime keys/media_oss/dati) and used by chat, tools, and the UI.

## Desktop (Tauri)

- **OAuth refresh token**: encrypted in `{data_dir}/PointerApp/auth.dat` (AES-256-GCM, machine-bound key via HKDF). No OS keyring.
- **Login / refresh**: `/auth/app/token` returns `api_key`, `llm_provider`, `user.is_platform_admin`, and the platform model directory (`platformProviders` + `tierDefaults`). The client creates/updates platform providers from that directory and does not keep a local allowlist of platform models.
- **Normal users**: use platform-issued API key; cannot edit debug toggles in the UI. They **can** override scene tier models; those maps persist.
- **Platform admins**: may override provider/model/debug settings; all user-owned fields persist to `user_settings.json`; apiKey stays in memory / OAuth-injected.

### Settings save actions

- **智能体 / 模型服务 / 界面配置 / 调试**: all persist to `user_settings.json` (single save path; provider apiKey cleared on write).
- **Provider keys**: only keys the user explicitly typed are encrypted into `provider_keys.enc` (AES-256-GCM, machine-bound, separate purpose key from `auth.dat`) and survive restarts. Platform-injected keys (OAuth / login) are never persisted — they live only in platform memory. Standalone keys are user-typed and persist the same way as desktop custom providers.
- **平台账户**: login/logout via OAuth (`auth.dat`); no footer save.
- Theme follows `user_settings.json` (round-trips through the API).

### API

- `GET get_settings` → `EffectiveSettingsView` (`user`, `platform`, `merged`, `canEditPlatform`, `isPlatformAdmin`)
- `PUT update_user_settings` → persists full `UserSettings` immediately (single user-owned save path; frontend sends a user-slice snapshot, backend overwrites directly)
- `PUT update_debug_session_settings` → admin only; in-memory only (never persisted; session-scoped debug providers)
- `PUT update_platform_settings` → admin only; in-memory only (no disk write)

## Web server

- No platform login; `canEditPlatform` is always `false`.
- Non-admin GET strips **debug** fields from `user` / `merged`. Scene tier maps stay.
- Non-admin PUT preserves server **debug** values (serde defaults never wipe them). Incoming `agentModeLlm` / `mediaModeLlm` / `computerTierLlm` are kept.
- `PUT /api/platform-settings` returns an error (read-only).

## Migration from legacy `settings.json`

On first startup after upgrade:

1. If `settings.json` exists and `settings.json.migrated` does not, read it → write **full `user_settings.json`** (theme + providers + user/debug fields).
2. Rename `settings.json` → `settings.json.migrated` (backup).
3. Remove deprecated `key.dat` if present.

**No keyring migration** — users re-login once; refresh token is stored in `auth.dat`.

## Computer agent tier LLM

User setting `computerTierLlm` maps `primary` | `intermediate` | `advanced` to model + thinking flags. Runtime `ComputerRoundLlmOverrides::for_tier` reads this map (tier config overrides agent manifest model defaults).

Defaults are **no longer embedded locally**: they come from the platform directory `tierDefaults.computerTierLlm` on login and are merged into the runtime view (user-configured values keep precedence). Before login / without platform defaults, the map is empty and computer runs require an explicit configuration.

## Platform model directory

The control plane is the only place that adds, removes, or reorders platform models.

- Login / token refresh applies `platformProviders` as a complete directory: create missing providers, replace their model lists and model-level params, and drop `source=platform` providers that the directory no longer lists.
- This client does not read `modelCatalog` to build providers. That field stays on the official login APIs so older clients keep working; the new client only consumes `platformProviders` + `tierDefaults`.
- Scene defaults (`agentModeLlm` / `mediaModeLlm` / `computerTierLlm` / `computerPipelineLlm` / `mediaGeneration`) come from `tierDefaults`. The settings UI compares “已覆盖” against this directory, not against names compiled into the client.
- User settings never persist provider records with `source=platform` (id does not matter; the catalog can add or replace vendors). A user fork of the same id must be `source=user`. Scene **tier maps** may point at platform provider ids — that is a user override, not a provider record.
- DashScope / DeepSeek / Volcengine **API dialect** is inferred from base URL (and directory capability fields), not from a frozen vendor id list.
- Capability flags (`supportsVision` / `supportsAudio` / `canGenerateImage` / `canGenerateVideo`) come only from the platform model entry or the user’s checkboxes. Unset means off. Image/video understanding use vision; speech-to-text uses `supportsAudio`; image/video generation pickers use the generation flags. Do not infer from model name or API URL.
- Explicit catalog flags must survive client merge/prune so scene pickers can list the model.
- Default image/video generators come from `tierDefaults.mediaGeneration` and fill `mediaModelOverrides` when the user has not set them.
- Sampling: catalog `temperature` (default 0.7) and `topP` (default 0.95) apply per provider/model and are sent as `temperature` / `top_p` on chat/completions.
- Billing rate lives on each official model row on the control plane. Login payloads and the directory hash omit rate fields, so a rate-only edit does not rebuild client providers.

## Security notes (`auth.dat`)

- Protects against casual file reads; not HSM/keychain-grade.
- Blob is bound to the machine (`machine-uid` with hostname/username fallback).
- Copying `auth.dat` to another host fails decryption.

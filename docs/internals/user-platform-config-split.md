# User vs platform configuration split

## Overview

Runtime configuration is split into two layers:

| Layer | Contents | Persistence | Editable by |
|-------|----------|-------------|-------------|
| **User** | Everything the user can edit, incl. debug fields: theme, coding rules, completion sound, providers (structure, no secrets), active provider/model/temperature/maxTokens, tool approval, agent mode, context settings, tool rounds, mode/tier LLM maps, Computer prefs, parallel limits | `user_settings.json` — **full snapshot, no whitelist**. User-typed provider keys are encrypted into `provider_keys.enc` (AES-256-GCM, machine-bound) | All users (debug fields: `is_platform_admin` only) |
| **Platform** | In-memory only: runtime provider list (with injected OAuth/TOML keys), media OSS credentials, server-side DaTi CAPTCHA config | **Never persisted** | `is_platform_admin` only |

Merged **`ModelSettings`** is built at runtime via `merge_user_platform(user, platform)` (user fields + platform runtime keys/media_oss/dati) and used by chat, tools, and the UI.

## Desktop (Tauri)

- **OAuth refresh token**: encrypted in `{data_dir}/PointerApp/auth.dat` (AES-256-GCM, machine-bound key via HKDF). No OS keyring.
- **Login / refresh**: `/auth/app/token` returns `api_key`, `llm_provider`, and `user.is_platform_admin`. Credentials are injected into the in-memory provider list (`apply_login_llm_credentials`).
- **Normal users**: use platform-issued API key; cannot edit debug fields in the UI.
- **Platform admins**: may override provider/model/debug settings; all user-owned fields persist to `user_settings.json`; apiKey stays in memory / OAuth-injected.

### Settings save actions

- **智能体 / 模型服务 / 界面配置 / 调试**: all persist to `user_settings.json` (single save path; provider apiKey cleared on write).
- **Provider keys**: only keys the user explicitly typed are encrypted into `provider_keys.enc` (AES-256-GCM, machine-bound, separate purpose key from `auth.dat`) and survive restarts. Platform-injected keys (OAuth / server.toml) are never persisted — they live only in platform memory.
- **平台账户**: login/logout via OAuth (`auth.dat`); no footer save.
- Theme follows `user_settings.json` (round-trips through the API).

### API

- `GET get_settings` → `EffectiveSettingsView` (`user`, `platform`, `merged`, `canEditPlatform`, `isPlatformAdmin`)
- `PUT update_user_settings` → persists full `UserSettings` immediately
- `PUT update_agent_settings` → merges incoming ModelSettings user fields, persists to `user_settings.json`
- `PUT update_settings` → same as `update_agent_settings` (user-owned persistence)
- `PUT update_debug_session_settings` → admin only; persists debug model config to `user_settings.json`
- `PUT update_platform_settings` → admin only; in-memory only (no disk write)

## Web server

- No platform login; `canEditPlatform` is always `false`.
- Non-admin GET strips debug fields from `user` slice too; non-admin PUT preserves server debug values (serde defaults never wipe them).
- `PUT /api/platform-settings` returns an error (read-only).

## Migration from legacy `settings.json`

On first startup after upgrade:

1. If `settings.json` exists and `settings.json.migrated` does not, read it → write **full `user_settings.json`** (theme + providers + user/debug fields).
2. Rename `settings.json` → `settings.json.migrated` (backup).
3. Remove deprecated `key.dat` if present.

**No keyring migration** — users re-login once; refresh token is stored in `auth.dat`.

## Computer agent tier LLM

User setting `computerTierLlm` maps `primary` | `intermediate` | `advanced` to model + thinking flags. Runtime `ComputerRoundLlmOverrides::for_tier` reads this map (tier config overrides agent manifest model defaults).

Defaults:

- Primary / intermediate: `qwen3.5-plus`, thinking on, budget 2048
- Advanced: `qwen3.7-plus`, thinking on, budget 8192

## Security notes (`auth.dat`)

- Protects against casual file reads; not HSM/keychain-grade.
- Blob is bound to the machine (`machine-uid` with hostname/username fallback).
- Copying `auth.dat` to another host fails decryption.

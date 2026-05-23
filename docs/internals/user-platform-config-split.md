# User vs platform configuration split

## Overview

Runtime configuration is split into two layers:

| Layer | Contents | Persistence | Editable by |
|-------|----------|-------------|-------------|
| **User** | Theme; optional UI cache (`userNickname`) | `user_settings.json` | All users |
| **Platform** | Providers, generation params, agent defaults, Computer tier LLM, workspace, etc. | **In-memory only** (process lifetime) | `is_platform_admin` only |

Merged **`ModelSettings`** is built at runtime via `merge_user_platform(user, platform)` and used by chat, tools, and the UI.

## Desktop (Tauri)

- **OAuth refresh token**: encrypted in `{data_dir}/PointerApp/auth.dat` (AES-256-GCM, machine-bound key via HKDF). No OS keyring.
- **Login / refresh**: `/auth/app/token` returns `api_key`, `llm_provider`, and `user.is_platform_admin`. Credentials are injected into the in-memory provider list (`apply_login_llm_credentials`).
- **Normal users**: use platform-issued API key; cannot edit platform settings in the UI.
- **Platform admins**: may override provider/model settings for the current session; changes are lost on restart (defaults restored, re-login re-injects keys).

### API

- `GET get_settings` → `EffectiveSettingsView` (`user`, `platform`, `merged`, `canEditPlatform`, `isPlatformAdmin`)
- `PUT update_user_settings` → theme only, persisted immediately
- `PUT update_platform_settings` → admin only, memory
- Legacy `update_settings` → admin: platform patch; non-admin: theme only

## Web server

- No platform login; `canEditPlatform` is always `false`.
- Platform config stays at code defaults; theme still persists via `user_settings.json`.
- `PUT /api/platform-settings` returns an error (read-only).

## Migration from legacy `settings.json`

On first startup after upgrade:

1. If `settings.json` exists and `settings.json.migrated` does not, read **theme only** → write `user_settings.json`.
2. Rename `settings.json` → `settings.json.migrated` (backup).
3. Remove deprecated `key.dat` if present.
4. Platform fields are imported into `local_platform_settings.json` (desktop) from legacy `settings.json` / `settings.json.migrated`.

**No keyring migration** — users re-login once; refresh token is stored in `auth.dat`.

## Computer agent tier LLM

Platform setting `computerTierLlm` maps `primary` | `intermediate` | `advanced` to model + thinking flags. Runtime `ComputerRoundLlmOverrides::for_tier` reads this map (tier config overrides agent manifest model defaults).

Defaults:

- Primary / intermediate: `qwen3.5-plus`, thinking on, budget 2048
- Advanced: `qwen3.6-plus`, thinking on, budget 8192

## Security notes (`auth.dat`)

- Protects against casual file reads; not HSM/keychain-grade.
- Blob is bound to the machine (`machine-uid` with hostname/username fallback).
- Copying `auth.dat` to another host fails decryption.

# Computer Mouse Movement

## Goal

Standalone, extensible mouse movement for the computer agent. Path geometry and per-step timing are planned separately (aligned with Python `MouseMove._generate_path` / `_calculate_intervals`). Tool handlers call `ActionBackend::move_to_with_profile`; `human_like` in `tool_args` or `computerHumanLike` in agent config selects the profile.

## Current implementation

**Modules:** `crates/pointer-core/src/agents/computer/mouse_path.rs`, `mouse_move.rs`, `timing.rs`, `action_enigo.rs`

### Architecture

| Component | Role |
|-----------|------|
| `mouse_path::bezier_path` | Cubic Bézier waypoints (Python `mouse_path.py`) |
| `MouseMovePathPlanner` | `(from, to)` → waypoint list (geometry only) |
| `MouseMoveTimingPlanner` | waypoint count → per-step sleep intervals |
| `MouseMovePlanner` | composes path + timing + optional path jitter → `MouseMovePlan` |
| `execute_move_plan` | pre-delay / pre-jitter → waypoint moves + sleeps → final `move_abs` at target |
| `EnigoBackend::move_to_with_profile` | read position → plan → execute via enigo |

### Profiles

| Profile | When | Path | Time |
|---------|------|------|------|
| `MouseMoveProfile::fast()` | Default (`human_like=false`) | 1-point straight | ~0.05s total |
| `MouseMoveProfile::human_like()` | `human_like=true` or `computerHumanLike` | Bézier ~10 pts + path jitter | 0.5s ease-out + pre-jitter (2 steps) |
| `drag_to_start` / `drag_segment` | Drag with `human_like` | Bézier | 0.35s / 0.45s ease-in-out (+ perturb on drag segment) |

### Click / hover flow (`actions.rs`)

All absolute moves go through `move_to_with_profile`. Additional delays:

| Constant | Value | When |
|----------|-------|------|
| `SETTLE_AFTER_ABSOLUTE_MOVE_MS` | 100ms | After move, before click / double-click / right-click / scroll-at |
| `POST_MOUSE_BUTTON_SETTLE_MS` | 50ms | After button gesture |
| — | — | `hover_at` / `hover_index`: move only, **no** settle |

### Configuration

- **Per tool call:** optional `human_like` (bool) in `tool_args` overrides default.
- **Settings UI:** 设置 → 执行智能体 → Computer → **人性化鼠标移动**（`computerHumanLike`，默认关闭）。
- **Agent config (fallback):** `computerHumanLike: "true"` in [`AGENT.md`](../pointer-core/src/agents/computer/AGENT.md) front matter when settings cannot be loaded.

### Backend

- **enigo** `move_mouse(Abs)` on macOS / Windows / Linux (HID-level synthetic events).
- Prompt `human_like` is wired in `tool_mouse`, `tool_composite`, `tool_modified_click`.

## Tunables (`timing.rs` / `mouse_move.rs`)

```text
MOUSE_MOVE_DEFAULT_POINT_COUNT      = 10
MOUSE_MOVE_TOTAL_DURATION_SECS      = 0.5
MOUSE_MOVE_FAST_DURATION_SECS       = 0.05
SETTLE_AFTER_ABSOLUTE_MOVE_MS       = 100
POST_MOUSE_BUTTON_SETTLE_MS         = 50
```

## Extension roadmap

### Phase 1 — Profiles (done)

- `fast` / `human_like` / drag presets
- `human_like` tool + `computerHumanLike` config default

### Phase 2 — Settings UI (done)

- Settings → Computer card toggle for `computerHumanLike` (persist in app settings, APP + WEB)

### Phase 3 — Guards

- Max duration, max points, fail-fast on repeated backend errors

## Cross-platform notes

- **macOS / Windows / Linux:** shared planner and enigo execution; Accessibility / input permissions required where applicable.
- **Surface:** runtime backend only; no protocol change between APP and WEB clients.

## Reference (Python)

| Aspect | Python (`human_like=True`) | Rust (`human_like=True`) |
|--------|---------------------------|-------------------------|
| Path | Bézier ~10 points | Bézier ~10 points |
| Time | 0.5s ease-out total | 0.5s ease-out total |
| Pre-click settle | 100ms | 100ms |

| Aspect | Python (`human_like=False`) | Rust (`human_like=False`) |
|--------|------------------------------|---------------------------|
| Path | ~instant | 1-point fast (~0.05s) |

See `D:\workspace\pointer\agents\computer\mouse_move.py`, `mouse_path.py`.

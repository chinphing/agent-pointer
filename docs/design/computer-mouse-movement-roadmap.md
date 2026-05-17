# Computer Mouse Movement

## Goal

Standalone, extensible mouse movement for the computer agent. Path geometry and per-step timing are planned separately (aligned with Python `MouseMove._generate_path` / `_calculate_intervals`). Tool handlers call `ActionBackend::move_to` only; they do not configure movement profiles yet.

## Current implementation

**Modules:** `crates/pointer-core/src/agents/computer/mouse_move.rs`, `timing.rs`, `action_enigo.rs`

### Architecture

| Component | Role |
|-----------|------|
| `MouseMovePathPlanner` | `(from, to)` → waypoint list (geometry only) |
| `MouseMoveTimingPlanner` | waypoint count → per-step sleep intervals |
| `MouseMovePlanner` | composes path + timing → `MouseMovePlan` |
| `execute_move_plan` | each waypoint: backend `move_abs` + `sleep(dt)`; then one more `move_abs` at `target` for OS hover refresh |
| `EnigoBackend::move_to` | read position → plan → execute via enigo |

### Path geometry (`LinearUniform`)

1. **Main segment:** straight line from current cursor to target, uniform parameter `t`, max **14px** between consecutive points (`MOUSE_MOVE_LINEAR_STEP_MAX_PX`). Point count scales with distance (`ceil(distance / 14)`).
2. **Final approach:** re-sample only the **last coarse segment** (second-to-last waypoint → target; if the coarse path has one point, from `from` → target) at **5px** steps (`MOUSE_MOVE_APPROACH_STEP_MAX_PX`), then a **1px** hop to the target (`MOUSE_MOVE_APPROACH_FINAL_GAP_PX`: penultimate waypoint sits one pixel before the aim point, last waypoint is the target). Helps OS / app hover and hit-testing without changing the intended pixel.
3. **Same point:** `from == to` → empty path, move skipped.

No Bézier / jitter yet. Spatial density on the final segment is higher; there is no separate “denser only in time” rule on geometry.

### Timing (default)

| Setting | Value |
|---------|--------|
| Mode | `MouseMoveDurationMode::Total` |
| Total duration | **0.5s** (`MOUSE_MOVE_TOTAL_DURATION_SECS`) |
| Curve | **ease-out** (`ease_in_out = false`), same formula as Python `_ease_out_intervals` |
| Distribution | One interval per waypoint; intervals sum to 0.5s; **shorter sleeps early, longer near the end** |

Reserved but unused by default: `Step` mode (fixed **0.03s** per point, `MOUSE_MOVE_STEP_DURATION_SECS`), `ease_in_out`.

### Click / hover flow (`actions.rs`)

All absolute moves go through `EnigoBackend::move_to` (path above). Additional delays:

| Constant | Value | When |
|----------|-------|------|
| `SETTLE_AFTER_ABSOLUTE_MOVE_MS` | 100ms | After move, before click / double-click / right-click / scroll-at |
| `POST_MOUSE_BUTTON_SETTLE_MS` | 50ms | After button gesture |
| — | — | `hover_at` / `hover_index`: move only, **no** settle |

Typical `click_at`: ~0.5s move sleeps + 100ms settle + click + 50ms (plus optional `tool_args.wait` before next screenshot).

### Backend

- **enigo** `move_mouse(Abs)` on macOS / Windows / Linux (HID-level synthetic events).
- Prompt `human_like` is **not** wired to Rust yet; all moves use `MouseMoveConfig::default()`.

### Removed / not used

- Fixed **6ms** per-step interval (replaced by total-time ease-out).
- macOS Session-tap post-move nudge (`mouse_hover_refresh`) — removed after dense approach proved sufficient in testing.

## Tunables (`timing.rs`)

```text
MOUSE_MOVE_LINEAR_STEP_MAX_PX      = 14.0
MOUSE_MOVE_APPROACH_STEP_MAX_PX    = 5.0
MOUSE_MOVE_APPROACH_FINAL_GAP_PX   = 1.0
MOUSE_MOVE_TOTAL_DURATION_SECS     = 0.5
MOUSE_MOVE_STEP_DURATION_SECS      = 0.03   # Step mode only
SETTLE_AFTER_ABSOLUTE_MOVE_MS      = 100
POST_MOUSE_BUTTON_SETTLE_MS        = 50
```

## Why this structure

- Stable `move_to` API for `ActionExecutor` and tools
- Path vs timing split matches Python and allows new strategies without touching call sites
- Unit tests on planners and ease curves (`mouse_move` tests)
- Cross-platform: same planner; enigo for execution (APP + WEB share backend)

## Extension roadmap

### Phase 1 — Profiles

- Presets: `fast`, `balanced`, `precise` (step sizes, total duration)
- Optional per-tool override (e.g. drag)

### Phase 2 — Curved paths

- Bézier / spline branch (Python `mouse_path.py` parity)
- Optional jitter with caps; deterministic mode for CI

### Phase 3 — Tool integration

- Map `human_like` in `tool_args` to `MouseMoveConfig`
- Structured logs: strategy, point count, elapsed time

### Phase 4 — Guards

- Max duration, max points, fail-fast on repeated backend errors

## Cross-platform notes

- **macOS / Windows / Linux:** shared planner and enigo execution; Accessibility / input permissions required where applicable.
- **Surface:** runtime backend only; no protocol change between APP and WEB clients.

## Reference (Python)

| Aspect | Python (`human_like=True`) | Rust (current default) |
|--------|---------------------------|-------------------------|
| Path | Bézier ~10 points | Straight 14px + final 5px segment |
| Time | 0.5s ease-out total | 0.5s ease-out total |
| Pre-click settle | 100ms | 100ms |

See `PyProjects/pointer/agents/computer/mouse_move.py`, `mouse_path.py`.

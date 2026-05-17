# Computer Mouse Movement Roadmap

## Goal

Build a standalone and extensible mouse movement layer for the computer agent.
The first production version uses straight-line uniform movement only.
Future strategies can be introduced without changing tool handlers.

## Current implementation (v1)

- Added dedicated module: `crates/pointer-core/src/agents/computer/mouse_move.rs`
- Split responsibilities:
  - `MouseMovePlanner`: converts `(from -> to)` into waypoints
  - `execute_move_plan`: executes waypoints against backend move API
- Strategy abstraction:
  - `MouseMoveStrategy` enum (currently `LinearUniform`)
  - `MouseMoveConfig` for tunables
- Runtime behavior:
  - read current cursor position
  - generate straight-line waypoints with uniform step size
  - move point by point with fixed sleep interval
- Integration point:
  - `EnigoBackend::move_to` now goes through planner + executor path

## Tunables (v1 defaults)

- `MOUSE_MOVE_LINEAR_STEP_MAX_PX = 14.0`
- `MOUSE_MOVE_LINEAR_STEP_INTERVAL_MS = 6`

These are centralized in `crates/pointer-core/src/agents/computer/timing.rs`
so we can tune movement smoothness/latency without touching call sites.

## Why this structure

- Keeps action API stable (`move_to` stays unchanged for callers)
- Makes strategy evolution low-risk (new enum branch + planner logic)
- Makes behavior testable (planner and executor are unit-testable)
- Preserves cross-platform compatibility by reusing enigo absolute move calls

## Extension roadmap

### Phase 1: Configurable linear profiles

- Add profile presets: `fast`, `balanced`, `precise`
- Dynamically tune step size by distance and action type
- Optional per-tool override (e.g. drag may require denser sampling)

### Phase 2: Easing and timing curves

- Add ease-in/ease-out timing on top of linear geometry
- Keep endpoint precision guaranteed
- Add guardrails to avoid over-slow behavior on short distances

### Phase 3: Curved/human-like paths

- Add Bezier/spline path planner branch
- Optional jitter/noise model with strict caps
- Keep deterministic mode for test and CI reproducibility

### Phase 4: Safety and observability

- Emit structured logs for movement stats:
  - strategy, point count, total distance, elapsed time
- Add optional safety clamps:
  - max duration
  - max points
  - fail-fast on backend repeated move errors

## Implementation plan for next iteration

1. Introduce `MouseMoveProfile` presets mapped to config values.
2. Thread profile through action layer (internal only, no tool schema change yet).
3. Add integration tests:
   - long-distance move point count bounds
   - endpoint correctness under all profiles
4. Add movement metrics log at info/debug level.
5. Evaluate if drag path should adopt a separate movement policy.

## Cross-platform and cross-surface notes

- Platform: v1 implementation is shared for macOS/Windows/Linux because it uses
  the same enigo absolute movement API.
- Surface: logic is backend runtime behavior; APP and WEB entry points consume
  the same backend capability and require no divergent protocol changes.

/**
 * Vitest global setup: pin the host capabilities tests are allowed to depend on.
 *
 * `src/stores/settings.ts` derives the sub-agent fan-out ceiling from
 * `navigator.hardwareConcurrency` — `clamp(cores × 10, 32, 100)` — and falls back
 * to 1 core when the value is missing. That makes any assertion about the ceiling
 * depend on the machine running the test:
 *
 *   - Node 20 has no `navigator` global at all  → 1 core  → ceiling 32
 *   - Node 21+ reports the real CPU count       → 8 cores → ceiling 80
 *
 * A test asserting "33 survives the clamp" therefore passed on a developer laptop
 * and failed in CI for a reason that had nothing to do with the code under test.
 *
 * Pin the number here so every suite sees the same host. A test that needs a
 * different value stubs it itself (`vi.stubGlobal('navigator', …)`).
 */

/** Fixed logical-core count reported to every test. */
export const TEST_LOGICAL_CORES = 8

if (!globalThis.navigator) {
  // Node 20: no `navigator` at all. Create a minimal one so property pinning works.
  Object.defineProperty(globalThis, 'navigator', {
    value: {},
    configurable: true,
    writable: true
  })
}

Object.defineProperty(globalThis.navigator, 'hardwareConcurrency', {
  value: TEST_LOGICAL_CORES,
  configurable: true,
  writable: true
})

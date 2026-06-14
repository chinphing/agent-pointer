## Exploration closure

Before claiming breadth is complete for a behavior change, close three loops for every anchor:

1. **Identity fan-out** — symbol, wire string (quoted key, route, env name), naming aliases. **Grep each identity repo-wide**; record hits or **0 hits**.
2. **Registration chain** — define → register/wire → default/init → read/use → display/persist.
3. **Boundary pass (Surfaces)** — server/runtime, RPC/IPC, client/UI, config, docs/prompts: verify, list readers, or skip with reason.

### Impact scan hints

| If the anchor is… | Before editing, you must… |
|-------------------|---------------------------|
| Function, type, field, constant | Identity fan-out globally; read non-test hits you may affect; Surfaces pass |
| Config, env key, route, API shape | Grep **wire string** globally; trace registration chain; list all-layer readers |
| Persistent or session state | Trace create → update → clear; include next session / next user turn |
| Error message, early return | Trace who catches or displays it |
| Threshold, enum, policy text | Grep same value in tests and nearby prompts/docs |
| Public or cross-package export | Grep importers outside the immediate file |

**Anti-patterns:** stop at first match; grep symbol but not wire string; treat Surfaces as optional for non-config changes; assume single-layer scope without recording negative grep.

# Native Tool Calling Protocol

This project uses provider-native tool calling as the only runtime protocol
for agent actions. User-facing delivery aligns with OpenClaw: **final replies
are assistant message text**, not a separate delivery tool.

## Contract

- Agents call tools through native `tool_calls`.
- Tool arguments are sent as native function args.
- **User-facing replies** are written as **assistant `content`** on the final
  turn (no standalone `response` tool).
- **IM outbound media:** append `MEDIA:` lines in the final assistant reply; the
  host delivers text and files to the IM channel — see `docs/developer/channel-integration.md`.
- Text-serialized custom envelopes are deprecated and must not be emitted by prompts.

## Request/response behavior

- Chat requests include `tools` and `tool_choice=auto` when tool use is enabled for the round.
- Stream parsing consumes native `delta.tool_calls` and assembles final tool calls by index and id.
- Message replay for next rounds preserves `assistant.tool_calls` and `role=tool` messages.

## Compatibility policy

- No fallback to the legacy JSON envelope parser for new runs.
- Providers that do not support native tool calling are considered unsupported for tool-enabled rounds.
- Legacy persisted messages that used `tool_name: response` JSON envelopes are still parsed for display.

## Observability requirements

- Parsing or provider anomalies must produce warnings.
- Tool execution errors must not be silent.
- Keep informative logs around stream parsing, malformed args, and protocol mismatch paths.

## Parallel tool execution (same assistant turn)

When the model returns multiple native `tool_calls` in one assistant message, the host may
execute eligible tools concurrently subject to conflict detection and platform limits.

### Parallel-eligible tools (default)

| Category | Tool ids |
|----------|----------|
| File read | `file_read`, `file_grep`, `file_glob`, `file_list` |
| File write | `file_write`, `file_edit` (different paths only) |
| Terminal | `terminal` |
| Sub-agent | `run_subagent` (serial waves; one active delegation mut path) |
| Media | `image_generate`, `video_generate`, `media_understand` |
| Other | `web_search`, `skill_read`, `session_search`, `memory` |

### Not parallel (serial)

- All Computer desktop tools
- Sidecar tools (`task_board_*`, `action_verify`, …)
- `read_lints`, `cron_job`, `skill_import` (P1 serial; intra-tool parallelism optional)

### Conflict rules

- Same canonical file path: read/write/edit must not overlap in one parallel wave.
- `terminal`: no conversation-level mutex; abort is per `tool_call_id`.
- Media tools (`media_understand`, `image_generate`, `video_generate`): no batch conflict key;
  multiple jobs in one wave are allowed, capped by `maxParallelMediaJobs`.

### Concurrency limits (platform settings)

| Setting | Default |
|---------|---------|
| `maxParallelToolCalls` | `min(CPU logical cores, 8)` |
| `maxParallelSubAgents` | same |
| `maxParallelMediaJobs` | same |

Unset settings use the CPU-based default; explicit values may exceed 8.

### Scheduling

1. `plan_tool_batch()` builds ordered waves (parallel or serial).
2. Parallel waves run via async join; outcomes merge in original `tool_calls` order.
3. Computer profile and manual-approval batches force full serial execution.

Logs: `tool_batch_plan` and `tool_batch_exec` include mode, wave count, and degrade reason.

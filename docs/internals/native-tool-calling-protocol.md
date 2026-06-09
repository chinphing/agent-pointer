# Native Tool Calling Protocol

This project uses provider-native tool calling as the only runtime protocol
for agent actions. User-facing delivery aligns with OpenClaw: **final replies
are assistant message text**, not a separate delivery tool.

## Contract

- Agents call tools through native `tool_calls`.
- Tool arguments are sent as native function args.
- **User-facing replies** are written as **assistant `content`** on the final
  turn (no standalone `response` tool).
- **IM outbound media:** append `MEDIA:` lines in the final reply, or call
  `channel_message` (`action: send`) — see `docs/guides/channel-integration.md`.
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

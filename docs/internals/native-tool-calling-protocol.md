# Native Tool Calling Protocol

This project now uses provider-native tool calling
as the only runtime protocol for agent actions.

## Contract

- Agents call tools through native `tool_calls`.
- Tool arguments are sent as native function args.
- User-facing replies are sent with tool `response`
  using argument `text`.
- Text-serialized custom envelopes are deprecated
  and must not be emitted by prompts.

## Request/response behavior

- Chat requests include `tools` and `tool_choice=auto`
  when tool use is enabled for the round.
- Stream parsing consumes native `delta.tool_calls`
  and assembles final tool calls by index and id.
- Message replay for next rounds preserves
  `assistant.tool_calls` and `role=tool` messages.

## Compatibility policy

- No fallback to the legacy JSON envelope parser.
- Providers that do not support native tool calling
  are considered unsupported for tool-enabled rounds.
- If such providers are needed, add explicit adapter
  logic before enabling them in production.

## Observability requirements

- Parsing or provider anomalies must produce warnings.
- Tool execution errors must not be silent.
- Keep informative logs around stream parsing,
  malformed args, and protocol mismatch paths.

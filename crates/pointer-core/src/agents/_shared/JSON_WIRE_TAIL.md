[Native tool-calling contract — mandatory]

Use provider-native tool calling.
Do not serialize tool calls as text envelopes.

- For user-visible replies, write assistant
  content directly.
- For operations, call tools directly with
  native tool arguments.
- Use qualified names where required,
  such as `file:read` and `task_board:patch`.

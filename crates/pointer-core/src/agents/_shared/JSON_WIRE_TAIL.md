[Native tool-calling contract — mandatory]

Use provider-native tool calling.
Do not serialize tool calls as text envelopes.

- For user-visible replies, write assistant
  content directly.
- For operations, call tools directly with
  native tool arguments.
- Tools use flat names (e.g. **`file_read`**,
  **`file_write`**, **`file_edit`**,
  **`task_board_patch`**, **`task_board_init`**).
  Call them with their specific parameters —
  no `method` argument.

[Native tool-calling contract — mandatory]

Use provider-native tool calling.
Do not serialize tool calls as text envelopes.

- For user-visible replies, write assistant
  content directly.
- For operations, call tools directly with
  native tool arguments.
- For method-style tools, call the registered name
  and pass **`method`** in **`arguments`**
  (e.g. **`file`** + **`method`: `read`**,
  **`task_board`** + **`method`: `patch`**).

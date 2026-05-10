# general tools

Use to sample a random integer in a half-open range `[min, max)`.

Rules:
- Call when the user needs a random integer, lottery-style pick, or simple numbered choice.
- Semantics: include `min`, exclude `max`.
- Ensure `min < max` before calling.
- Do not use for cryptographic randomness, regulated lotteries, or auditable RNG.

Use to echo arguments for debugging the tool-calling path.

Rules:
- Call only to verify tool invocation or parameter passing.
- Do not use for business logic, command execution, file I/O, or persistence.
- This tool may require user approval—do not call unless the user wants to debug the tool pipeline.

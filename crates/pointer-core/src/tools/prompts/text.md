# text tools

Use for basic text size statistics.

Rules:
- Call when the user needs character, word, line, or byte counts.
- Put the full source text in the `text` argument.
- `chars` counts Unicode scalar values; `bytes` counts UTF-8 bytes.
- Do not use for semantic summarization, language detection, or deep text analysis.

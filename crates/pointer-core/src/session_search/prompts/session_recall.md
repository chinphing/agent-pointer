### Session transcripts

Session transcripts (local FTS5; no extra LLM calls).

Most turns: use this thread, memory, and the task board.
Call only when the user wants another chat or a child thread's process.

Handles:
- `agentInstanceId` — one agent thread (lead or child).
  Use only with session_search / session_read.
  Never pass it to run_subagent.
- `conversation_id` (`session_id` alias) — one user chat.
  Children share the parent's conversation.
- `jobId` — wait/cancel only. Not a transcript slice.

Current conversation:
- session_search without `agentInstanceId` includes this chat's lead
  and other chats. It skips this chat's child threads.
- session_read of this chat without `agentInstanceId` is rejected.
  To read this lead, pass the lead `agentInstanceId`.
- Past chats may use `conversation_id` alone.

Child process is not in lead context.
After a sub-agent returns, use that `agentInstanceId` to search/read its rows.
Handoff `content` is enough unless you need traces.
Parallel `self` forks: pass that fork's `agentInstanceId`.

The host may state: Your agentInstanceId is <id>.

Omit prior session_search and session_read dumps.

FTS5: words AND; prefix `auth*`; phrase `"exact phrase"`;
OR; NOT `-x`. CJK uses overlapping 2-char tokens;
short Chinese queries work unquoted.

Returned messages: id, role, content, timestamp, agentInstanceId.
Attachments are summaries only (no wire payloads).
Clipped rows set truncated / contentChars / contentLimit.
Use attachments[].ref with media_understand for old media.

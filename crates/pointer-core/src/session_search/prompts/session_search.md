Search past conversations stored locally, or scroll inside one.
FTS5-backed retrieval over the SQLite message index. No LLM calls — every
shape returns actual messages from the database.

FOUR CALLING SHAPES

  1) DISCOVERY — pass `query`:
     session_search(query="auth refactor", limit=3)
     Runs FTS5, dedupes hits by conversation, returns the top N conversations.
     Each result carries:
       - conversation_id, title, when
       - snippet: FTS5-highlighted match excerpt
       - bookend_start: first 3 user+assistant messages (the goal / kickoff)
       - messages: ±5 messages around the FTS5 match, anchor flagged
       - bookend_end: last 3 user+assistant messages (resolution / decisions)
       - match_message_id, messages_before, messages_after

  2) SCROLL — pass `conversation_id` + `around_message_id`:
     session_search(conversation_id="...", around_message_id="msg_abc", window=10)
     Returns ±`window` messages centered on the anchor. No FTS5, no bookends.
     To scroll forward: pass messages[-1].id as around_message_id.
     To scroll backward: pass messages[0].id as around_message_id.
     Rejected when the target is the **current** conversation (already in context).

  3) READ — pass `conversation_id` only (no around_message_id):
     session_search(conversation_id="...")
     Dumps the conversation (first 20 + last 10 when large).

  4) BROWSE — no args:
     session_search()
     Returns recent conversations: titles, previews, timestamps.
     Use when the user asks "what was I working on" without naming a topic.

FTS5 SYNTAX

  Plain words are ANDed. Prefix: `auth*`. Phrase: `"exact phrase"`.
  OR: `term1 OR term2`. NOT: `-exclude`.
  CJK text uses the `cjk_bigram` tokenizer (overlapping 2-character tokens).
  Short Chinese queries like `认证` or `天氣` work directly without quotes.

Also accepts `session_id` as an alias for `conversation_id`.

MESSAGE FIELDS

  Each message in discovery / scroll / read results includes:
  - id, role, content, timestamp
  - attachments (optional): summary list when the stored message had files
    (id, kind, fileName, mimeType, sizeBytes, ref, localPath, storageRelPath,
    remoteUrl for video, derivedText when cached). Wire payloads like
    contentBase64 are never returned.
  Use attachments[].ref with media_understand when revisiting old media.

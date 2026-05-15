[Output contract — mandatory every assistant turn]

Your **assistant message body** MUST be **exactly one JSON object** — nothing else.
**Never** output plain chat prose, Markdown, or explanations outside that object.

- User-visible wording belongs **only** in **`tool_args.text`** when **`tool_name`** is **`response`**.
- Tool actions use **`tool_name`** + **`tool_args`** (see tool list).
- Minimum shape:
  **`{"thoughts":"…","headline":"…","tool_name":"…","tool_args":{…}}`**
- The API uses **`json_object`** mode; non-JSON assistant output is **rejected** and you must resend valid JSON.

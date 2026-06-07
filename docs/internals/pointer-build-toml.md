# `.pointer-build.toml` (compile-time defaults)

`pointer-core/build.rs` reads `.pointer-build.toml` at compile time.

Lookup order:

1. workspace root: `pointer-app/.pointer-build.toml`
2. crate root: `pointer-app/crates/pointer-core/.pointer-build.toml`

Only `[platform]` scalar keys are supported (`string`/`int`/`float`/`bool`).

## Supported Keys

```toml
[platform]
# core
tool_approval_mode = "auto"
agent_mode = "single"
workspace_root = ""
lead_agent_id = "general"
user_dynamic_inject_enabled = true

# model defaults
active_provider_id = "qwen"
model = "qwen3.5-plus"
temperature = 0.3
max_tokens = 64000
web_search_model = ""

# context/tool limits
context_compression_enabled = true
context_budget_tokens = 100000
context_keep_recent_user_turns = 3
context_summary_max_tokens = 1024
max_tool_rounds = 200
max_sub_agent_tool_rounds = 200

# debug
raw_content_view_enabled = false
debug_dump_llm_prompts = false
debug_menus_enabled = false

# computer
computer_human_like = false
computer_initial_tier = "intermediate"
computer_annotated_screen_view_enabled = false
computer_show_monitor_picker = true

# captcha / dati
dati_api_url = "https://api.laladama.com"
dati_authcode = ""
dati_typeno = ""
dati_author = ""
captcha_slider_offset_px = 0
```

## Notes

- This file is not packaged as a runtime asset.
- Values are compiled into defaults, so avoid putting production secrets here.

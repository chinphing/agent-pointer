WeCom IM channel attachments are not being delivered. The user can see attachments in the Pointer desktop client, but they don't arrive in WeChat Work (企微). Text messages work fine.

This regression was likely introduced by commit e47aa67 which:
1. Added a path validation check in crates/pointer-channels/src/outbound_resolve.rs:
   - Checks `is_user_filesystem_path(rel) || path_under_app_data(&path)` 
   - If neither is true, bails with "outbound media path outside app data"
2. Changed crates/pointer-core/src/media_generation/mod.rs:
   - format_generation_tool_result now outputs `MEDIA:pointer-media://generated-media/xxx.png` instead of `MEDIA:/absolute/path/file.png`

Please investigate and fix:

ROOT CAUSE ANALYSIS:
1. Read crates/pointer-channels/src/outbound_resolve.rs - check if the new path validation is too strict and rejects valid generated-media or session-sandbox paths
2. Read crates/pointer-core/src/media_generation/mod.rs - check how the MEDIA: output format changed
3. Read crates/pointer-core/src/media/resolve.rs - check how resolve_local_media_path handles pointer-media://generated-media/xxx.png paths
4. Read crates/pointer-core/src/media/store.rs - check path_under_app_data and media_abs_path_unscoped
5. Check if the path validation in outbound_resolve.rs mistakenly rejects paths that are under the app data directory but were canonicalized differently
6. Read crates/pointer-channels/src/im_stream_outbound.rs - check send_media_refs error handling

FIX:
Apply the necessary fix. The most likely fix is either:
- Relaxing the path validation in outbound_resolve.rs (e.g., also allowing paths under session-sandboxes)
- Or fixing the path resolution chain so pointer-media://generated-media/ paths resolve correctly and pass validation

After fixing, do a cargo check to ensure it compiles.

Investigate why iLink getconfig returns Ok(None) - response is valid JSON with ret=0 but no context_token field.

From the logs:
```
[WARN] weixin sendmessage ret=-2; refreshing context_token
[WARN] channel im stream outbound message_end failed: weixin context_token stale (ret=-2); getconfig did not return a token
```

The flow is:
1. sendmessage returns ret=-2 (stale token)
2. retry_after_stale_token calls refresh_via_getconfig(client, user_id, token)
3. refresh_via_getconfig:
   a. fetch_config_token(client, user_id, Some(current_token)) → get_config with token
      - check_ilink_ret passes (ret=0)
      - token_from_getconfig_response returns None (no 'context_token' field in response)
   b. fetch_config_token(client, user_id, None) → get_config WITHOUT token
      - Same result - no context_token in response
4. refresh_via_getconfig returns Ok(None)
5. retry_after_stale_token errors: "getconfig did not return a token"

Please investigate the root cause and fix:

1. ADD RESPONSE LOGGING: In fetch_config_token, log the actual response body when context_token is missing:
   - Log the full response JSON at info level when token_from_getconfig_response returns None
   - This will help us see what format the server actually returns (maybe it's contextToken camelCase, or nested inside a data field)

2. CHECK THE ACTUAL RESPONSE FORMAT:
   - Look at the getconfig response JSON structure
   - The field might be 'contextToken' (camelCase) instead of 'context_token'
   - It might be nested: data.context_token or something similar
   - It might be under a different field name entirely

3. FIX PROACTIVE REFRESH AGE STACKING:
   - In apply_refresh, when Ok(None) is returned, update the cached_at_ms timestamp so the age doesn't keep growing indefinitely
   - Currently, an Ok(None) returns the old token WITHOUT updating the age, causing the age to stack to hundreds of thousands of ms
   - This spams the log with useless proactive refresh warnings

4. FIX getconfig WITHOUT token:
   - When sending getconfig without a context_token, the server should issue a new one
   - But it's not doing so - investigate whether we need additional parameters
   - Maybe we need to send an empty context_token field: "context_token": "" instead of omitting it

5. ADD RESPONSE FALLBACKS:
   - Try alternative field names: contextToken, context_token, data.context_token
   - Try with empty string token instead of None

After fixing, run cargo check -p pointer-channels -p pointer-core to ensure it compiles.

### Scenario: app access

> **Not currently reachable.** The AppAccess family has `needs_verify() == false`
> (`pipeline/operation.rs`), so `computer_pipeline_loop` judges `list_apps` /
> `launch_app` from host text (`parse_app_access_host_outcome`) and returns
> before any Verify LLM call. This file is compiled in but never sent. Kept as
> the spec for that host judgement; if AppAccess ever joins the LLM verify path,
> align it with `parse_app_access_host_outcome` first.

Judge `list_apps` / `launch_app` from tool reply and before/after screenshots.

**Primary evidence:** **[Tool result]**; **[Screen after action]** for window focus.

| Tool | Pass when |
| --- | --- |
| `list_apps` | Result includes app lines matching goal |
| `launch_app` | Target window frontmost **or** tool reports success with **`Verified:`** in reply |

| Fail when |
| --- |
| `FAILED` tool text, host verification failed, wrong app focused, no window after load |

**`loading_detected`:** splash or launch animation still visible.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

1. [Tool result] vs `goal` — list match or launch success/failure text.
2. After screenshot — correct app frontmost when launch; or explain mismatch.
3. Pass, fail, or loading splash.

**Forbidden:** pointer analysis; numbered CoT templates; JSON.

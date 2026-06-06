## Disabled controls (gray / inactive UI)

Before acting, check the control looks **actionable** on **`[Screen after action]`**.
Faded / low-contrast / no-hover styling often means **disabled** — clicks do nothing.

### Control types (all can gray out)

**Click:** button, icon button, split button, link, FAB  
**Pick one / on-off:** checkbox, radio, toggle/switch, dropdown/select, chip/filter  
**Type / value:** text field, textarea, spinner, slider, date-time picker, upload zone  
**Navigate:** menu item, tab, breadcrumb/wizard step, pagination  
**In lists:** row action icons, tree node/chevron, drag handle  
**Other:** rating stars; gray shortcut hint beside a menu row (hint only — item may work)

**Caution:** gray may mean **secondary** (still enabled) — compare siblings and hover.

### Common causes

| Cause | Do first |
|-------|----------|
| Incomplete form | Fix required fields / errors / terms |
| Nothing / wrong selection | Select row/file/item; fix single vs multi-select |
| Workflow order | Finish current step / tab / wizard stage |
| Permission / read-only | Switch mode, account, or path |
| Loading | **`wait`**, re-check; no spam clicks |
| Modal / background window | Close front dialog or focus target app |
| Dependency off | Enable parent toggle/checkbox first |
| Range / empty / lock | Boundary value, empty list, offline/sync/file lock |
| Business rule | Trial, gate, time window — alternate path or ask user |

### Cues

Lighter than neighbors · no hover/press/focus · not-allowed cursor · blocking tooltip.

### Agent rules

- No repeat-click on still-gray controls; no **pass** if unchanged.
- Fix **cause**, not the gray control; match tactic to type (select row → row icon; form → Submit; modal → parent).
- Unclear blocker → clarification or pivot.

# Settings overview

English | [简体中文](../../zh-CN/user/settings.md)

Almost every switch in Pointer lives in **Settings**. There are three ways in:

| What you want to change | Where to go |
| --- | --- |
| Skills / scheduled tasks / connections | Click the matching entry directly in the sidebar workbench |
| Agents (tiers, personalization) | The gear at the top right of the tier popover in the composer |
| Full settings (12 sections) | The **Account** menu at the bottom left → **Settings** (admin account) |

The left sidebar splits the 12 sections into three groups, and three of them only appear once a condition is met:

```
Settings
├─ Agents · Models · System                          ← the everyday ones
├─ Automation · Connections · Skills · Plugins · MCP ← integrations and extensions
└─ Debug* · Cloud host* · Usage · About              ← * only appear once the condition is met
```

## The 12 sections at a glance

| Section | Sidebar description | What it manages |
| --- | --- | --- |
| **Agents** | Tiers and behavior | Which tier each of the three scenes uses, media understanding tiers, web search tiers, personalization rules |
| **Models** | Providers and tier mapping | Platform services and custom services, parameters and capabilities. See [Configuring model services](model-providers.md) |
| **System** | UI, desktop, and runtime | Display, notifications, desktop automation, media generation, runtime, execution, tool permissions |
| **Automation** | Schedules and webhooks | The scheduled task list and creation; webhook sources. See [scheduled-tasks.md](scheduled-tasks.md) |
| **Connections** | WeChat / Feishu / WeCom / DingTalk | IM channel QR or manual setup, session reset, public URL, pairing approval |
| **Skills** | Enable and manage skills | Enable/disable skills per agent, filter by source, import a zip |
| **Plugins** | Manage Pointer plugins | Enable, authorize, uninstall, import from a folder or ZIP |
| **MCP** | External tool servers | Adding, editing and deleting external MCP services, tool list, refresh and restart |
| **Debug** | Save chat requests | Save each turn's request, show raw content, show annotated Computer screenshots |
| **Cloud host** | Purchase and manage | Balance and top-up, purchase, renew, release, open the cloud window. See [cloud-host.md](cloud-host.md) |
| **Usage** | Token usage history | Totals and details by time range |
| **About** | Version and updates | Current version, check for updates |

## Which sections "disappear"

Not everyone has every section — if you cannot see one it is not broken, it just does not apply to you:

| Section / button | Appears when |
| --- | --- |
| **Debug** | Platform admin, **or** a standalone deployment |
| **Cloud host** | **Desktop**, and **not** a standalone deployment |
| **About → "Check for updates"** | **Desktop** on a **managed** build; other builds only show the version number |

The web build is missing one more thing: **webhook triggers** only appear on the server / web, and the desktop's **Automation** does not have them (see [scheduled-tasks.md](scheduled-tasks.md)).

## Agents

**Scene tiers** is the star of this page. The three scenes pick their tier independently, and the tiers from low to high are **Fast → Standard → Expert** (speed from high to low, price from low to high, capability from low to high):

| Scene | What it is for |
| --- | --- |
| **General assistant** | Everyday chat, writing and tool calls |
| **Vibe coding** | Deep refactors, cross-file edits and tests |
| **Computer use** | Desktop actions, browser automation and file handling |

| Element | Description |
| --- | --- |
| Tier radio buttons | One row per scene; the selection takes effect immediately |
| **Models** button | Opens "Tier models": give the Fast / Standard / Expert tiers of that scene a model and reasoning parameters each. A tier marked **Overridden** means its model was changed by hand |
| **Media understanding** | Image understanding / speech transcription / video understanding, one tier each |
| **More tools → Web search** | The model behind each web search tier (compatible search models only) |
| **More** | Tiers and models for the other workers |
| **Personalization** | A block of rules written into every chat's system prompt (`[USER RULES]`), **up to 4000 characters**; leave it empty to use only the product default rules |

"Personalization" suits long-lived constraints like "only change the behavior I explicitly ask for" or "state your assumptions first when something is ambiguous" — it goes into the prompt every time, so the shorter and more precise the better.

## System

Seven blocks, all on one page you scroll down:

| Block | What is in it |
| --- | --- |
| **Display** | Interface language (System / Chinese / English); tool call display: show sidecar tool calls, show non-sidecar tool calls, show tool call cards, show tool call results; agent output: show reasoning, show task board, show sub-agent frames, show child task boards; execution: collapse to status bar while running, collapse process by default |
| **Notifications** | Play sound on finish (a short chime when a chat turn ends) |
| **Desktop automation** | Human-like mouse (curved path with micro-jitter; off is straight uniform motion), follow active display (primary by default, follows the display the window is on after opening apps) |
| **Media generation** | Pick a model for image generation and one for video generation; the default entry shows "Platform default ({model})", and a changed one is marked "Overridden" |
| **Runtime** | **Media understanding**: whether ffmpeg is ready; click **Manage** for details, **Check again**, or **Ask assistant to install**; **Terminal environment**: `KEY→VALUE` overrides for the agent's terminal subprocesses |
| **Execution** | Parallelism and limits, content limits, terminal timeouts — see the tables below |
| **Tool permissions** | **Auto-run** (the AI runs tools straight away) or **Confirm sensitive actions** (ask you first for file, command and similar actions) |

### Execution: parallelism and limits

| Item | Description | Default |
| --- | --- | --- |
| **Tool parallelism** | Run several tools together in one turn; off runs them one by one | On |
| **General tools** | How many file reads, foreground terminals, searches, etc. can run at the same time in one turn (background terminals are not counted) | `min(8, CPU cores)` |
| **Sub-agents** | Cap on sub-agents running at the same time in this session, shared by foreground and background | Same as above |
| **Media tools** | How many image, video and media-understanding jobs run at the same time in one turn | Same as above |
| **Rounds · This turn** | Max consecutive tool calls in this turn | 5000 |
| **Rounds · Subtasks** | Max consecutive tool calls inside each subtask (subtasks are for small scopes; cap 500) | 500 |
| **Task parallelism · Concurrent tasks** | How many independent tasks can run at once; chat, schedules and webhooks share this, and the same session still queues | 4 |

Click **View queue** to see what is pending and waiting right now.

### Execution: content limits and terminal timeouts

Content returned from a tool to the AI is capped — past the cap only the truncated part is kept (for the terminal, the tail):

| Item | What it covers | Default |
| --- | --- | --- |
| **Upload (MB)** | Max size of a file uploaded in a chat, excluding video | 100 |
| **Body (KB)** | Max body returned from a single file read | 64 |
| **Line (bytes)** | Max bytes kept per line when reading a file or searching | 1024 |
| **Search hits** | Max results returned from one search | 50 |
| **Terminal (KB)** | Max terminal output per stream returned to the AI | 16 |
| **Terminal timeout · Idle (sec)** | Used when a command sets no timeout; the command ends after this long with no new output, and output resets the timer | 30 |
| **Terminal timeout · Max runtime (hours)** | Max wall-clock time from start to forced stop | 24 |

> Arguments a tool passes itself **can only be lowered**, never exceed the caps set here.

## Connections

Four tabs: **WeChat / Feishu / WeCom / DingTalk**. For each channel, tick **Enabled** first, then pick a connection method:

| Method | Description |
| --- | --- |
| **Scan to connect** | Confirm by scanning with the matching app (WeChat QR login only works on desktop) |
| **Enter credentials** | Feishu: App ID / App Secret; WeCom: Bot ID / Bot Secret / WSS URL; DingTalk: Client ID (AppKey) / Client Secret; WeChat has no manual entry, QR only |
| **Advanced (Webhook fallback)** | Feishu / WeCom / DingTalk can switch to Webhook callback mode (needs the public base URL below) |

"Shared settings" apply to every channel:

| Item | Description |
| --- | --- |
| **IM outbound** | Controls messages pushed to IM while the agent runs; the final reply is always sent, and tool progress only announces "started" |
| **Session reset (min)** | How long idle before a new session starts, **default 60**, `0` disables it; you can also start a new one by hand in IM with `/new`, `/reset` or "new chat" |
| **Public base URL** | Used to build the platform callback URL when a channel uses Webhook mode |
| **Pairing approval** | With the pairing DM policy, strangers get a pairing code; enter and approve it here |

## Skills, plugins, MCP

| Section | Key points |
| --- | --- |
| **Skills** | The top has three tabs by **worker** (General assistant / Vibe coding / Computer use), each with its own enabled set; filter by source (All / User / Built-in / External / Plugin), search by name, import a zip. Skills that come from a plugin cannot be toggled by hand; the plugin manages them |
| **Plugins** | One status per row: **Unauthorized / Validation failed / Enabled / Disabled / Re-auth required / Degraded**; on desktop you can choose to import a folder or a ZIP file from the "Import plugins" menu. When external plugins are detected a banner offers one-click import |
| **MCP** | When adding a service, pick one of two: **Remote service (http)** with the service address and an optional access token, or **Local program (stdio)** with the launch command (advanced settings add command arguments and environment variables). Each service can **View tools**, and that tool list is what it can offer the AI |

## Debug, Usage, About

| Section | Contents |
| --- | --- |
| **Debug** | Save each chat request (written to a local debug log), raw content view (an extra "View raw content" entry on assistant messages), annotated screenshots (annotated screenshot previews used by Computer use). All three are off by default |
| **Usage** | Time range: Today / Yesterday / Last 7 days / Last 30 days; shows the total plus input and output, and below that a breakdown by time, model and agent |
| **About** | Current version; the managed desktop build also has "Check for updates", and once a new version is found you can "Update now / Later / Skip this version". See [Official packages vs. local builds](which-build.md) |

## FAQ

**"Service ID already exists; choose another"**

Service IDs are unique on this machine in model configuration. Just pick another one; it is only used locally to tell services apart and does not affect calls.

**"Enter service ID, name, and API URL"**

Those three are required for a custom service; you cannot save if one is missing.

**"Provider not found; cancel and edit again"**

The service was deleted while you were editing it. Cancel the dialog, refresh the list and edit it again.

**"Not found" / "Not ready: missing components"**

Media understanding under Runtime is not ready. Click **Manage** for details; you can **Check again**, or **Ask assistant to install** and let it install itself. **IM video and frame extraction depend on a local ffmpeg**; an individual video can still fail to extract frames because of its codec or a corrupt file, and that does not mean the install is broken.

**"No overrides yet. Click Add to create one."**

The terminal environment variables are still empty. Add one `KEY` / `VALUE` row and it persists after saving; it takes priority over the process environment and `.env`.

**The skill toggle will not move**

That skill comes from a plugin. Plugins manage their skills as a set; to change it, deal with that plugin under **Plugins** first.

**"Plugin {name} failed validation and cannot be enabled"**

The plugin did not pass validation and is in the "Validation failed" state, so it cannot be enabled. Import a version that passes validation, or just uninstall it.

**Changed a setting and nothing happened**

Two kinds: language and display switches take effect **immediately**; model service parameters take effect **after saving**. Execution numbers (caps, timeouts) also take effect after saving.

## Related

- [Configuring model services](model-providers.md) — where models come from and how to add your own
- [scheduled-tasks.md](scheduled-tasks.md) — let the agent run on a schedule
- [cloud-host.md](cloud-host.md) — buy an instance running in the cloud

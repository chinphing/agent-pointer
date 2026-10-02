# UI and frontend
English | [简体中文](../../zh-CN/ui/README.md)

| Document | Description |
|------|------|
| [i18n.md](../../zh-CN/ui/i18n.md) | UI Chinese/English internationalisation: `uiLocale`, vue-i18n, locale file conventions and phased progress |
| [visual-theme.md](../../zh-CN/ui/visual-theme.md) | Flat theme tokens, light/dark switching; the conversation column width follows the middle column (including Workspace dragging) adaptively |
| [markdown-typography.md](../../zh-CN/ui/markdown-typography.md) | Chat / workspace Markdown body font size, heading levels, bold-as-heading |
| [external-links.md](../../zh-CN/ui/external-links.md) | In-app http(s) links open in the system default browser (desktop) / a new tab (web) |
| [markdown-charts.md](../../zh-CN/ui/markdown-charts.md) | Markdown `chartjs` fence → local interactive Chart.js charts |
| [markdown-mermaid.md](../../zh-CN/ui/markdown-mermaid.md) | Markdown `mermaid` fence → local Mermaid, coloured by the light/dark tokens |
| [markdown-svg.md](../../zh-CN/ui/markdown-svg.md) | Markdown `svg` fence → sanitised inline flowcharts / diagrams |
| [markdown-media-boundaries.md](../../zh-CN/ui/markdown-media-boundaries.md) | Markdown / SVG / Chart / HTML feature boundaries and the HTML table conventions |
| [task-complete-sound.md](../../zh-CN/ui/task-complete-sound.md) | Task-complete sound (account setting `playSoundOnFinish`) |
| [turn-elapsed.md](../../zh-CN/ui/turn-elapsed.md) | Turn "elapsed"; identical to the pre-addition behaviour when "collapse execution by default" is off |
| [web-branding-welcome-elapsed.md](../../zh-CN/ui/web-branding-welcome-elapsed.md) | Server-customisable welcome tip / elapsed prefix (default copy unchanged) |
| [sidebar-conversation-select.md](../../zh-CN/ui/sidebar-conversation-select.md) | Sidebar click-select: commit `currentId` first, the main area loads asynchronously |
| [last-conversation-restore.md](../../zh-CN/ui/last-conversation-restore.md) | Restore the last selected conversation on start (`pointer.chat.lastConversationId`) |
| [sidebar-awaiting-view.md](../../zh-CN/ui/sidebar-awaiting-view.md) | Background completion not yet viewed: a solid dot in the sidebar |
| Settings → **Account** | Balance, sign-in state, "Sign out"; task-complete sound toggle |
| [assistant-message-ui.md](../../zh-CN/ui/assistant-message-ui.md) | Assistant message `thoughts` / reasoning / raw output; consecutive tool calls collapsed by default |
| [attachment-file-icons.md](../../zh-CN/ui/attachment-file-icons.md) | Attachment chips pick common document icons by extension (xlsx / pdf / zip, etc.) |
| [mobile-chat.md](../../zh-CN/ui/mobile-chat.md) | Mobile: hide avatars and always show time/copy; the Composer has only attachment + send; welcome page input at the bottom, no slogan/experience section |
| [message-list-layout-cache.md](../../zh-CN/ui/message-list-layout-cache.md) | Structural fingerprint caching for completed turns in the message list; only the tail is recomputed while streaming |
| [message-list-scroll-follow.md](../../zh-CN/ui/message-list-scroll-follow.md) | Streaming output sticks to the bottom: scrolling up detaches, sticking to the bottom / the button restores |
| [message-turn-pagination.md](../../zh-CN/ui/message-turn-pagination.md) | Messages paginated by user turn: scroll to the top for earlier, scroll to the bottom after `around` for newer, jump to latest |
| [tool-payload-memory.md](../../zh-CN/ui/tool-payload-memory.md) | Terminal output and tool bodies for the current turn: only a summary is kept in memory, fetched on expand |
| [conversation-nav.md](../../zh-CN/ui/conversation-nav.md) | Short bar navigation on the right edge of the main area: all user messages, click to locate via `around` |
| [streaming-markdown-throttle.md](../../zh-CN/ui/streaming-markdown-throttle.md) | Streaming Markdown render throttling: 100ms by default, 250ms for long text |
| [subagent-stream-ui-perf.md](../../zh-CN/ui/subagent-stream-ui-perf.md) | Streaming UI batching and SubAgentFrame relief with many concurrent sub-agents |
| [background-job-ui-consistency.md](../../zh-CN/ui/background-job-ui-consistency.md) | Background job host state / occupancy reconciliation / nested title and statistics consistency |
| [workspace-file-preview-mode.md](../../zh-CN/ui/workspace-file-preview-mode.md) | Right-hand workspace raw / preview shared mode (registered by extension) |
| [workspace-file-preview-find.md](../../zh-CN/ui/workspace-file-preview-find.md) | Right-hand workspace text preview find (⌘/Ctrl+F, previous/next match) |
| [workspace-file-preview-json.md](../../zh-CN/ui/workspace-file-preview-json.md) | Right-hand workspace collapsible JSON preview (hooked onto the shared raw / preview) |
| [workspace-file-preview-html.md](../../zh-CN/ui/workspace-file-preview-html.md) | Right-hand workspace sandboxed HTML preview (hooked onto the shared raw / preview) |
| [workspace-panel-refresh.md](../../zh-CN/ui/workspace-panel-refresh.md) | Refresh conventions when the right-hand workspace opens / switches conversation / switches tab; file tree find |
| Turn change summary | The footer of every turn says "N files changed", not above the input box. See [`turn-change-summary.md`](../../zh-CN/ui/turn-change-summary.md); for the baseline diff see [`../../developer/turn-file-baseline-review.md`](../../zh-CN/developer/turn-file-baseline-review.md) |
| Sidebar conversation search | The second line shows a snippet around the matched keyword; clicking a body hit locates and briefly highlights the corresponding message. See [`../../internals/sidebar-conversation-search.md`](../../zh-CN/internals/sidebar-conversation-search.md) |
| New-conversation empty state | "Popular" and category tabs side by side (`GET /api/experiences/home`); search keyword (`GET /api/experiences?q=`); clicking switches the bound agent and fills `prompt_text` into the input box |
| Experience cover image | Welcome page cards have two columns: a title bar (title on the left + a **24×24** badge on the right, vertically centred); a content column (the body takes the full row); the **recommended experiences** block is collapsed by default and expands on clicking the title bar; uploads must be at least **200×200** pixels, 1:1 recommended |
| Experience card text limits | Titles at most **15** characters, descriptions (`narrative_text` / excerpt) at most **45** characters; consistent with the official-site create/edit form and API validation (`experience_card_limits`) |

[Back to the documentation index](../README.md)

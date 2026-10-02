# UI and workspace

English | [简体中文](../../zh-CN/user/workspace.md)

The UI has four parts: on the left the **sidebar** (projects and chats), in the middle the **chat** (header + message stream + composer), on the right the **workspace panel** (files, Git changes, terminal).

The **workspace** is the local folder this chat is bound to — the agent's reads and writes and the terminal commands all happen inside it.

```
┌──────────────┬────────────────────────────────────┬────────────────────┐
│ Sidebar      │ Chat header: project / workspace   │ Workspace panel*   │
│  ├ Workbench ├────────────────────────────────────┤  ├ Workspace files │
│  ├ Projects  │ Messages                           │  ├ Changed files   │
│  └ Recent    │                                    │  └ Debug terminal  │
│              ├────────────────────────────────────┤                    │
│              │ Composer: attachments / agent / tier                    │
└──────────────┴────────────────────────────────────┴────────────────────┘
   * Desktop + admin account + window width ≥ 1024px only
```

## Sidebar

At the top are the **four Workbench entries**, one click to a common place:

| Entry | Goes to |
| --- | --- |
| **New task** | Start a new chat (created under the project when one is selected) |
| **Scheduled tasks** | Settings → Automation |
| **Skills** | Settings → Skills |
| **Connections** | Settings → Connections |

Below come projects, pinned and recent:

| Area | What you can do |
| --- | --- |
| **Projects** | Hover shows a magnifier for "Search projects"; `+` is "Add project" (project name and project folder); the `⋯` menu on each row has "New local task", "Pin project / Unpin project", "Project settings", "Delete project" |
| **Chats** | The hover magnifier is "Search chats" (searches content; hits show "{n} matches"); double-click the title to rename; pin or delete on the right of the row |
| **Recent** | Chats that belong to no project |

A chat row's state is obvious at a glance: a spinner = running; a solid dot + "New completion" = finished in the background and not yet viewed.

**Right-clicking a chat** gives four more actions:

| Menu item | Description |
| --- | --- |
| Pin / Unpin | Move it to the **Pinned** section |
| Rename | Same as double-clicking the title |
| Copy chat ID | Handy for troubleshooting and matching logs |
| Copy workspace path | Copy the folder path this chat is bound to |
| Reveal in Finder | Open the workspace folder (desktop only; unavailable when the folder is empty) |

Deleting a chat takes **two steps**: click "Delete" and "Confirm delete" and "Cancel" appear in place; click again to actually delete — no system dialog.

Bottom left are "**Current desktop**" (view a desktop screenshot) and the "**Account**" menu (Settings, Appearance, Balance, Sign in / Sign out).

## Chat header: project and workspace

The middle of the header is the **project / workspace selector**, which decides which folder this chat works in:

| State | Behaviour |
| --- | --- |
| Nothing selected yet | Shows "Select project" |
| Selected | Shows the project name, or "Default project" |
| **After the first message** | **Locked** — the button greys out with the tooltip "Project locked" |

**The lock is deliberate**: once a chat has started running the folder cannot change, or the file context of later turns would not match earlier ones. Start a new task to change folders.

In the selector you can pick "Local folder" (the desktop opens a folder picker) or an existing project; on the web only a path input shows (the web header has no project selector — switch projects from the sidebar).

> One gotcha: **clicking the already-selected project = deselect** (clears the workspace), not "re-select".

"**Open workspace**" on the right of the header expands the right-hand panel (shown for desktop + admin account only).

## Composer

| Element | Description |
| --- | --- |
| **Attach** | Paperclip button, drag and drop, or paste an image; progress shows while uploading, and a failure can be retried |
| **Agent** | Pick which executing agent runs this turn (General / Vibe coding / Computer use, …) |
| **Tier** | Fast / Standard / Expert; the gear next to it jumps to the **Agents** settings page to change tier models |
| **Send / Stop** | Send when idle; turns into Stop while running (tooltip "Stop this turn and cancel all background jobs") |

Messages sent while generating are not lost — they go into the **send queue** (shown as "Queued" in the panel) and are sent in order once the turn ends. In the queue panel you can "Remove from queue" or "Force send".

While background jobs are still running, a "Still running in background" panel appears above the composer, where you can "Cancel all background jobs" or end just one.

## Workspace panel

The right column has three tabs:

| Tab | Content |
| --- | --- |
| **Workspace files** | File tree. The search box is "Find files"; right-clicking a file gives "Preview file", "Open with default app" (desktop), "Copy absolute path", "Copy relative path", "Reveal in Finder", "Refresh file tree", "Delete"; deletion asks for confirmation |
| **Changed files** | Git change list with a change-count badge; right-click to "View Diff" or preview. Shows "No Git changes" when there are none |
| **Debug terminal** | A real terminal (xterm). You can create a "New Shell", split panes left/right or top/bottom, restart or kill a Shell; each tab has its own folder and environment |

Previews open as tabs in the panel so you can view several side by side; a notice like "Showing first 1 MB only" means the content was truncated.

If the folder is not a Git repository yet, the Changes tab offers an "Ask Pointer to init a Git repo" button — no need to type the command yourself.

## What you see in the message stream

| Element | Description |
| --- | --- |
| **Tool cards** | One card per tool call, with states Running / Done / Failed / Cancelled / Rejected / Waiting for your input / Waiting for approval; expand to see arguments and results |
| **Collapsed process** | Consecutive calls are folded into a group; the heading offers "Expand process / Collapse process" |
| **Sub-agent frame** | Subtasks are boxed separately ("Subtask process") and can nest |
| **Thinking panel** | "Thinking process" expands and collapses |
| **Raw output** | To see the model's raw response, expand "Raw output": reasoning, raw body channel and tool call arguments |
| **Copy** | "Copy" at the bottom right of every message; it becomes "Copied" |
| **Images** | Click to zoom (wheel to zoom · drag to pan · double-click to reset · Esc to close) |
| **Audio** | Playable; clips with a transcript show the text |
| **Media gallery** | Generated images / videos offer "Download", "Open" and "Copy file path" |
| **Task board bubble** | Complex tasks show progress ({done}/{total} milestones); expand to see the steps |
| **Change summary** | When files changed during a turn, shows "Modified {n} files" |

## Find in chat and the chat nav bar

| Feature | How to use |
| --- | --- |
| **Find in chat** | `⌘/Ctrl+F` searches loaded messages and shows a count like "1/3"; `Enter` next, `Shift+Enter` previous, `Esc` close |
| **Chat nav bar** | The thin vertical bar on the right of the chat, one tick per turn of yours; turns with a milestone are a **diamond**. Hover for a preview, click to jump |

Three places claim `⌘/Ctrl+F`: find in chat, the workspace **file tree** find, and find inside a file preview. Whichever has the cursor / focus wins — pressing it in the workspace panel goes to the file tree or the preview first.

## Compact mode

When **Computer use** runs a desktop task, the window shrinks into a **compact status bar** (a Dock-style strip that keeps only the status and "Stop" / "Expand") to give the screen to the software being operated. It restores automatically when the task ends or you switch chats.

This is **desktop** behaviour; the web does not shrink.

## Mobile

On narrow screens (width < 768px):

| Change | Description |
| --- | --- |
| Sidebar hidden | Narrow screens show no sidebar and no expand button in the header — mobile centres on a single chat |
| Workspace panel | Hidden below 1024px |
| Composer | The paperclip moves to the left of the input box, and the agent / tier toolbar collapses |
| New chat button | For very long chats (more than 30 LLM calls) a "New chat" button appears at the bottom right; clicking it confirms "You will switch to a new chat. The current chat stays in your history and will not be lost." |

## FAQ

**"Project locked"**

This chat already has messages, so the workspace cannot change. Start a new task to change it.

**"Folder picker is unavailable; enter the path manually"**

The web has no system folder picker. Type the path, or use the desktop.

**"Workspace path is empty" / "Reveal in Finder is not available on the web"**

"Reveal in Finder" needs the desktop and a chat bound to a folder.

**"Not a Git repository"**

The Changes tab relies on Git. Click "Ask Pointer to init a Git repo", or run `git init` yourself.

**"Select a project folder in the composer to show workspace files and Git changes here."**

No workspace is selected yet. Pick a folder with the project / workspace selector in the header.

**"No matching chats"**

Chat search looks at content. Try another keyword, or clear the search to see everything.

**⌘/Ctrl+F does nothing**

When the cursor is in the terminal, `Ctrl+F` belongs to the terminal; find in chat only works on loaded messages and is not available on the welcome page.

## Related

- [Getting started](getting-started.md) — your first chat and shortcuts
- [Settings overview](settings.md) — UI display, execution limits, terminal timeouts
- [Using Skills](skills.md) — what to do once skills are in the workspace

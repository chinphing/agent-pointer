# 侧边栏项目导航

## Persisted model

Projects are persisted independently from conversations. A project has a
stable id, name, workspace root, default flag, pin state, and archive state.
`Conversation.projectId` is the ownership link; the workspace root remains
the execution directory for the conversation.

In a new conversation, choosing a raw workspace directory creates or reuses
the project for that normalized directory. The composer keeps that project
selection pending, then persists conversation ownership before the first
message is dispatched. Desktop uses the directory picker; Web confirms a
typed directory with Enter.

On first startup after this feature is introduced, migration is gated by the
absence of the `projects` table, not by `schema_version`. It creates **per-user**
default projects from each distinct non-empty `conversations.session_user_id`
(sandbox `{session-sandboxes}/{session_user_id}/`), plus a legacy empty-owner
default when needed, attaches historical conversations by matching owner +
workspace root, and deduplicates other roots into projects. Cron, webhook, and IM-generated sessions remain
outside project navigation.

## Visibility (multi-user)

Conversation metas, sidebar search, and project list/get use `ListScope`
(see [session-user-id.md](../developer/session-user-id.md)): platform admins
see every user's rows; other users only see their own `session_user_id`.

## Sidebar behavior

- The existing **+** is the sole new-task action. It attaches a task to the
  current project when one is active, otherwise to the default project.
- Selecting a project **name** activates the project and expands it when collapsed;
  it does not collapse an already-expanded project. The **chevron** alone toggles
  expand/collapse. Expand state is remembered in `localStorage`
  (`pointer.sidebar.expandedProjectIds`) across reloads; deleted project ids are
  pruned from that set.
- Expanded projects query only their own conversations, with the same
  cursor-based paging model as recent conversations.
- Sidebar projects are queried separately from recent conversations. Initial
  load returns at most five projects total (`loadProjects` first page, same as
 「加载更多项目」). `hasMoreProjects` comes from `nextCursor` on that page, so the
  button is hidden when there is no further page (no need to click once to find out).
  Pinned projects remain first;
  projects with the same pin state are ordered by their newest conversation
  activity, then by id for stable pagination. Empty projects use creation time
  as their activity fallback.
  Loading more uses cursor pagination across both pinned and unpinned projects.
  The project list viewport remains five project rows tall and scrolls
  internally after more projects or nested conversations are loaded.
- Top-level actions are ordered **New task → Scheduled tasks → Skills →
  Connections**.
- **Scheduled tasks** opens the existing Automation settings section;
  **Skills** opens the skill manager; **Connections** opens channel settings.
- Pinned conversations appear in a dedicated **置顶** section above projects.
  The section is hidden when there are no pinned conversations. The section
  header can collapse independently (same pattern as Projects / Recent).
  Pinned rows show title only (no timestamp). **All** pinned metas are loaded
  at boot (meta pages continue while the trailing row is still pinned); the
  list viewport is five rows tall (`max-h` matching project rows) and scrolls
  internally when there are more than five. Pinning does not bump `updatedAt`.
  Pinned rows are excluded from the recent list and from nested project task
  lists to avoid duplicates.
- Conversation rows support a right-click menu: pin/unpin, rename, copy session
  id, copy workspace directory, and (desktop only) reveal workspace in Finder.
  Empty workspace actions are disabled. Web hides the Finder action.
- The conversation list (**最近**) sits below the project list and is
  ordered by `updatedAt` descending (stable id tie-break). Like projects and
  search, it is scoped by `ListScope` (admin sees all users; others only their
  own `session_user_id`). The section title
  (and search / new-task actions) stays **outside** the scroll container so it
  does not scroll away; only the conversation rows scroll.
  Recent rows show title and optional snippet only (no timestamp).
  Conversation pin state is persisted on the conversation meta row (`isPinned`).
  Double-click a title to rename in a dialog.
- Opening a recent conversation whose project is outside the loaded sidebar
  fetches that project by id for Composer context only. This lookup does not
  insert or highlight the project in the sidebar.
- Conversation search is collapsed by default on the right side of the
  **Recent conversations** header, matching project search behavior.
- The **Pinned**, **Projects**, and **Recent conversations** sections can each
  be collapsed independently from their header. Section collapse and the whole
  sidebar collapsed flag are restored from `localStorage` on next open
  (`pointer.sidebar.sectionCollapse`, `pointer.sidebar.collapsed`); project
  expand uses `pointer.sidebar.expandedProjectIds`. The last selected
  conversation id is restored on next open via
  `pointer.chat.lastConversationId` (see
  [`../ui/last-conversation-restore.md`](../ui/last-conversation-restore.md)).

Deleting a project is rejected while it owns conversations, so no operation can
silently orphan conversation history. The default project cannot be deleted.

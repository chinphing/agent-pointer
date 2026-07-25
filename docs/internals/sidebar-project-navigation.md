# Sidebar project navigation

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
absence of the `projects` table, not by `schema_version`. It creates one
default project from the global workspace root, attaches historical desktop
conversations with an empty or matching root to it, and deduplicates all other
workspace roots into projects. Cron, webhook, and IM-generated sessions remain
outside project navigation.

## Sidebar behavior

- The existing **+** is the sole new-task action. It attaches a task to the
  current project when one is active, otherwise to the default project.
- Selecting a project header only expands or collapses it; it never opens or
  creates a conversation.
- Expanded projects query only their own conversations, with the same
  cursor-based paging model as recent conversations.
- Sidebar projects are queried separately from recent conversations. Initial
  load returns at most five projects total, with pinned projects first.
  Loading more uses cursor pagination across both pinned and unpinned projects.
  The project list viewport remains five project rows tall and scrolls
  internally after more projects or nested conversations are loaded.
- Top-level actions are ordered **New task → Scheduled tasks → Skills →
  Connections**.
- **Scheduled tasks** opens the existing Automation settings section;
  **Skills** opens the skill manager; **Connections** opens channel settings.
- The conversation list remains global and time ordered below the project list.
- Conversation search is collapsed by default on the right side of the
  **Recent conversations** header, matching project search behavior.
- The **Projects** and **Recent conversations** sections can each be collapsed
  independently from their header.

Deleting a project is rejected while it owns conversations, so no operation can
silently orphan conversation history. The default project cannot be deleted.

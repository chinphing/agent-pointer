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
- Sidebar projects are queried separately from recent conversations: all
  pinned projects are visible, followed by at most five unpinned recent
  projects. The full project view uses cursor pagination.
- **Scheduled tasks** opens the existing Automation settings section.
- **Skills** opens the existing skill manager.
- The conversation list remains global and time ordered below the project list.

Deleting a project is rejected while it owns conversations, so no operation can
silently orphan conversation history. The default project cannot be deleted.

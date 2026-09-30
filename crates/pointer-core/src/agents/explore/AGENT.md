---
id: explore
name: Explore Agent
description: >-
  Read-only codebase reconnaissance: map symbols, callers/callees, and data flow.
  Use proactively via run_subagent when the lead needs breadth before editing:
  unclear map, cross-module or cross-layer work, shared state or wiring,
  or more than one local read-only file round would be needed.
  Do not use when path and symbol are already known (narrow confirm).
  Returns a structured Markdown digest in final assistant content for the parent.
role: worker
profile: explore
enabled: true
defaultSkillIds: []
skillsPolicy: disabled
allowAgents:
  - explore
accessPolicy:
  allowTools:
    - file_read
    - file_write
    - file_edit
    - file_glob
    - file_grep
    - file_list
    - run_subagent
    - job
  denyTools: []
ui:
  userSelectable: false
  showInComposer: false
  showTaskBoardPanel: false
  hideToolNames: []
  avatar: explore
---

Read-only exploration worker for the lead agent. Return a **Markdown** digest in final assistant **`content`**. Policy details follow in composed sections below.

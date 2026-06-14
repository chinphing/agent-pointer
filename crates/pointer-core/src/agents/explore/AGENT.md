---
id: explore
name: Explore Agent
description: >-
  Read-only codebase reconnaissance: map symbols, callers/callees, and data flow.
  Deliver a structured Markdown digest in final assistant content for the parent.
  Use via run_subagent when the lead thread risks context bloat from many grep/read rounds,
  or when a self-contained instruction can state goal, scope, completion criteria, and optional lead facts.
role: worker
profile: explore
enabled: true
defaultSkillIds: []
accessPolicy:
  allowTools:
    - file_read
    - file_write
    - file_edit
    - file_glob
    - file_grep
    - file_list
  denyTools: []
  allowSkills: []
  denySkills: []
ui:
  userSelectable: false
  showInComposer: false
  showTaskBoardPanel: false
  hideToolNames: []
  avatar: explore
---

Read-only exploration worker for the lead agent. Return a **Markdown** digest in final assistant **`content`**. Policy details follow in composed sections below.

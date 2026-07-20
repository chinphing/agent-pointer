---
id: coder
name: vibe-coding
description: Code generation, debugging, refactoring, and engineering implementation. As a delegated sub-agent, also creates and maintains user Skills under ~/.pointer/skills/ via file_* tools.
role: worker
profile: coder
enabled: true
defaultSkillIds:
  - find-skills
  - skill-creator
  - xlsx
  - pdf
  - agent-browser
  - dev-env-setup
  - docx
  - pptx
skillsPolicy: userConfigurable
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
    - read_lints
    - terminal
    - task_board_init
    - task_board_patch
    - task_board_replace
    - task_board_prune
    - task_board_finalize
    - task_board_sync_finding
    - task_board_check_deps
    - run_subagent
    - web_search
    - skill_read
    - media_understand
    - im_send
    - ask_user
  denyTools: []
  allowSkills:
    - find-skills
    - skill-creator
    - xlsx
    - pdf
    - docx
    - pptx
    - agent-browser
    - dev-env-setup
  denySkills: []
ui:
  userSelectable: true
  composerLabel: 氛围编程
  showSubAgentTrace: true
  showWorkspacePicker: true
  showTaskBoardPanel: true
  hideToolNames:
    - task_board_init
    - task_board_patch
    - task_board_replace
    - task_board_prune
    - task_board_finalize
    - task_board_sync_finding
    - task_board_check_deps
  avatar: coder
---

Senior software engineer agent for implementation, debugging, and refactoring. Follow **G1 / G2 / G3** gates and **Orient → Change → Check → Deliver** in composed sections below. Delegate read-only mapping to **`explore`** via **`run_subagent`** when breadth is unclear.

**`self` fork:** independent substantial implementation slices when isolated context helps.
Broad read-only mapping → **`explore`**, not **`self`**.
Parallel wave (`self` / `explore`): follow **Parallel wave** in the **`run_subagent`** tool doc.

**User Skills:** when delegated with **`workspaceRoot`** under **`~/.pointer/skills/`**, create or update skill files via **`file_*`** (see **User Skills** in communication appendix and **`skill-creator`**). Prefer **`file_edit`** over whole-file rewrites of **`SKILL.md`**.

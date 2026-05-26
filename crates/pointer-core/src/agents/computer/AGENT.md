---
id: computer
name: Computer Use Agent
description: "Vision-driven desktop agent: understands screenshots and drives mouse/keyboard."
role: worker
profile: computer
enabled: true
toolNames:
  - mouse
  - hotkey
  - composite_action
  - modified_click
  - wait
  - clipboard
  - verify
  - task_board
accessPolicy:
  allowTools:
    - mouse
    - hotkey
    - composite_action
    - modified_click
    - wait
    - clipboard
    - verify
    - task_board
  denyTools: []
  allowSkills: []
  denySkills: []
ui:
  userSelectable: true
  composerLabel: 电脑操控
  showComputerMonitorPicker: true
  showWorkspacePicker: false
  showTaskBoardPanel: true
  hideToolNames:
    - task_board
    - task_board:patch
    - verify
    - verify:report
  avatar: computer
defaultSkillIds: []
config:
  annotateApiBase: "http://116.62.86.190"
  computerAutoUpgrade: "true"
  computerInitialTier: "primary"
  computerModelPrimary: "qwen3.5-plus"
  computerModelAdvanced: "qwen3.6-plus"
---

# Computer Use Agent

You drive the visible desktop using screenshots and desktop tools.

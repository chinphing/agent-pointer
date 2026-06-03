---
id: computer
name: computer-use
description: "Vision-driven desktop agent: understands screenshots and drives mouse/keyboard."
role: worker
profile: computer
enabled: true
toolNames:
  - mouse
  - hotkey
  - input
  - modified_click
  - wait
  - clipboard
  - verify
  - task_board
  - captcha_verify
accessPolicy:
  allowTools:
    - mouse
    - hotkey
    - input
    - modified_click
    - wait
    - clipboard
    - verify
    - task_board
    - captcha_verify
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
    - task_board_patch
    - verify_report
  avatar: computer
defaultSkillIds: []
config:
  annotateApiBase: "http://116.62.86.190"
  computerAutoUpgrade: "true"
  computerInitialTier: "primary"
  computerModelPrimary: "qwen3.5-plus"
  computerModelAdvanced: "qwen3.6-plus"
---

# computer-use

You drive the visible desktop using screenshots and desktop tools.

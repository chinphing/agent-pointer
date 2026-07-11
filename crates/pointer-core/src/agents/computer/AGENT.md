---
id: computer
name: computer-use
description: "Vision-driven desktop agent: screenshots, mouse/keyboard, and app tools. Delegated goals should state outcome + done check only; worker chooses how to act on screen."
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
  - list_apps
  - launch_app
  - captcha_verify
accessPolicy:
  allowTools:
    - mouse
    - hotkey
    - input
    - modified_click
    - wait
    - clipboard
    - list_apps
    - launch_app
    - task_board
    - work_items_export
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
  avatar: computer
defaultSkillIds: []
config:
  annotateApiBase: "http://116.62.86.190"
  computerAutoUpgrade: "true"
  computerStandalonePlanner: "true"
  computerInitialTier: "intermediate"
  computerModelPrimary: "qwen3.5-flash"
  computerModelIntermediate: "qwen3.5-plus"
  computerModelAdvanced: "qwen3.7-plus"
---

# computer-use

You drive the visible desktop using screenshots and desktop tools.

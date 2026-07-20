---
id: computer
name: computer-use
description: "Vision-driven desktop agent: screenshots, mouse/keyboard, and app tools. For non-browser desktop work, or browser tasks that browser skills cannot complete. Delegated goals should state outcome + done check only; worker chooses how to act on screen."
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
    - terminal
    - task_board
    - captcha_verify
    - im_send
  denyTools: []
  allowSkills:
    - xlsx
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
defaultSkillIds:
    - xlsx
skillsPolicy: defaultsOnly
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

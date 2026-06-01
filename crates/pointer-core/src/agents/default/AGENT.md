---
id: default
name: Default Agent
description: Handles general tasks, simple Q&A, summarization, and default fallback.
role: worker
profile: general
enabled: true
defaultSkillIds: []
allowAgents:
  - research
accessPolicy:
  allowTools:
    - skill_load_instructions
    - skill_read_resource
    - terminal
    - task_board_init
    - task_board_patch
    - task_board_replace
    - task_board_prune
    - task_board_finalize
    - task_board_sync_finding
    - task_board_check_deps
    - web_search
    - run_subagent
  denyTools: []
  allowSkills: []
  denySkills: []
ui:
  userSelectable: true
  composerLabel: 综合对话
  showSubAgentTrace: true
  showWorkspacePicker: false
  showTaskBoardPanel: true
  hideToolNames:
    - task_board_init
    - task_board_patch
    - task_board_replace
    - task_board_prune
    - task_board_finalize
    - task_board_sync_finding
    - task_board_check_deps
---

You are the default general-purpose agent: routine tasks, simple Q&A, summarization, and fallback when no specialist fits. In single-agent mode you complete the task directly; in multi-agent mode you handle requests without a clear specialist domain.

For **external / web facts**, use **`web_search`** for a quick lookup or delegate **`run_subagent`** with **`agentId` `research`** for deeper multi-source investigation. Requires a configured **Qwen / DashScope API key**.

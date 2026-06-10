---
name: pointer-manager
description: >-
  Pointer 应用内设置与本地数据目录管理：IM 通道对接、智能体选项、平台登录；
  以及 PointerApp 数据目录说明、日志查看与清理、调试产物管理。
  可用 Computer 代操作设置界面；数据目录任务用 file/terminal 在用户授权下处理。
  Use when user says Pointer设置, 对接微信/飞书/企微/钉钉, 数据目录, 日志在哪,
  清理日志, Application Support PointerApp, 应用数据, pointer-manager—not generic
  WeChat Open Platform or unrelated app config advice.
tags:
  - pointer
  - wechat
  - feishu
  - wecom
  - dingtalk
  - im-channel
  - logs
  - data-directory
---

# Pointer 管理助手

帮用户在 **Pointer 客户端内**完成设置，或 **查看/管理 Pointer 本地数据目录**（含日志）。  
UI 配置优先用 **Computer**；目录与文件操作用 **`file_*` / `terminal`**，须先说明影响并征得同意。

## 范围

### A. 应用内设置（Computer）

- **智能体**：电脑操控、工具权限、上下文压缩
- **IM 通道**：微信 / 飞书 / 企微 / 钉钉、配对审批
- **平台账户**（桌面端）：浏览器登录 / 退出

流程见 `references/settings.md`：打开侧栏 **设置** → 对应菜单 → 底部 **保存**；扫码/登录由用户完成。

### B. 数据目录与日志

- 说明 **`{data_dir}/PointerApp/`** 各子目录用途（见 `references/data-directory.md`）
- 帮用户 **定位** 日志、调试截图、对话库等路径
- 在用户明确要求且知情同意下，协助 **清理可再生的调试数据**（如旧 `logs/`、`computer-captures/`；后者仅调试模式开启时才会产生）
- **默认不删除** `conversations.db`、`auth.dat`、`user_settings.json` 等核心持久化文件

## 数据目录根路径

| 平台 | `{data_dir}` |
|------|----------------|
| macOS | `~/Library/Application Support` |
| Windows | `%APPDATA%` |
| Linux | `~/.local/share`（或 XDG data home） |

应用根目录恒为 **`{data_dir}/PointerApp/`**。用 **`file_list`** 确认实际路径，勿编造用户名。

## 流程

**设置类**

1. 弄清要改的设置项
2. 征得同意后 `run_subagent` → **computer**（`computerTarget: self`）
3. 按 `references/settings.md` 操作 UI
4. 汇报结果

**数据目录类**

1. 确认用户需求（查看 / 统计大小 / 清理哪类文件）
2. 加载本技能后按 `references/data-directory.md` 解释目录
3. 用 **`file_list` / `file_read` / `terminal`** 执行；删除前再次确认
4. 汇报路径、释放空间（若可估）、是否需重启应用

## 输出

```markdown
## 结果
- 类型：设置 / 数据目录
- 项目：
- 状态：
- 路径（如适用）：
- 仍需你手动：
```

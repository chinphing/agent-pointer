---
name: pointer-manager
description: >-
  Pointer in-app settings and local data directory: connections (WeChat, Feishu, WeCom,
  DingTalk), agent tiers/models, system options, automation, skills/plugins/MCP; PointerApp
  data directory, logs, debug artifacts. Use Computer for settings UI; file/terminal for
  directory tasks with user consent. Use when the user mentions Pointer settings,
  connect/configure IM channels, data directory, log location, clear logs,
  Application Support PointerApp, pointer-manager—not generic WeChat Open Platform or
  unrelated app config. When matched, call skill_read first; do not answer from the skills
  index alone.
tags:
  - pointer
  - settings
  - wechat
  - feishu
  - wecom
  - dingtalk
  - im-channel
  - automation
  - mcp
  - plugins
  - logs
  - data-directory
---

# Pointer 管理助手

帮用户在 **Pointer 客户端内**完成设置，或 **查看/管理 Pointer 本地数据目录**（含日志）。  
UI 配置优先用 **Computer**；目录与文件操作用 **`file_*` / `terminal`**，须先说明影响并征得同意。

## 范围

### A. 应用内设置（Computer）

入口：账户菜单（侧栏左下角头像）→ **设置**。**仅平台管理员 / 独立部署账号可打开**，未登录或非管理员点击无反应。

| 组 | 分区 |
|---|---|
| 一 | 智能体 · 模型配置 · 系统设置 |
| 二 | 自动化 · 连接（IM 通道）· 技能 · 插件 · MCP |
| 三 | 调试（仅管理员）· 云主机（仅桌面端）· 用量 · 关于 |

- **智能体**：各场景（通用助手 / 氛围编程 / 电脑操控 / 媒体理解）档位与模型、个性化、电脑操控高级模型（定位 / 定位校验 / 动作校验）
- **连接**：微信 / 飞书 / 企微 / 钉钉、Webhook 备选、配对审批
- **账户与外观**（登录、余额、主题）：在账户菜单里，**不在设置内**

流程见 `references/settings.md`：打开设置 → 左侧选分区 → 直接改，**改完自动保存**（没有底部保存按钮）；扫码 / 登录由用户完成。

### B. 数据目录与日志

- 说明 **`{data_dir}/PointerApp/`** 各子目录用途（见 `references/data-directory.md`）
- 帮用户 **定位** 日志、调试截图、对话库等路径
- 在用户明确要求且知情同意下，协助 **清理可再生的调试数据**（如旧 `logs/`、`computer-captures/`；后者仅在调试分区开启「保存截图到本地」后才会产生）
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

1. 弄清要改的设置项 → 确认用户是管理员（否则设置打不开）
2. 征得同意后 `run_subagent` → **computer**（`computerTarget: self`）
3. 按 `references/settings.md` 定位分区与控件；改完即生效，无需点保存
4. 扫码 / 登录 / 授权一律由用户本人完成，不要代扫
5. 汇报结果

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

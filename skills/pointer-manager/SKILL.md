---
name: pointer-manager
description: >-
  Pointer in-app settings, local data directory, and server deployment: connections (WeChat,
  Feishu, WeCom, DingTalk), agent tiers/models, system options, automation, skills/plugins/MCP;
  PointerApp data directory, logs, debug artifacts; sandbox execution environment for the
  server (docker image build/registry/offline tar, Cube/envd install and multi-node, tier vs
  backend choice, POINTER_SERVER_ENVIRONMENT_* variables, resource sizing, sandbox start
  failures). Use Computer for settings UI; file/terminal for directory tasks with user consent.
  Use when the user mentions Pointer settings, connect/configure IM channels, data directory,
  log location, clear logs, Application Support PointerApp, deploy Pointer server, prepare
  sandbox runtime, install Cube, execution environment not bound—not generic WeChat Open
  Platform or unrelated app config. When matched, call skill_read first; do not answer from the
  skills index alone.
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
  - deploy
  - docker
  - cube
  - envd
  - sandbox
---

# Pointer 管理助手

帮用户在 **Pointer 客户端内**完成设置，或 **查看/管理 Pointer 本地数据目录**（含日志），
或 **给服务端准备沙箱执行环境**（agent 在哪儿跑命令、开浏览器）。
UI 配置优先用 **Computer**；目录与文件操作用 **`file_*` / `terminal`**，须先说明影响并征得同意。

## 范围

### A. 应用内设置（Computer）

入口：账户菜单（侧栏左下角头像）→ **设置**。**桌面客户端始终可打开**；网页 / 服务器部署仅平台管理员可打开。

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

### C. 服务端部署与环境准备

agent 要在服务端跑命令、读写文件、开浏览器，先得有**执行环境**。两个后端二选一：

| 场景 | 档位 `provider` | 后端 `backend` |
|---|---|---|
| 桌面 App，跑在用户自己电脑 | `local` | `host` |
| 单机 web，使用者是同事 | `container` / `light` | `docker` |
| **多用户 web，互不可信** | `container` / `light` | `envd` + Cube |
| 已有 K8s | `container` | `docker`（或自建 Kata/gVisor） |

判断口诀：**同一台机器上跑的是不是"别人的代码"**。是 → Cube；否 → Docker。

档位决定**有没有桌面**（`container` 有 GUI，`light` 只有命令行）；档位与后端必须自洽，
否则服务端启动直接报错。完整命令见：

- `references/deploy-docker.md` —— 镜像三种来源、宿主权限、验证、容量规划
- `references/deploy-cube.md` —— KVM 前提、单机安装、加计算节点、做模板、卷与用户状态

**动手前先探测**，不要凭猜：

```bash
uname -m                                     # x86_64 / arm64
ls -l /dev/kvm 2>/dev/null                   # 有没有硬件虚拟化
docker ps >/dev/null 2>&1 && echo docker-ok  # 有没有 Docker
df -h /data 2>/dev/null | tail -1            # 磁盘
```

四条硬事实：

- **有镜像 ≠ 能跑**：必须装 Docker，镜像是文件系统快照，运行要守护进程
- **Cube 也要 Docker**：MySQL / Redis / CubeProxy / CoreDNS 都是容器
- **镜像名不是 digest**：规格指纹只 hash 镜像名字，`latest` 更新后老容器不会重建
- **容器以非 root 运行**（`pointer`，uid 1000）：`apt` 要 `sudo`，`pip --user` / `npm` 不用

## 数据目录根路径

| 平台 | `{data_dir}` |
|------|----------------|
| macOS | `~/Library/Application Support` |
| Windows | `%APPDATA%` |
| Linux | `~/.local/share`（或 XDG data home） |

应用根目录恒为 **`{data_dir}/PointerApp/`**。用 **`file_list`** 确认实际路径，勿编造用户名。

## 流程

**设置类**

1. 弄清要改的设置项 → 桌面客户端可直接进；网页 / 服务器部署需管理员账号
2. 征得同意后 `run_subagent` → **computer**（`computerTarget: self`）
3. 按 `references/settings.md` 定位分区与控件；改完即生效，无需点保存
4. 扫码 / 登录 / 授权一律由用户本人完成，不要代扫
5. 汇报结果

**数据目录类**

1. 确认用户需求（查看 / 统计大小 / 清理哪类文件）
2. 加载本技能后按 `references/data-directory.md` 解释目录
3. 用 **`file_list` / `file_read` / `terminal`** 执行；删除前再次确认
4. 汇报路径、释放空间（若可估）、是否需重启应用

**部署类**

1. 先跑探测命令（`uname -m` / `/dev/kvm` / `docker ps` / `df`），用结果选后端
2. 分清**在哪台机器执行**：Pointer 跑在用户本机，部署目标常常是另一台服务器

   | 动作 | 在哪 |
   |---|---|
   | `docker build` / `docker load` / 冒烟验证 | 目标机 |
   | 装 Cube、加计算节点（要 root） | 目标 Linux 服务器 |
   | 改 `server.toml` | 目标机 |

   连不上目标机就**整理成命令清单交给用户执行**，并说明每步验证什么；不要假装能远程登录
3. 按后端读对应 reference，照命令走，不要凭记忆写
4. 验证到**服务端真能跑**才算完成：docker 后端 agent 跑 `pwd` 返回 `/workspace`；
   envd 后端 `id` 返回 uid ≥ 100000。只看到"容器起来了"不算
5. 汇报：选了哪个后端及理由、改了哪些配置、验证结果、仍需用户做的事

## 输出

```markdown
## 结果
- 类型：设置 / 数据目录 / 部署
- 项目：
- 状态：
- 路径（如适用）：
- 仍需你手动：
```

# 统一工作区与容器挂载

> 状态：已确认方向，按阶段落地  
> 范围：Client/Web 共用工作区模型、用户级 Environment、`/home` + `/data` 挂载、会话执行归属  
> 不在范围：设置页改造、独立 Web 实现

## 结论摘要

- Client 与 Web 复用同一套 WorkspaceService / DTO / 选择组件；只在「如何拿到候选目录」上适配。
- 沙箱持久根是 `/home/pointer` 与 `/data`，不是 `/workspace`。`workspace_root` 只表示当前项目或 worktree。
- 已登录用户默认复用**用户级** Environment；会话持有 `ResolvedWorkspace` 与执行归属，不各自起容器。
- 不新增 `workspace_kind` / `workspace_ref` / `worktree_id`；复用 Project.`workspace_root`，Conversation 经 `project_id` 继承。
- skills/plugins 代码只读；CLI 凭证写 Home；Skill 启停走用户设置，不写技能目录。

## 目录布局

```text
{base}/{user_key}-home → /home/pointer
{base}/{user_key}-data → /data
  ├── workspaces/{project}/
  ├── worktrees/{conversation_key}/
  ├── attachments/{conversation_id}/
  ├── media/chat/{conversation_id}/
  ├── media/generated/{conversation_id}/
  ├── captures/{date}/{conversation_id}/
  └── turn-baselines/{conversation_id}/{turn_id}/
```

- `user_key` 是碰撞安全、不含个人信息的稳定哈希，满足 envd 扁平卷名。
- 普通目录选择器根是 `/data/workspaces`；工作区面板根是当前项目/worktree。
- worktree 不混入 Project 字段，按 `conversation_id` 查独立注册表。

## Environment 与会话

| 概念 | 含义 |
|------|------|
| Environment | 一台执行电脑：`home_root` + `data_root` |
| ResolvedWorkspace | 某个会话当前打开的项目/worktree |
| ConversationExecutionContext | cwd、进程/PTY、取消、凭证注入范围 |

缓存键（已登录）：`(user_key, backend, tier, template_fingerprint)`。  
匿名与显式隔离任务仍按会话/任务起临时 Environment。

会话隔离首先是应用层归属与并发控制，不是同用户内的强安全边界。需要强隔离时使用独立 Environment。

## 复用与补齐

已有：

- 按会话的 CancellationToken / JobSupervisor
- terminal 进程组与树终止
- Console 独立 PTY
- ComputerState 按会话状态

需补齐：

- 统一 ConversationExecutionContext 与 Pointer 启动进程归属
- Console 按会话批量关闭
- 自动端口登记；用户固定端口冲突按真实电脑语义报错
- Desktop 控制 lease（`pin` 只防回收）

## 阶段

1. 安全地基：`user_key`、路径策略、拒绝沙箱挂宿主目录、拒绝沙箱会话宿主 worktree source
2. 用户级双卷与 Environment 生命周期
3. 共享 API / WorkspaceChooser
4. 工具与面板收口到 ResolvedWorkspace
5. 附件媒体写切到 `/data`
6. Environment worktree
7. 迁移与旧布局退役


## 布局迁移工具

`pointer_core::storage_migration::migrate_user_layout(user_id, dry_run)`：

1. 扫描 `conversation-media/`、`generated-media/`、`computer-captures/`；
2. dry-run 写入 journal（`{user-home}/.pointer/layout-migration-journal.json`）为 `planned`；
3. 正式迁移按文件 copy + size/sha256 校验，journal 记 `copied`；
4. 再次运行幂等跳过已完成项。

新写入已走用户 data 卷的 v2 `storage_rel_path`；旧路径 dual-read 保留至兼容窗口结束。

## 媒体预览读路径

v2 附件在沙箱里是 `/data/...`。预览、下载和流式播放按当前会话环境取字节，再发给浏览器：

- 沙箱环境：走该环境的读文件接口（远端整段读；容器有宿主挂载时由环境映射到挂载文件再流式返回）。
- 本地档位，或请求没带会话、环境还没建好：继续读本机文件。
- 应用数据目录里的旧路径不进 `/data`，始终读本机。
- 桌面播放同样遵守这条：沙箱会话不把宿主文件地址交给播放器，读不到本机地址时改走上面的预览读路径。

## 会话内执行

工作区在容器里时，读、写、git 和 stdio MCP 都走当前会话环境。本地档位继续在本机执行。会话环境还没缓存时直接报错，不去读宿主上的 data 卷目录。

- 插件加载阶段没有会话，stdio MCP 仍在本机握手以发现工具；沙箱会话真正调用时，在自己的环境里再起一份。
- HTTP MCP 连的是远端服务，不按宿主进程拒绝。
- 沙箱 worktree 的创建、检查和删除都在会话环境里对 `/data/worktrees` 执行 git。本地档位仍用本机 git。
- 回合文件 diff 与文件工具相同：`/data/...` 走已缓存的会话环境，相对路径拼调用方传来的项目目录。环境还没缓存就报错。Web 和 Client 都调用这一段。
- 回合基线跟执行环境走。本地写应用数据目录的 `turn-baselines/`。沙箱写该环境的 `/data/turn-baselines/`。全文、路径旁路和页脚摘要都通过这份环境读写。

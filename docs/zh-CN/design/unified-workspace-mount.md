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
  └── captures/{date}/{conversation_id}/
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

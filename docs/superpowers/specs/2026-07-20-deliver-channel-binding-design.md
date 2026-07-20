# Deliver 通道绑定（小白可用）设计

## 目标

让小白用户与 agent（`cron_job` 工具）都能用**通道名**指定定时任务的
IM 推送对象，无需填写 `ou_` / `oc_` 等平台 ID，也无需手改
`channels_config.json`。

产品前提：

- 用户可连接多个 IM 通道（飞书 / 钉钉 / 企微 / 微信）。
- **每个通道账号有且仅有一条投递绑定**（即现有 home channel）。
- 选「飞书」= 推到飞书当前绑定的那一个人（或显式绑过的群）。

## 非目标

- 不定时任务创建时按「最近聊过的多人列表」选人（主路径不需要）。
- 不在主路径暴露自定义 deliver 字符串（`feishu:ou_xxx` 等）。
- 不做全量通讯录同步。
- 不新增独立的 `im_deliver` 配置工具；建/改任务时的推送对象只通过
  `cron_job.deliver`（及设置页等价 UI）指定。
- 不在本阶段做会话标题栏「设为推送对象」菜单（若后续需要可另开）。
- 不改变 `im_send` 立刻发消息的语义；进阶显式 ID 目标仍可留给
  `im_send` / 后续进阶能力，不进入定时任务小白路径。

## 背景（与 Hermes / 现状）

Hermes：裸平台名 → home channel；另有 channel directory 做人名解析。

Pointer 现状（Phase 1–2）：

- `cron_jobs.deliver` + `ImDeliverHook` 已可按字符串推送。
- `homeRecipientId` / `homeIsGroup` 需手编配置。
- 设置页与 `cron_job` 工具仍展示/接受完整 deliver 格式说明，小白门槛高。

本设计把「home」产品化为**每通道一条绑定**，并把定时任务侧的选择面
收敛为通道名。

## 概念

### 通道绑定（Channel Binding）

每个已启用通道账号一条：

| 字段（已有） | 含义 |
|--------------|------|
| `homeRecipientId` | 绑定对象的平台 ID |
| `homeIsGroup` | 是否为群 |

解析：

- `deliver: "feishu"` → 飞书 default（或唯一）账号的绑定对象
- `deliver: "feishu,dingtalk"` → 两个通道各自的绑定对象
- `deliver: "all"` → 所有**已绑定**的通道（无绑定則跳过并记日志）

未绑定却写入该通道名时：创建/更新任务应失败并返回可读错误（见错误处理）。

### 自动绑定规则

1. **私聊**：某用户向 bot 发私聊消息时，将该通道账号的绑定更新为
   **当前（最后一次）私聊对方**（覆盖旧绑定）。
2. **群聊**：群消息**不**自动改绑定。
3. 绑定写入持久化配置（与现有 `channels_config` / 账号配置同一存储），
   APP 与 Web/server 一致。

显示名：有则缓存（钉钉事件已有 `senderNick`；飞书等可后续补查询），
用于设置页展示「飞书 · 已绑定：张三」；解析投递仍用 ID。

## 用户路径

### A. 设置页 · 定时任务

1. 「推送到 IM」开关；关闭 = `deliver` 为空。
2. 打开后：对每个**已启用且已绑定**的通道展示多选（文案用通道中文名，
   如「飞书」「钉钉」）。
3. 仅一个已绑定通道时：默认勾选该通道。
4. 未绑定通道：禁用该项，旁注「请先在该通道私聊 Pointer」。
5. 列表展示：`投递：飞书`，不展示原始 deliver 字符串（title/调试可保留）。

自定义字符串输入默认**隐藏**（进阶折叠可选，非必须本期）。

### B. Agent · `cron_job` 工具

用户说「每天早八推日报到飞书」→ agent 调用：

```text
cron_job(action="create", …, deliver="feishu")
```

`deliver` 约定（工具 schema + 提示词）：

- 主路径只接受：通道名、逗号分隔多通道、`all`、空。
- 通道名：`feishu` | `dingtalk` | `wecom` | `weixin`（大小写不敏感，入库规范化小写）。
- **不要**在提示词主路径教模型填 `ou_` / `oc_`；若模型仍传入显式 ID
  格式，实现上可继续兼容解析（与现有 `im_delivery` 一致），但文档与
  提示词不以之为推荐。

`update` 动作若已支持改字段，应同样支持改 `deliver`（空字符串清空）。

### C. 绑定建立（无 UI 菜单本期）

依赖自动绑定：用户先在目标通道私聊 bot → 绑定更新为该用户 →
再建任务勾选/传入通道名。

通道设置页展示绑定状态（只读 + 可选「清除绑定」即可；更换依赖再私聊
覆盖，符合「最后一次私聊」）。

## 数据流

```text
IM 私聊入站
  → 更新该 channel/account 的 homeRecipientId + homeIsGroup=false
  → （可选）缓存显示名

创建/更新 cron（UI 或 cron_job 工具）
  → deliver = "feishu" | "feishu,dingtalk" | "all" | null
  → 校验：提及的通道均已绑定（all 则至少有一个已绑定）
  → 写入 cron_jobs.deliver

调度触发
  → trigger_meta.extra.deliver = 同上
  → run 结束 ImDeliverHook
  → resolve "feishu" → 当前绑定 OutboundContext → 推送
```

## 错误处理

| 情况 | 行为 |
|------|------|
| `deliver` 含未绑定通道名 | `cron_job` / HTTP create·update **拒绝**，中文/英文可读错误说明先私聊 |
| `deliver: "all"` 且无一绑定 | 拒绝 |
| 任务创建时已绑定，之后绑定被清或人变更 | 下次推送用**当前**绑定（按通道名解析，不快照 ID）；绑定被清则 hook 记 `last_delivery_error`，不改 run 成功态 |
| 自动绑定写配置失败 | warn 日志；不影响入站回复 |

## 实现要点（建议落点）

| 区域 | 变更 |
|------|------|
| `pointer-channels` 入站 | 私聊成功解析后调用「更新绑定」；群跳过 |
| `ChannelAccountConfig` | 沿用 home 字段；可选 `homeDisplayName` 缓存 |
| `im_delivery` | `list_home_delivery_targets` 供 UI；校验「通道是否已绑定」可复用 |
| `cron_job` 工具 | create/update 前校验 deliver；提示词改为通道名主路径 |
| Automation 设置页 | 开关 + 通道多选，映射为 deliver 字符串 |
| 通道设置（轻量） | 展示已绑定对象 / 未绑定引导 |
| 文档 | `channel-integration.md` Phase 3 本切片；webhook/cron 示例用通道名 |

跨端：逻辑在 `pointer-core` / `pointer-channels`；Tauri 与 server 共用 API/命令。

## 分期

| 切片 | 内容 |
|------|------|
| **P3a** | 私聊自动绑定（最后一人）；`cron_job` 提示词与校验以通道名为主；设置页开关 + 通道多选 |
| **P3b** | 通道设置页绑定状态展示；显示名缓存（飞书等可后续 API） |
| **P3c**（可选） | 进阶：显式绑群、折叠自定义 deliver、会话 UI 设绑定 |

## 测试要点

- 私聊 A 再私聊 B → 绑定为 B；群消息不改绑定。
- `deliver=feishu` 在已绑定 / 未绑定下 create 成功与失败。
- 多通道 `feishu,dingtalk` 与 `all`。
- UI 勾选映射字符串与回显通道名。
- APP 与 server 配置写入路径一致。

## 成功标准

小白路径：**在飞书私聊 Pointer → 建定时任务勾选「飞书」（或让 agent 设
`deliver=feishu`）→ 到点收到推送**，全程不出现平台 ID。

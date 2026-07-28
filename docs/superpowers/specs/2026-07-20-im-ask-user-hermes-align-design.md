# IM `ask_user` 对齐 Hermes `clarify` 设计

## 目标

IM 通道上 `ask_user` 与 Hermes `clarify` 同语义：

- **同轮阻塞**：工具等待用户回复，不结束 turn
- **入站拦截**：下一则 IM 消息解析为选项，填入 tool result，不当作新一轮对话
- App/Web 保持 oneshot UI；宿主提供「其他」自由输入（与 IM 同源校验）

## 非目标

- 本期不做飞书/钉钉原生交互卡片按钮（文本编号选择即可）
- 不改 cron/webhook 等无人值守路径（本来不应调用 `ask_user`）

## 流程

```
IM 用户消息 → run_chat(trigger=Im)
  → 模型输出问题/选项文本 + ask_user
  → MessageEnd → IM 推送可见文本
  → ask_user 注册 pending(base_conv_id) + oneshot，阻塞
  → 用户下一条 IM 消息进入 gateway.process_inbound
  → 命中 pending：解析「1」/标签 → submit_ask_user → 不跑新 turn
  → oneshot 唤醒 → tool result { selected } → 同轮继续 LLM
```

## 关键实现

| 组件 | 说明 |
|------|------|
| `im_ask_user` 模块 | `register` / `try_resolve` / `clear`；按 base `conv_id` 索引 |
| `dispatch_ask_user` | IM 改为与桌面相同的 oneshot 阻塞；超时默认 600s |
| `ChannelGateway::process_inbound` | `/stop` 之后、`handle_inbound` 之前拦截 |
| `channel_outbound` 提示词 | 改为阻塞语义，要求先写出编号选项再调工具 |
| `/stop` | cancel 时按 base 清 pending，并 cancel 匹配的 desktop_conv_id token |

## 回复解析

对齐 Hermes 文本 fallback：

1. 纯数字 `1` / `1.` / `1)` → 映射为对应 option **label**（Pointer 增强）
2. 完整 label（大小写不敏感）→ 该 label
3. 其余非空原文 → **原样**作为回答放行（自由文本 / `Other`）
4. 仅空消息拒绝并提示，保持 pending

多选时按逗号/中文逗号/空白拆 token，逐个按上规则处理。

## 超时

超时返回 tool JSON：`selected: null`, `timed_out: true`，模型自行决定是否继续。

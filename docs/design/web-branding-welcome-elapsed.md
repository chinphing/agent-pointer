# 服务端 Web branding：欢迎 tip + 耗时前缀

## 决策（已确认）

| 项 | 结论 |
|----|------|
| 进行中收起 | **A**：不增加 server 默认；部署文档要求打开「默认收缩执行过程」 |
| Tip 出现时机 | 仅**全新空会话**（`showWelcomeHome`）；非清屏/失败 hydrate 空窗 |
| 未知耗时 | 跟前缀：`{prefix}耗时未知` |

## 范围

与 `page_title` / `composer_placeholder` 同套路的可选覆盖；默认产品文案不动。交付表格属智能体 Markdown，不在本方案。

详见落地文档 [`../ui/web-branding-welcome-elapsed.md`](../ui/web-branding-welcome-elapsed.md)。

## 分期

1. P0：config / meta / `webBranding` / `formatTurnElapsed` / 欢迎 tip UI / 测试
2. P0：standalone 与 UI 文档；collapse 用设置说明（A）
3. 并行：业务智能体提示词输出进度表（与 branding 解耦）

---
name: pointer-config
description: >-
  在 Pointer 应用内配置用户向设置（非调试菜单）：IM 通道对接/连接微信、飞书、企微、钉钉，
  智能体选项，平台账户登录；可用 Computer 代操作设置界面。
  Use when user says 对接微信, 怎么对接微信, 配置微信/飞书/企微/钉钉, IM通道, Pointer设置,
  扫码登录, 配对码, 工具权限, or set up Pointer messaging—not generic WeChat Open Platform advice.
tags:
  - pointer
  - wechat
  - feishu
  - wecom
  - dingtalk
  - im-channel
---

# Pointer 配置助手

帮用户在对话里完成 Pointer 设置，优先用 **Computer** 操作客户端；扫码、浏览器登录等由用户亲自完成。

## 范围

**可配置：**

- **智能体**：电脑操控、工具权限、上下文压缩
- **IM 通道**：微信 / 飞书 / 企微 / 钉钉、配对审批
- **平台账户**（桌面端）：浏览器登录 / 退出

## 流程

1. 弄清用户要改哪一项
2. 征得同意后 `run_subagent` → **computer**
3. 打开 **设置**（侧栏齿轮）→ 进入对应左侧菜单
4. 代操作 UI；遇到扫码 / 登录 / 配对码时**暂停等用户**
5. 点底部 **保存**（智能体、IM 通道会持久化）
6. 简要汇报结果

UI 位置与分通道要点见 `references/settings.md`。

## 输出

```markdown
## 配置结果
- 项目：
- 状态：
- 仍需你手动：
```

# 飞书 CLI（lark-cli）集成 SOP

[English](../../en/developer/feishu-cli-integration-sop.md) | 简体中文

> 版本: 1.0 | 适用平台: Windows | 工具: `@larksuite/cli` v1.0.53
>
> 只差「首次对接 + 授权」这一步？见 [lark-cli-quickstart.md](lark-cli-quickstart.md)（Device Flow、scope 增补、验证清单）。本页是完整命令 SOP。

---

## 目录

1. [安装](#1-安装)
2. [应用配置](#2-应用配置)
3. [权限授权](#3-权限授权)
4. [发送消息](#4-发送消息)
5. [文档操作](#5-文档操作)
6. [电子表格操作](#6-电子表格操作)
7. [中文编码问题（Windows 必读）](#7-中文编码问题windows-必读)
8. [常见错误及处理](#8-常见错误及处理)
9. [快速参考命令表](#9-快速参考命令表)

---

## 1. 安装

```bash
npm install -g @larksuite/cli
```

安装后验证：
```bash
lark-cli --version
lark-cli --help
```

> **注意:** 安装后二进制文件位于 `%APPDATA%\npm\node_modules\@larksuite\cli\bin\lark-cli.exe`，由 `run.js` Node 脚本调用。

---

## 2. 应用配置

### 2.1 初始化应用配置（首次运行）

```bash
lark-cli config init --new
```

- 执行后显示 **二维码**，需用飞书 App 扫码
- 扫码后配置应用（appId + appSecret）
- 配置结果保存到 `~/.lark-cli/config.json`

### 2.2 查看配置状态

```bash
lark-cli config show
```

输出示例：
```json
{
  "appId": "cli_xxx",
  "appSecret": "****",
  "brand": "feishu",
  "profile": "cli_xxx"
}
```

### 2.3 绑定用户身份（AI 智能体场景）

```bash
lark-cli config bind --identity bot-only
```

> 参数说明：
> - `bot-only` — 仅机器人身份（安全默认，推荐）
> - `user-default` — 允许用户身份（可访问个人资源）

---

## 3. 权限授权

### 3.1 用户登录（核心前置步骤）

```bash
# Step 1: 请求授权链接（生成二维码）
lark-cli auth login --domain <domain1> --domain <domain2> --json --no-wait

# Step 2: 生成二维码图片
lark-cli auth qrcode "<verification_url>" --output qr.png

# Step 3: 用户扫码授权后完成登录
lark-cli auth login --device-code "<device_code>"
```

### 3.2 请求指定权限

```bash
lark-cli auth login --scope "<scope_name>" --json --no-wait
```

可同时请求多个权限：
```bash
lark-cli auth login --scope "scope1 scope2 scope3" --json --no-wait
```

### 3.3 授权流程总结

```
┌──────────────────────────────────────────────────────────────┐
│   lark-cli auth login --no-wait                              │
│       ↓                                                      │
│   返回 device_code + verification_url                        │
│       ↓                                                      │
│   lark-cli auth qrcode <url> → 生成二维码                    │
│       ↓                                                      │
│   用户用飞书 App 扫码授权                                     │
│       ↓                                                      │
│   lark-cli auth login --device-code <code>                   │
│       → 完成授权                                             │
└──────────────────────────────────────────────────────────────┘
```

### 3.4 查看当前权限

```bash
lark-cli auth status       # 查看当前登录状态和已授予 scopes
lark-cli auth scopes       # 查看应用可用的全部 scopes
```

### 3.5 常见权限清单

| 权限 Scope | 用途 | 首次需要授权 |
|---|---|---|
| `im:message` | 发送消息（Bot 身份） | 否（需登录） |
| `im:message.send_as_user` | 以用户身份发消息 | ✅ 需在开发者后台开启 + 重新授权 |
| `im:message:readonly` | 读取消息 | 否 |
| `docx:document:create` | 创建文档 | 否 |
| `docx:document:readonly` | 读取文档内容 | 否 |
| `sheets:spreadsheet:create` | 创建电子表格 | ✅ 需授权 |
| `sheets:spreadsheet:write_only` | 写入电子表格 | ✅ 需授权 |
| `sheets:spreadsheet:read` | 读取电子表格 | ✅ 需授权 |
| `contact:user:search` | 搜索联系人 | ✅ 需授权 |
| `contact:user.base:readonly` | 读取用户基本信息 | 否 |

---

## 4. 发送消息

### 4.1 发送给单个用户

```bash
# Bot 身份发送（需要对方在同一企业租户）
lark-cli im +messages-send --user-id ou_xxx --text "消息内容" --as bot

# 用户身份发送（跨企业也可用）
lark-cli im +messages-send --user-id ou_xxx --text "消息内容" --as user
```

### 4.2 支持的格式

```bash
# 纯文本
lark-cli im +messages-send --user-id ou_xxx --text "你好"

# Markdown
lark-cli im +messages-send --user-id ou_xxx --markdown "**加粗** *斜体*"

# 富文本 JSON
lark-cli im +messages-send --user-id ou_xxx --content '{...}'
```

### 4.3 搜索联系人

```bash
lark-cli contact +search-user --query "姓名"
```

返回结果包含：
- `open_id` — 用户的 Open ID
- `p2p_chat_id` — 与该用户的 P2P 会话 ID
- `is_cross_tenant` — 是否为跨企业用户
- `has_chatted` — 是否曾聊过天

---

## 5. 文档操作

### 5.1 创建文档

```bash
# 创建空白文档
lark-cli docs +create --api-version v2 --content '<title>标题</title><p>内容</p>' --as user

# 创建文档（带格式）
lark-cli docs +create --api-version v2 --content '<title>文档标题</title><callout>提示内容</callout>' --as user
```

### 5.2 读取文档

```bash
lark-cli docs +read --doc-token xxx --api-version v2
```

### 5.3 写入文档内容

```bash
lark-cli docs +write --doc-token xxx --content '<p>新段落</p>' --append
```

---

## 6. 电子表格操作

### 6.1 创建表格

```bash
lark-cli sheets +workbook-create --title "表名" --as user
```

> **注意:** `--headers` 参数在 Windows 下通过 JSON 文件传递（见第 7 节）。

### 6.2 写入数据

```bash
# 通过 JSON 文件写入（推荐）
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F101 --cells @data.json --as user
```

`data.json` 格式要求：
```json
[
  [
    {"value": "列1"},
    {"value": "列2"},
    {"value": 123}
  ],
  [
    {"value": "数据A"},
    {"value": "数据B"},
    {"value": 456}
  ]
]
```

> **重要：** 每行内数组元素数量必须与 range 列数一致。

### 6.3 读取数据

```bash
# JSON 格式（完整信息）
cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F101 --as user

# CSV 格式（精简）
cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F101 --format csv --as user
```

### 6.4 更新单元格样式

```bash
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F1 --cells @header_style.json --as user
```

---

## 7. 中文编码问题（Windows 必读）

### 7.1 问题原因

Windows 下 PowerShell 的编码链路导致中文参数乱码：

```
链路                   编码                     结果
PowerShell 参数 →    $OutputEncoding = US-ASCII   ❌ 中文被截断
                    ↓
lark-cli.exe 收到    GB2312 / 乱码               ❌ 解析失败
```

### 7.2 ✅ 唯一可靠方案

**`cmd.exe /c` + `@文件` 传递中文数据**

```bash
# ✅ 正确：中文写在 JSON 文件里，通过 @file 加载
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --range A1:F1 --cells @data.json --as user
```

> 所有包含中文的命令行参数（`--content`、`--cells`、`--headers` 等），在 Windows 下都**必须**用 `@file` 方式传入，不能直接写中文内联。

### 7.3 什么时候不需要 `cmd.exe /c`

- 没有中文参数的场合（纯英文/数字/布尔值），可以直接用 PowerShell 调用
- 但使用了 `@file` 加载 JSON 文件时，也建议加 `cmd.exe /c` 前缀保持统一

### 7.4 JSON 文件编写规范

```json
[
  [
    {"value": "中文文本"},
    {"value": 42},
    {"value": "是"}
  ]
]
```

- 文件保存为 **UTF-8 without BOM** 编码
- 文件名使用英文（如 `data.json`），避免文件名本身含中文
- 可使用 `file_write` 工具生成 JSON 文件

---

## 8. 常见错误及处理

### 错误 1：命令无任何输出（exit code 1）

**现象：** 执行命令后无 stdout/stderr，仅返回退出码 1。

**原因：** PowerShell 中文编码问题，`lark-cli` 收到乱码参数后静默退出。

**解决：** 使用 `cmd.exe /c` 前缀 + `@file` 传递中文参数。

---

### 错误 2：`cross tenant p2p chat operate forbid`（230038）

**现象：** 给跨企业用户发消息失败。

**原因：** 飞书平台限制，不允许跨企业直接私聊。

**解决：**
- 将对方拉入群聊后，Bot 可在群内发消息
- 或直接在飞书 App 内手动与对方聊天
- 或给同企业用户发消息

---

### 错误 3：`Bot has NO availability to this user`（230013）

**现象：** Bot 身份发送消息失败。

**原因：** 接收方不属于 Bot 应用所在的企业租户。

**解决：** 与错误 2 相同——跨企业用户需用群聊或用户身份发送。

---

### 错误 4：授权后 scope 未生效

**现象：** `auth login --scope xxx` 完成后，`scopes` 列表中仍无该权限。

**原因：** 该权限需要在飞书开发者后台先开启，再在授权页面点击同意。

**完整流程：**

```
1. 飞书开发者后台 (https://open.feishu.cn/app)
   → 选择应用
   → 权限管理 → 搜索 scope → 开启
   → 版本管理与发布 → 创建版本 → 发布

2. CLI 授权
   lark-cli auth login --scope "xxx" --json --no-wait
   → 扫码 → 在授权页面确认同意
   → lark-cli auth login --device-code <code>
   → 验证: lark-cli auth scopes
```

---

### 错误 5：`invalid JSON` / `expected type "array", got "string"`

**现象：** `--cells` 或 `--headers` 参数 JSON 解析失败。

**原因：**

| 常见问题 | 正确写法 |
|---|---|
| 一维数组 `["a","b"]` | 二维数组 `[["a","b"]]` |
| 纯字符串值 `"a"` | 对象 `{"value": "a"}` |
| PowerShell 内联中文 `'{"value":"中文"}'` | `@file` 加载 |

---

### 错误 6：`Unknown domain` 或参数解析失败

**现象：** `--domain im,docs` 报错。

**原因：** 某些版本的 lark-cli 不支持逗号分隔的 domain 列表。

**解决：**
```bash
# ✅ 正确：重复 --domain
--domain im --domain docs --domain contact

# ❌ 错误：逗号分隔
--domain im,docs
```

---

### 错误 7：授权页面未显示新 scope

**现象：** 扫码后授权页面的权限列表未包含新申请的 scope。

**原因：**
1. 开发者后台未开启该权限
2. 开启了但未发布新版本
3. 启用了但版本发布尚未生效（等待 1-2 分钟）

**解决：**
```bash
# 检查应用是否已有此 scope
lark-cli auth scopes | grep scope_name
```

---

## 9. 快速参考命令表

### 配置管理

| 操作 | 命令 |
|---|---|
| 查看配置 | `lark-cli config show` |
| 初始化配置 | `lark-cli config init --new` |
| 查看身份状态 | `lark-cli auth status` |
| 列出可用权限 | `lark-cli auth scopes` |

### 授权管理

| 操作 | 命令 |
|---|---|
| 用户登录（获取授权链接） | `lark-cli auth login --json --no-wait` |
| 生成二维码 | `lark-cli auth qrcode <url> --output qr.png` |
| 完成授权（轮询） | `lark-cli auth login --device-code <code>` |
| 带 scope 登录 | `lark-cli auth login --scope "scope1 scope2" --json --no-wait` |

### 消息

| 操作 | 命令 |
|---|---|
| 搜索用户 | `lark-cli contact +search-user --query "姓名" --as user` |
| 发消息（文本） | `lark-cli im +messages-send --user-id ou_xxx --text "内容"` |
| 发消息（Markdown） | `lark-cli im +messages-send --user-id ou_xxx --markdown "**内容**"` |
| 列出群聊 | `lark-cli im +chat-list` |

### 文档

| 操作 | 命令 |
|---|---|
| 创建文档 | `lark-cli docs +create --api-version v2 --content '<title>T</title>'` |
| 读取文档 | `lark-cli docs +read --doc-token xxx` |
| 追加内容 | `lark-cli docs +write --doc-token xxx --content '<p>内容</p>' --append` |

### 电子表格

| 操作 | 命令 |
|---|---|
| 创建表格 | `lark-cli sheets +workbook-create --title "表名" --as user` |
| 写入数据 | `cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --range A1:F100 --cells @data.json --as user` |
| 读取数据（JSON） | `cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --range A1:F100 --as user` |
| 读取数据（CSV） | `cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --range A1:F100 --format csv --as user` |

---

## 附录：完整上手流程（速查）

```bash
# 1. 安装
npm install -g @larksuite/cli

# 2. 初始化应用
lark-cli config init --new
# → 扫码配置 appId + appSecret

# 3. 授权登录（首次获取基础权限：消息、文档）
lark-cli auth login --domain im --domain docs --json --no-wait
→ 生成二维码 → 扫码 → 完成授权

# 4. 验证
lark-cli auth status                    # 确认用户已登录
lark-cli contact +search-user --query "自己"  # 确认可搜索联系人

# 5. 发一条测试消息
lark-cli im +messages-send --user-id ou_xxx --text "Hello" --as bot

# 6. 创建文档
lark-cli docs +create --api-version v2 --content '<title>测试</title>' --as user

# 7. 如需电子表格权限，补充授权
lark-cli auth login --scope "sheets:spreadsheet:create sheets:spreadsheet:write_only sheets:spreadsheet:read" --json --no-wait
→ 扫码授权 → 完成

# 8. 创建电子表格
lark-cli sheets +workbook-create --title "测试表" --as user

# 9. 写入数据（Windows 用 @file）
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --range A1:C3 --cells @data.json --as user
```

# Feishu CLI (lark-cli) integration SOP

English | [简体中文](../../zh-CN/developer/feishu-cli-integration-sop.md)

> Version: 1.0 | Platform: Windows | Tool: `@larksuite/cli` v1.0.53
>
> Only missing the "first-time onboarding + authorization" step? See [lark-cli-quickstart.md](lark-cli-quickstart.md) (Device Flow, scope additions, verification checklist). This page is the full command SOP.

---

## Table of contents

1. [Installation](#1-installation)
2. [Application configuration](#2-application-configuration)
3. [Authorization](#3-authorization)
4. [Sending messages](#4-sending-messages)
5. [Document operations](#5-document-operations)
6. [Spreadsheet operations](#6-spreadsheet-operations)
7. [Chinese encoding issues (required reading on Windows)](#7-chinese-encoding-issues-required-reading-on-windows)
8. [Common errors and handling](#8-common-errors-and-handling)
9. [Quick reference command table](#9-quick-reference-command-table)

---

## 1. Installation

```bash
npm install -g @larksuite/cli
```

Verify after installation:
```bash
lark-cli --version
lark-cli --help
```

> **Note:** after installation the binary is located at `%APPDATA%\npm\node_modules\@larksuite\cli\bin\lark-cli.exe`, invoked by the `run.js` Node script.

---

## 2. Application configuration

### 2.1 Initialize application configuration (first run)

```bash
lark-cli config init --new
```

- After running, a **QR code** is displayed; scan it with the Feishu App
- After scanning, configure the application (appId + appSecret)
- The result is saved to `~/.lark-cli/config.json`

### 2.2 View configuration status

```bash
lark-cli config show
```

Example output:
```json
{
  "appId": "cli_xxx",
  "appSecret": "****",
  "brand": "feishu",
  "profile": "cli_xxx"
}
```

### 2.3 Bind user identity (AI agent scenario)

```bash
lark-cli config bind --identity bot-only
```

> Parameter notes:
> - `bot-only` — bot identity only (safe default, recommended)
> - `user-default` — allow user identity (can access personal resources)

---

## 3. Authorization

### 3.1 User login (core prerequisite step)

```bash
# Step 1: request an authorization link (generates a QR code)
lark-cli auth login --domain <domain1> --domain <domain2> --json --no-wait

# Step 2: generate a QR code image
lark-cli auth qrcode "<verification_url>" --output qr.png

# Step 3: the user scans the code to authorize, completing login
lark-cli auth login --device-code "<device_code>"
```

### 3.2 Request a specific permission

```bash
lark-cli auth login --scope "<scope_name>" --json --no-wait
```

Multiple permissions can be requested at the same time:
```bash
lark-cli auth login --scope "scope1 scope2 scope3" --json --no-wait
```

### 3.3 Authorization flow summary

```
┌──────────────────────────────────────────────────────────────┐
│   lark-cli auth login --no-wait                              │
│       ↓                                                      │
│   returns device_code + verification_url                     │
│       ↓                                                      │
│   lark-cli auth qrcode <url> → generates a QR code           │
│       ↓                                                      │
│   the user scans it with the Feishu App to authorize         │
│       ↓                                                      │
│   lark-cli auth login --device-code <code>                   │
│       → authorization complete                               │
└──────────────────────────────────────────────────────────────┘
```

### 3.4 View current permissions

```bash
lark-cli auth status       # view the current login status and granted scopes
lark-cli auth scopes       # view all scopes available to the application
```

### 3.5 Common permission list

| Permission scope | Purpose | Requires first-time authorization |
|---|---|---|
| `im:message` | Send messages (Bot identity) | No (login required) |
| `im:message.send_as_user` | Send messages as the user | ✅ Must be enabled in the developer console + re-authorized |
| `im:message:readonly` | Read messages | No |
| `docx:document:create` | Create documents | No |
| `docx:document:readonly` | Read document content | No |
| `sheets:spreadsheet:create` | Create spreadsheets | ✅ Authorization required |
| `sheets:spreadsheet:write_only` | Write to spreadsheets | ✅ Authorization required |
| `sheets:spreadsheet:read` | Read spreadsheets | ✅ Authorization required |
| `contact:user:search` | Search contacts | ✅ Authorization required |
| `contact:user.base:readonly` | Read basic user information | No |

---

## 4. Sending messages

### 4.1 Send to a single user

```bash
# Send as Bot (the recipient must be in the same enterprise tenant)
lark-cli im +messages-send --user-id ou_xxx --text "消息内容" --as bot

# Send as user (also works across enterprises)
lark-cli im +messages-send --user-id ou_xxx --text "消息内容" --as user
```

### 4.2 Supported formats

```bash
# Plain text
lark-cli im +messages-send --user-id ou_xxx --text "你好"

# Markdown
lark-cli im +messages-send --user-id ou_xxx --markdown "**加粗** *斜体*"

# Rich-text JSON
lark-cli im +messages-send --user-id ou_xxx --content '{...}'
```

### 4.3 Search for contacts

```bash
lark-cli contact +search-user --query "姓名"
```

The result contains:
- `open_id` — the user's Open ID
- `p2p_chat_id` — the P2P chat ID with that user
- `is_cross_tenant` — whether the user belongs to another enterprise
- `has_chatted` — whether you have chatted before

---

## 5. Document operations

### 5.1 Create a document

```bash
# Create a blank document
lark-cli docs +create --api-version v2 --content '<title>标题</title><p>内容</p>' --as user

# Create a document (with formatting)
lark-cli docs +create --api-version v2 --content '<title>文档标题</title><callout>提示内容</callout>' --as user
```

### 5.2 Read a document

```bash
lark-cli docs +read --doc-token xxx --api-version v2
```

### 5.3 Write document content

```bash
lark-cli docs +write --doc-token xxx --content '<p>新段落</p>' --append
```

---

## 6. Spreadsheet operations

### 6.1 Create a spreadsheet

```bash
lark-cli sheets +workbook-create --title "表名" --as user
```

> **Note:** on Windows the `--headers` parameter is passed through a JSON file (see section 7).

### 6.2 Write data

```bash
# Write through a JSON file (recommended)
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F101 --cells @data.json --as user
```

`data.json` format requirements:
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

> **Important:** the number of array elements in each row must match the number of columns in the range.

### 6.3 Read data

```bash
# JSON format (complete information)
cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F101 --as user

# CSV format (condensed)
cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F101 --format csv --as user
```

### 6.4 Update cell styles

```bash
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --sheet-name Sheet1 --range A1:F1 --cells @header_style.json --as user
```

---

## 7. Chinese encoding issues (required reading on Windows)

### 7.1 Cause

On Windows the PowerShell encoding chain garbles Chinese arguments:

```
Pipeline              Encoding                     Result
PowerShell argument → $OutputEncoding = US-ASCII   ❌ Chinese is truncated
                      ↓
lark-cli.exe receives GB2312 / mojibake            ❌ Parsing fails
```

### 7.2 ✅ The only reliable solution

**`cmd.exe /c` + `@file` to pass Chinese data**

```bash
# ✅ Correct: Chinese is written into a JSON file and loaded through @file
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --range A1:F1 --cells @data.json --as user
```

> Every command-line argument that contains Chinese (`--content`, `--cells`, `--headers`, …) **must** be passed with `@file` on Windows; you cannot write Chinese inline.

### 7.3 When `cmd.exe /c` is not needed

- When there are no Chinese arguments (pure English / numbers / booleans), you can call PowerShell directly
- But when loading a JSON file with `@file`, it is still recommended to prefix `cmd.exe /c` for consistency

### 7.4 JSON file conventions

```json
[
  [
    {"value": "中文文本"},
    {"value": 42},
    {"value": "是"}
  ]
]
```

- Save the file as **UTF-8 without BOM**
- Use English file names (e.g. `data.json`); avoid Chinese in the file name itself
- You can use the `file_write` tool to generate the JSON file

---

## 8. Common errors and handling

### Error 1: the command produces no output (exit code 1)

**Symptom:** after running the command there is no stdout/stderr, only exit code 1.

**Cause:** a PowerShell Chinese encoding problem — `lark-cli` silently exits after receiving garbled arguments.

**Fix:** use the `cmd.exe /c` prefix + `@file` to pass Chinese arguments.

---

### Error 2: `cross tenant p2p chat operate forbid` (230038)

**Symptom:** sending a message to a cross-enterprise user fails.

**Cause:** a Feishu platform restriction; direct private chat across enterprises is not allowed.

**Fix:**
- Add the other party to a group chat, then the Bot can send messages in the group
- Or chat with them manually in the Feishu App
- Or send the message to a user in the same enterprise

---

### Error 3: `Bot has NO availability to this user` (230013)

**Symptom:** sending a message with the Bot identity fails.

**Cause:** the recipient does not belong to the enterprise tenant where the Bot application lives.

**Fix:** same as error 2 — cross-enterprise users require a group chat or the user identity.

---

### Error 4: the scope does not take effect after authorization

**Symptom:** after `auth login --scope xxx` completes, the scope still does not appear in the `scopes` list.

**Cause:** the permission must first be enabled in the Feishu developer console, and then agreed to on the authorization page.

**Complete flow:**

```
1. Feishu developer console (https://open.feishu.cn/app)
   → select the application
   → Permission management → search the scope → enable it
   → Version management and release → create a version → publish

2. CLI authorization
   lark-cli auth login --scope "xxx" --json --no-wait
   → scan the QR code → confirm consent on the authorization page
   → lark-cli auth login --device-code <code>
   → verify: lark-cli auth scopes
```

---

### Error 5: `invalid JSON` / `expected type "array", got "string"`

**Symptom:** the `--cells` or `--headers` argument fails JSON parsing.

**Cause:**

| Common problem | Correct form |
|---|---|
| One-dimensional array `["a","b"]` | Two-dimensional array `[["a","b"]]` |
| A bare string value `"a"` | An object `{"value": "a"}` |
| Inline Chinese in PowerShell `'{"value":"中文"}'` | Load through `@file` |

---

### Error 6: `Unknown domain` or argument parsing failure

**Symptom:** `--domain im,docs` reports an error.

**Cause:** some versions of lark-cli do not support a comma-separated domain list.

**Fix:**
```bash
# ✅ Correct: repeat --domain
--domain im --domain docs --domain contact

# ❌ Wrong: comma-separated
--domain im,docs
```

---

### Error 7: the authorization page does not show the new scope

**Symptom:** after scanning, the permission list on the authorization page does not include the newly requested scope.

**Cause:**
1. The permission was not enabled in the developer console
2. It was enabled but no new version was published
3. It was enabled and published, but the release has not taken effect yet (wait 1-2 minutes)

**Fix:**
```bash
# Check whether the application already has this scope
lark-cli auth scopes | grep scope_name
```

---

## 9. Quick reference command table

### Configuration management

| Action | Command |
|---|---|
| View configuration | `lark-cli config show` |
| Initialize configuration | `lark-cli config init --new` |
| View identity status | `lark-cli auth status` |
| List available permissions | `lark-cli auth scopes` |

### Authorization management

| Action | Command |
|---|---|
| User login (get an authorization link) | `lark-cli auth login --json --no-wait` |
| Generate a QR code | `lark-cli auth qrcode <url> --output qr.png` |
| Complete authorization (poll) | `lark-cli auth login --device-code <code>` |
| Login with scopes | `lark-cli auth login --scope "scope1 scope2" --json --no-wait` |

### Messages

| Action | Command |
|---|---|
| Search users | `lark-cli contact +search-user --query "姓名" --as user` |
| Send a message (text) | `lark-cli im +messages-send --user-id ou_xxx --text "内容"` |
| Send a message (Markdown) | `lark-cli im +messages-send --user-id ou_xxx --markdown "**内容**"` |
| List group chats | `lark-cli im +chat-list` |

### Documents

| Action | Command |
|---|---|
| Create a document | `lark-cli docs +create --api-version v2 --content '<title>T</title>'` |
| Read a document | `lark-cli docs +read --doc-token xxx` |
| Append content | `lark-cli docs +write --doc-token xxx --content '<p>内容</p>' --append` |

### Spreadsheets

| Action | Command |
|---|---|
| Create a spreadsheet | `lark-cli sheets +workbook-create --title "表名" --as user` |
| Write data | `cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --range A1:F100 --cells @data.json --as user` |
| Read data (JSON) | `cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --range A1:F100 --as user` |
| Read data (CSV) | `cmd.exe /c lark-cli sheets +cells-get --spreadsheet-token xxx --range A1:F100 --format csv --as user` |

---

## Appendix: Complete onboarding flow (quick reference)

```bash
# 1. Install
npm install -g @larksuite/cli

# 2. Initialize the application
lark-cli config init --new
# → scan the QR code to configure appId + appSecret

# 3. Authorization login (first-time basic permissions: messages, documents)
lark-cli auth login --domain im --domain docs --json --no-wait
→ generate a QR code → scan it → authorization complete

# 4. Verify
lark-cli auth status                    # confirm the user is logged in
lark-cli contact +search-user --query "自己"  # confirm contacts can be searched

# 5. Send a test message
lark-cli im +messages-send --user-id ou_xxx --text "Hello" --as bot

# 6. Create a document
lark-cli docs +create --api-version v2 --content '<title>测试</title>' --as user

# 7. If spreadsheet permissions are needed, authorize them additionally
lark-cli auth login --scope "sheets:spreadsheet:create sheets:spreadsheet:write_only sheets:spreadsheet:read" --json --no-wait
→ scan the QR code to authorize → done

# 8. Create a spreadsheet
lark-cli sheets +workbook-create --title "测试表" --as user

# 9. Write data (use @file on Windows)
cmd.exe /c lark-cli sheets +cells-set --spreadsheet-token xxx --range A1:C3 --cells @data.json --as user
```

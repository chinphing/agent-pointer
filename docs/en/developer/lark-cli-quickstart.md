# Feishu CLI (lark-cli) initial setup and authorization SOP

English | [简体中文](../../zh-CN/developer/lark-cli-quickstart.md)

> Version: 1.0 | Platform: Windows | Tool: `@larksuite/cli`
>
> This page covers first-time setup and authorization only. For the complete command SOP (sending messages, docs, sheets, Chinese encoding, error handling) see [feishu-cli-integration-sop.md](feishu-cli-integration-sop.md).

---

## 1. Installation

```bash
npm install -g @larksuite/cli
```

Verify the installation:
```bash
lark-cli --version
```

---

## 2. Initial setup (Device Flow)

> ⚠️ `lark-cli config init --new` prints the QR code to stderr and blocks waiting for the scan, so it is unreliable.
> The right approach: use the device code flow to generate the QR code manually.

### 2.1 Get the verification link

```bash
lark-cli auth login --json --no-wait
```

Returns:
```json
{
  "device_code": "xxx",
  "verification_url": "https://accounts.feishu.cn/oauth/v1/device/verify?flow_id=xxx&user_code=xxx",
  "expires_in": 600
}
```

### 2.2 Generate the QR code (PNG image)

```bash
lark-cli auth qrcode "<verification_url>" --output qr.png --size 300
```

### 2.3 The user scans the code

Scan the QR code image generated in the previous step with the Feishu app.

### 2.4 Complete the authorization

```bash
lark-cli auth login --device-code "<device_code>"
```

Example output:
```
OK: 授权成功! 用户: 刘 (ou_xxx)
  本次请求 scopes: im:message, docx:document:create, ...
```

### 2.5 Verify

```bash
lark-cli config show
lark-cli auth status
```

---

## 3. Scope authorization

### 3.1 Check before authorizing

```bash
# 查看应用可用的全部权限
lark-cli auth scopes
```

### 3.2 Re-authorize with the required scopes

Add the scopes you need:
```bash
lark-cli auth login --scope "scope1 scope2" --json --no-wait
```

> The `--scope` argument takes several scopes separated by spaces, for example:
> `--scope "im:message docx:document:create sheets:spreadsheet:create"`

Then repeat steps 2.2 → 2.3 → 2.4.

### 3.3 The domain argument (mind the syntax)

```bash
# ✅ 正确：重复 --domain
--domain im --domain docs --domain contact

# ❌ 错误：逗号分隔会被当成一个 domain
--domain im,docs
```

### 3.4 Common scope list

| Scope | Purpose |
|---|---|
| `im:message` | Send messages (as the bot) |
| `im:message.send_as_user` | Send messages as the user (must be enabled in the developer console first) |
| `docx:document:create` | Create documents |
| `docx:document:readonly` | Read documents |
| `sheets:spreadsheet:create` | Create spreadsheets |
| `sheets:spreadsheet:write_only` | Write to spreadsheets |
| `sheets:spreadsheet:read` | Read spreadsheets |
| `contact:user:search` | Search contacts |
| `contact:user.base:readonly` | Read basic user information |

---

## 4. Full flow for adding a new scope

> Tools used: **Computer** (desktop browser automation)

```
Feishu developer console (https://open.feishu.cn/app)
  → select the app
  → Permission management → search the scope → enable
  → Version management & release → create a version → publish
  ↓
Re-authorize in the CLI
  → lark-cli auth login --scope "新scope" --json --no-wait
  → generate the QR code → the user scans to confirm
  → authorization complete
  ↓
Verify
  → lark-cli auth scopes  # confirm the new scope is in effect
```

> **Note:** after enabling a permission you must **publish a new version** in the developer console before that scope appears on the authorization page.

---

## 5. Verification checklist

- [ ] `lark-cli --version` → prints the version number normally
- [ ] `lark-cli config show` → has appId and appSecret
- [ ] `lark-cli auth status` → the user is signed in and the token is valid
- [ ] `lark-cli auth scopes` → all the required scopes are in the list
- [ ] Send a test message → `im +messages-send --text "test" --as bot`
- [ ] Create a test document → `docs +create --content '<p>test</p>' --as user`

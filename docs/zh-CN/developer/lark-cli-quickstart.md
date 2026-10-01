# 飞书 CLI（lark-cli）初始对接与授权 SOP

> 版本: 1.0 | 适用平台: Windows | 工具: `@larksuite/cli`

---

## 1. 安装

```bash
npm install -g @larksuite/cli
```

验证安装：
```bash
lark-cli --version
```

---

## 2. 初始对接（Device Flow）

> ⚠️ `lark-cli config init --new` 的二维码输出到 stderr 且阻塞等待扫码，不可靠。
> 正确做法：使用设备码流程手动生成二维码。

### 2.1 获取验证链接

```bash
lark-cli auth login --json --no-wait
```

返回：
```json
{
  "device_code": "xxx",
  "verification_url": "https://accounts.feishu.cn/oauth/v1/device/verify?flow_id=xxx&user_code=xxx",
  "expires_in": 600
}
```

### 2.2 生成二维码（PNG 图片）

```bash
lark-cli auth qrcode "<verification_url>" --output qr.png --size 300
```

### 2.3 用户扫码

用飞书 App 扫描上一步生成的二维码图片。

### 2.4 完成授权

```bash
lark-cli auth login --device-code "<device_code>"
```

输出示例：
```
OK: 授权成功! 用户: 刘 (ou_xxx)
  本次请求 scopes: im:message, docx:document:create, ...
```

### 2.5 验证

```bash
lark-cli config show
lark-cli auth status
```

---

## 3. 权限授权

### 3.1 授权前检查

```bash
# 查看应用可用的全部权限
lark-cli auth scopes
```

### 3.2 携带指定权限重新授权

添加所需 scope：
```bash
lark-cli auth login --scope "scope1 scope2" --json --no-wait
```

> `--scope` 参数多个 scope 用空格分隔，如：
> `--scope "im:message docx:document:create sheets:spreadsheet:create"`

然后重复 2.2 → 2.3 → 2.4 三步。

### 3.3 domain 参数（注意语法）

```bash
# ✅ 正确：重复 --domain
--domain im --domain docs --domain contact

# ❌ 错误：逗号分隔会被当成一个 domain
--domain im,docs
```

### 3.4 常用 scope 清单

| 权限 Scope | 用途 |
|---|---|
| `im:message` | 发送消息（Bot 身份） |
| `im:message.send_as_user` | 以用户身份发消息（需开发者后台先开启） |
| `docx:document:create` | 创建文档 |
| `docx:document:readonly` | 读取文档 |
| `sheets:spreadsheet:create` | 创建电子表格 |
| `sheets:spreadsheet:write_only` | 写入电子表格 |
| `sheets:spreadsheet:read` | 读取电子表格 |
| `contact:user:search` | 搜索联系人 |
| `contact:user.base:readonly` | 读取用户基本信息 |

---

## 4. 添加新 scope 的完整流程

> 使用到的工具：**Computer**（桌面浏览器自动化）

```
飞书开发者后台 (https://open.feishu.cn/app)
  → 选择应用
  → 权限管理 → 搜索 scope → 开启
  → 版本管理与发布 → 创建版本 → 发布
  ↓
CLI 重新授权
  → lark-cli auth login --scope "新scope" --json --no-wait
  → 生成二维码 → 用户扫码确认
  → 完成授权
  ↓
验证
  → lark-cli auth scopes  # 确认新 scope 已生效
```

> **注意：** 开启权限后必须在开发者后台**发布新版本**，授权页面才会出现该 scope。

---

## 5. 验证 checklist

- [ ] `lark-cli --version` → 正常显示版本号
- [ ] `lark-cli config show` → 有 appId 和 appSecret
- [ ] `lark-cli auth status` → 用户已登录，token 有效
- [ ] `lark-cli auth scopes` → 需要的权限都在列表中
- [ ] 发送测试消息 → `im +messages-send --text "test" --as bot`
- [ ] 创建测试文档 → `docs +create --content '<p>test</p>' --as user`

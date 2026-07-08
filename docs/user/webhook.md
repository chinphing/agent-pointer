# Webhook 自动化

通过 HTTP 从 CI、脚本或外部系统触发 Pointer Agent，并可选附带附件。

## 前置条件

- 已部署并暴露公网地址的 **Pointer server**（云主机或自部署实例）
- 在 **设置 → 自动化 → Webhook** 中添加来源（如 `ci`），记录 **来源 id** 与 **Token**

**Token 保存说明：**

- 列表中显示尾号（如 `****1d34`），点击即可复制完整 Token（需已登录平台账户）。
- 添加来源时 Token 也会自动复制到剪贴板。
- 若遗失 Token：删除该来源后重新添加（Token 仅可设置一次，不可覆盖）。

同一来源在本地日历日（04:00 切换）内会续接同一会话；跨日自动开新会话。

## 调用流程

1. **上传附件**（可选，支持多个）：`POST /api/webhooks/<来源>/upload`
2. **触发 Agent**：`POST /api/webhooks/<来源>`，请求体带描述文本与附件引用
3. 设 `blocking: true` 可同步等待 Agent 回复；默认异步返回 `runId`

大文件请先上传再引用，不要把文件塞进 JSON。单文件上传上限 **30 MiB**。

完整 API 说明见 **[`../developer/webhook-api.md`](../developer/webhook-api.md)**。

## Python 示例

依赖：

```bash
pip install requests
```

脚本（支持多附件；在顶部配置域名、来源 id 与 Token）：

```python
#!/usr/bin/env python3
"""
Pointer Webhook 调用示例（支持多附件）
- 输入：描述文本、可选多个附件路径
- 输出：Agent 同步回复（blocking 模式）
"""

from __future__ import annotations

import argparse
import json
import mimetypes
import sys
from pathlib import Path
from typing import Any

import requests

# ============ 在此配置 ============
WEBHOOK_HOST = "https://your-pointer-host"  # Pointer server 公网地址，不含末尾 /
WEBHOOK_SRC = "ci"                          # 自动化面板里配置的来源 id
WEBHOOK_TOKEN = "your-token-here"           # 面板生成的 Token
TIMEOUT_SECONDS = 300                       # 同步等待 Agent 的最长时间（最大 600）
# =================================


class PointerWebhookError(Exception):
    pass


def _headers() -> dict[str, str]:
    return {
        "Authorization": f"Bearer {WEBHOOK_TOKEN}",
    }


def _raise_for_status(resp: requests.Response, action: str) -> None:
    if resp.ok:
        return
    detail = resp.text.strip() or resp.reason
    raise PointerWebhookError(f"{action} failed ({resp.status_code}): {detail}")


def upload_attachment(file_path: Path, conversation_id: str | None = None) -> dict[str, Any]:
    """POST /api/webhooks/:src/upload"""
    if not file_path.is_file():
        raise PointerWebhookError(f"attachment not found: {file_path}")

    mime_type, _ = mimetypes.guess_type(file_path.name)
    data: dict[str, str] = {}
    if conversation_id:
        data["conversationId"] = conversation_id

    with file_path.open("rb") as f:
        files = {
            "file": (file_path.name, f, mime_type or "application/octet-stream"),
        }
        resp = requests.post(
            f"{WEBHOOK_HOST}/api/webhooks/{WEBHOOK_SRC}/upload",
            headers=_headers(),
            data=data,
            files=files,
            timeout=120,
        )

    _raise_for_status(resp, f"upload {file_path.name}")
    return resp.json()


def upload_attachments(file_paths: list[Path]) -> tuple[str, list[dict[str, Any]]]:
    """上传多个文件，复用同一会话 conversationId。"""
    if not file_paths:
        return "", []

    conversation_id: str | None = None
    attachments: list[dict[str, Any]] = []

    for file_path in file_paths:
        uploaded = upload_attachment(file_path, conversation_id=conversation_id)
        conversation_id = uploaded["conversationId"]
        attachments.append(
            {
                "id": uploaded["attachmentId"],
                "kind": uploaded["kind"],
                "mimeType": uploaded["mimeType"],
                "fileName": uploaded["fileName"],
                "storageRelPath": uploaded["storageRelPath"],
            }
        )

    return conversation_id or "", attachments


def trigger_webhook(
    description: str,
    attachments: list[dict[str, Any]] | None = None,
    conversation_id: str | None = None,
) -> dict[str, Any]:
    """POST /api/webhooks/:src（blocking=true 同步等待结果）"""
    payload: dict[str, Any] = {
        "text": description,
        "blocking": True,
        "timeoutSeconds": TIMEOUT_SECONDS,
    }
    if conversation_id:
        payload["conversationId"] = conversation_id
    if attachments:
        payload["attachments"] = attachments

    resp = requests.post(
        f"{WEBHOOK_HOST}/api/webhooks/{WEBHOOK_SRC}",
        headers={**_headers(), "Content-Type": "application/json"},
        json=payload,
        timeout=TIMEOUT_SECONDS + 30,
    )

    _raise_for_status(resp, "trigger")
    return resp.json()


def call_pointer_webhook(
    description: str,
    attachment_paths: list[str] | None = None,
) -> dict[str, Any]:
    """
    主流程：可选上传多个附件 -> 触发 Agent -> 返回结果

    成功时返回示例：
    {
      "ok": true,
      "runId": "...",
      "conversationId": "webhook:ci:20260708",
      "text": "Agent 的最终回复"
    }
    """
    conversation_id: str | None = None
    attachments: list[dict[str, Any]] | None = None

    if attachment_paths:
        paths = [Path(p) for p in attachment_paths]
        conversation_id, attachments = upload_attachments(paths)

    return trigger_webhook(
        description=description,
        attachments=attachments,
        conversation_id=conversation_id,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Call Pointer webhook with text and optional attachments",
    )
    parser.add_argument("description", help="发给 Agent 的任务描述")
    parser.add_argument(
        "-a", "--attachment",
        dest="attachment_paths",
        action="append",
        default=[],
        help="本地附件路径，可重复指定多次：-a a.pdf -a b.xlsx",
    )
    args = parser.parse_args()

    try:
        result = call_pointer_webhook(args.description, args.attachment_paths or None)
    except PointerWebhookError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    except requests.RequestException as e:
        print(f"Network error: {e}", file=sys.stderr)
        return 1

    print(json.dumps(result, ensure_ascii=False, indent=2))
    print("\n--- Agent Reply ---\n")
    print(result.get("text", ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

## 命令行用法

```bash
# 仅文本
python pointer_webhook_example.py "请总结今天的构建情况"

# 单个附件
python pointer_webhook_example.py "请分析这份报告" -a ./report.pdf

# 多个附件
python pointer_webhook_example.py "对比这两份文档" \
  -a ./report_v1.pdf \
  -a ./report_v2.pdf \
  -a ./summary.xlsx
```

## 在代码中调用

```python
result = call_pointer_webhook(
    "请汇总这些附件",
    attachment_paths=["./a.pdf", "./b.png", "./c.xlsx"],
)
print(result["text"])
```

## 常见问题

| 现象 | 处理 |
|------|------|
| 401 | 检查 Token 与来源 id 是否与自动化面板一致 |
| Token 不可用 | 确认已登录平台账户；删除来源后重新添加 |
| 504 | 增大 `TIMEOUT_SECONDS`（最大 600），或改用异步（`blocking: false`） |
| 附件找不到 | 先 upload，触发时使用返回的 `storageRelPath`，并保持同一 `conversationId` |
| 413 | 单文件超过 30 MiB，需拆分或改用其他传递方式 |

若来源配置了自定义鉴权 Header（非 `Authorization`），将脚本中 `_headers()` 改为对应头名与原始 Token 值。

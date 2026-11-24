#!/usr/bin/env python3
"""§5.6 隔离验收七条：对着 E2B 兼容网关（e2b-local / Cube）跑一遍。

用法：
    POINTER_SANDBOX_URL=http://127.0.0.1:3000 python3 scripts/verify-sandbox-isolation.py

脚本按 Pointer 现行挂载模型建卷（`POST /volumes`）并把 `/home/pointer`、`/data`
挂进沙箱（与 `env/mounts.rs` 的 home/data 双卷一致）。卷根取
`E2B_LOCAL_VOLUMES_DIR`（默认 `~/.e2b-local/volumes`），必须与网关配置的
`docker.volume_host_path` 一致。

每条检查输出 PASS / FAIL / SKIP，任一 FAIL 退出码 1。
SKIP 表示"当前后端不具备该能力"（例如 e2b-local 忽略 `readOnly`、以 root 跑），
不是通过——Cube 上必须全部 PASS。
"""

import base64
import json
import os
import shutil
import sys
import urllib.error
import urllib.request

CONTROL = os.environ.get("POINTER_SANDBOX_URL", "http://127.0.0.1:3000").rstrip("/")
TEMPLATE = os.environ.get("POINTER_SANDBOX_TEMPLATE", "debian:bookworm-slim")
TIMEOUT = int(os.environ.get("POINTER_SANDBOX_TIMEOUT", "600"))
VOLUMES = os.path.expanduser(
    os.environ.get("E2B_LOCAL_VOLUMES_DIR", "~/.e2b-local/volumes")
)
# (卷名, 沙箱内路径) — 与 mounts.rs：home + data
MOUNTS = [("iso-home", "/home/pointer"), ("iso-data", "/data")]


def http(method, url, data=None, ctype=None, timeout=120):
    req = urllib.request.Request(
        url, data=data, method=method, headers={"Content-Type": ctype} if ctype else {}
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except Exception as e:  # noqa: BLE001 - 网络层错误统一当失败证据
        return 0, str(e).encode()


def connect_exec(envd, script):
    """跑一段 bash 脚本，返回 (exit_code, stdout, stderr)。"""
    payload = json.dumps({"process": {"cmd": "/bin/bash", "args": ["-lc", script]}}).encode()
    body = b"\x00" + len(payload).to_bytes(4, "big") + payload
    status, raw = http(
        "POST",
        envd + "/process.Process/Start",
        body,
        "application/connect+json",
    )
    if status // 100 != 2:
        return -1, "", f"envd HTTP {status}: {raw[:200].decode(errors='replace')}"
    stdout = []
    stderr = []
    exit_code = 0
    i = 0
    while i + 5 <= len(raw):
        flags = raw[i]
        n = int.from_bytes(raw[i + 1 : i + 5], "big")
        i += 5
        chunk = raw[i : i + n]
        i += n
        if not chunk:
            continue
        try:
            msg = json.loads(chunk)
        except json.JSONDecodeError:
            continue
        event = msg.get("event") or {}
        data = event.get("data") or {}
        if "stdout" in data:
            stdout.append(base64.b64decode(data["stdout"]))
        if "stderr" in data:
            stderr.append(base64.b64decode(data["stderr"]))
        end = event.get("end") or {}
        if "exitCode" in end:
            exit_code = int(end["exitCode"])
        if flags & 0x02:
            break
    return (
        exit_code,
        b"".join(stdout).decode(errors="replace"),
        b"".join(stderr).decode(errors="replace"),
    )


def prepare_volumes():
    """在卷根下准备 iso-home / iso-data，并尽量经控制面注册。"""
    os.makedirs(VOLUMES, exist_ok=True)
    for name, _ in MOUNTS:
        path = os.path.join(VOLUMES, name)
        if os.path.isdir(path):
            shutil.rmtree(path, ignore_errors=True)
        os.makedirs(path, exist_ok=True)
        # 写入探针，方便确认双向可见
        with open(os.path.join(path, ".pointer-iso"), "w", encoding="utf-8") as f:
            f.write(name + "\n")
    # 控制面建卷（部分网关要求先 POST /volumes）
    for name, _ in MOUNTS:
        http(
            "POST",
            CONTROL + "/volumes",
            json.dumps({"name": name}).encode(),
            "application/json",
            timeout=30,
        )
    return True


def main():
    with_mounts = prepare_volumes()
    create_body = {"templateID": TEMPLATE, "timeout": TIMEOUT}
    if with_mounts:
        create_body["volumeMounts"] = [
            {"name": name, "path": path, "readOnly": False} for name, path in MOUNTS
        ]
    status, body = http(
        "POST", CONTROL + "/sandboxes", json.dumps(create_body).encode(), "application/json"
    )
    if status // 100 != 2:
        print(f"create sandbox failed: HTTP {status} {body[:200]!r}")
        return 2
    info = json.loads(body)
    envd, sid = info["envdURL"], info["sandboxID"]
    print(f"sandbox={sid} envd={envd}")

    results = []
    try:
        code, out, _ = connect_exec(
            envd, "mount | grep -E ' /(data|home/pointer) ' | wc -l"
        )
        mount_count = int(out.strip()) if out.strip().isdigit() else 0
        has_mounts = mount_count >= 2
        code, out, _ = connect_exec(envd, "id -u")
        uid = int(out.strip()) if out.strip().isdigit() else -1
        print(f"backend: mounts={mount_count} uid={uid} template={TEMPLATE}\n")

        def check(name, fn, needs_mounts=False):
            if needs_mounts and not has_mounts:
                results.append((name, "SKIP", "后端未挂载（无 volumeMounts 支持）"))
                return
            verdict, detail = fn()
            results.append((name, verdict, detail))

        def cross_user():
            # 只挂了当前用户的家目录：/home 下不应出现别的租户
            code, out, err = connect_exec(
                envd,
                "ls -1 /home 2>&1; echo '---'; ls /home/pointer/../other-tenant 2>&1",
            )
            text = out + err
            listed = [l for l in text.split("---")[0].split() if l]
            ok = listed == ["pointer"] and (
                "No such file" in text or "cannot access" in text
            )
            return ("PASS" if ok else "FAIL"), f"home={listed} {text.split('---')[-1].strip()[:60]}"

        def path_escape():
            # 从挂载点往上逃，不能落到宿主文件系统
            code, out, err = connect_exec(
                envd,
                "cat /data/../../Users/$(whoami)/.zshrc 2>&1 | head -1; "
                "ls -d /Users 2>&1 | head -1",
            )
            text = out + err
            ok = "No such file" in text or "cannot access" in text or "not found" in text
            return ("PASS" if ok else "FAIL"), text.strip()[:100]

        def data_writable():
            code, out, err = connect_exec(
                envd, "echo ok >/data/.iso-write && cat /data/.iso-write"
            )
            text = (out + err).strip()
            ok = code == 0 and "ok" in text
            return ("PASS" if ok else "FAIL"), text[:100]

        def home_writable():
            code, out, err = connect_exec(
                envd, "echo ok >/home/pointer/.iso-write && cat /home/pointer/.iso-write"
            )
            text = (out + err).strip()
            ok = code == 0 and "ok" in text
            return ("PASS" if ok else "FAIL"), text[:100]

        def network_ok():
            code, out, err = connect_exec(
                envd,
                "timeout 8 bash -c 'exec 3<>/dev/tcp/1.1.1.1/443' && echo ok || echo blocked",
            )
            ok = "ok" in out
            return ("PASS" if ok else "FAIL"), (out + err).strip()[:100]

        def no_host_tokens_in_data():
            # data 卷不应夹带宿主凭证文件名（探针）
            code, out, err = connect_exec(
                envd,
                "find /data \\( -name 'token.txt' -o -name '*_token.txt' \\) 2>&1 | head -5",
            )
            ok = code == 0 and not out.strip()
            return ("PASS" if ok else "FAIL"), (out + err).strip()[:140]

        def non_root():
            if uid < 0:
                return "SKIP", "取不到 uid"
            if uid == 0:
                return "SKIP", "后端无 user namespace（e2b-local 以 root 跑）"
            return ("PASS" if uid >= 100000 else "FAIL"), f"uid={uid}"

        check("1 跨用户不可见", cross_user, needs_mounts=True)
        check("2 路径逃逸无效", path_escape, needs_mounts=True)
        check("3 data 可写", data_writable, needs_mounts=True)
        check("4 home 可写", home_writable, needs_mounts=True)
        check("5 出网可用", network_ok)
        check("6 data 无宿主凭证", no_host_tokens_in_data, needs_mounts=True)
        check("7 非宿主 uid", non_root)
    finally:
        http("DELETE", CONTROL + f"/sandboxes/{sid}")

    print(f"{'检查':<22} {'结果':<6} 证据")
    print("-" * 78)
    failed = 0
    for name, verdict, detail in results:
        if verdict == "FAIL":
            failed += 1
        print(f"{name:<22} {verdict:<6} {detail}")

    skipped = sum(1 for _, v, _ in results if v == "SKIP")
    print(f"\n汇总：PASS {len(results) - failed - skipped} / FAIL {failed} / SKIP {skipped}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

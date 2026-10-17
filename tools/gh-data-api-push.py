#!/usr/bin/env python3
"""通过 GitHub REST API 上传整个仓库（一次性建一个提交）。

为什么不用 git push：本机到 github.com:443 的 git 传输不通（连接超时），
但 api.github.com 通（gh 的 API 调用一直正常）。走 Git Data API：
blob（逐文件）→ tree → commit → ref。
"""
import base64
import json
import os
import subprocess
import sys
import urllib.request

OWNER, REPO = "chinphing", "screenspot-pro-bench"
API = f"https://api.github.com/repos/{OWNER}/{REPO}"
ROOT = "/Users/starliu/pointer-all/agent-pointer/benchmarks/screenspot_grid"

TOKEN = subprocess.check_output(["gh", "auth", "token"], text=True).strip()


def call(method, path, body=None):
    req = urllib.request.Request(
        API + path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization": f"Bearer {TOKEN}",
                 "Accept": "application/vnd.github+json",
                 "User-Agent": "screenspot-push"})
    with urllib.request.urlopen(req, timeout=90) as r:
        return json.load(r)


def main():
    os.chdir(ROOT)
    files = subprocess.check_output(["git", "ls-files"], text=True).split()
    msg = subprocess.check_output(["git", "log", "-1", "--format=%B"], text=True).strip()
    print(f"  待上传 {len(files)} 个文件")

    tree, fail = [], []
    for i, p in enumerate(files, 1):
        try:
            data = open(p, "rb").read()
            b = call("POST", "/git/blobs",
                     {"content": base64.b64encode(data).decode(), "encoding": "base64"})
            tree.append({"path": p,
                         "mode": "100755" if os.access(p, os.X_OK) else "100644",
                         "type": "blob", "sha": b["sha"]})
        except Exception as e:  # noqa: BLE001
            fail.append(f"{p}: {type(e).__name__}: {e}")
        if i % 25 == 0 or i == len(files):
            print(f"  [{i}/{len(files)}] 失败 {len(fail)}", flush=True)

    if fail:
        print("  ❌ 上传失败：")
        for x in fail[:10]:
            print(f"    {x}")
        return 1

    t = call("POST", "/git/trees", {"tree": tree})
    c = call("POST", "/git/commits", {"message": msg, "tree": t["sha"]})
    call("POST", "/git/refs", {"ref": "refs/heads/main", "sha": c["sha"]})
    print(f"  ✅ 已提交 {c['sha'][:8]}（{len(tree)} 个文件）")
    return 0


if __name__ == "__main__":
    sys.exit(main())

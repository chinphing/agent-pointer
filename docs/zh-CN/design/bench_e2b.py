#!/usr/bin/env python3
"""实测真实 E2B envd 数据面（e2b-local + OrbStack）：exec 与文件 IO 延迟。

控制面：POST /sandboxes
数据面：envd 直连（files 走 REST，process 走 Connect streaming）
"""
import json
import statistics
import time
import urllib.request

API = "http://127.0.0.1:3000"


def post(path, obj, base=API, ctype="application/json", raw=None):
    data = raw if raw is not None else json.dumps(obj).encode()
    req = urllib.request.Request(base + path, data=data, headers={"Content-Type": ctype})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def get(path, base=API):
    with urllib.request.urlopen(base + path, timeout=120) as r:
        return r.read()


def connect_exec(envd, cmd, n=1):
    """Connect server-streaming 调用 process.Process/Start。"""
    payload = json.dumps({"process": {"cmd": "/bin/sh", "args": ["-c", cmd]}}).encode()
    body = b"\x00" + len(payload).to_bytes(4, "big") + payload
    req = urllib.request.Request(
        envd + "/process.Process/Start", data=body,
        headers={"Content-Type": "application/connect+json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        raw = r.read()
    out, i = [], 0
    while i + 5 <= len(raw):
        flags = raw[i]
        ln = int.from_bytes(raw[i + 1:i + 5], "big")
        seg = raw[i + 5:i + 5 + ln]
        i += 5 + ln
        if flags & 0x02:
            break
        try:
            out.append(json.loads(seg))
        except Exception:
            pass
    return out


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(len(xs) * p))]


def bench(fn, n=30, warmup=3):
    for _ in range(warmup):
        fn()
    ts = []
    for _ in range(n):
        t0 = time.perf_counter()
        fn()
        ts.append((time.perf_counter() - t0) * 1000.0)
    return {"median_ms": round(statistics.median(ts), 2), "p95_ms": round(pct(ts, 0.95), 2)}


out = {}
t0 = time.perf_counter()
info = json.loads(post("/sandboxes", {"templateID": "debian:bookworm-slim", "timeout": 900}))
out["sandbox_create"] = {"ms": round((time.perf_counter() - t0) * 1000, 1)}
E, SID = info["envdURL"], info["sandboxID"]
out["envd"] = E

try:
    # ---------- exec ----------
    out["exec_true"] = bench(lambda: connect_exec(E, "true"))
    out["exec_echo"] = bench(lambda: connect_exec(E, "echo hi"))

    # ---------- 文件 ----------
    for name, size in {"1KB": 1024, "100KB": 100 * 1024, "1MB": 1024 * 1024}.items():
        blob = b"x" * size
        p = f"/tmp/{name}.bin"
        post(f"/files?path={p}&username=root", None, base=E,
             ctype="application/octet-stream", raw=blob)
        out[f"file_write_{name}"] = bench(
            lambda pp=p, b=blob: post(f"/files?path={pp}&username=root", None, base=E,
                                      ctype="application/octet-stream", raw=b), n=15)
        out[f"file_read_{name}"] = bench(
            lambda pp=p: get(f"/files?path={pp}&username=root", base=E), n=15)

    # ---------- 批量 grep：逐文件 vs 一次 exec ----------
    print(json.dumps({k: v for k, v in out.items() if k != "envd"}, ensure_ascii=False), flush=True)
    try:
        r = connect_exec(E, "mkdir -p /tmp/tree && cd /tmp/tree && "
                            "for i in $(seq 1 200); do "
                            "echo 'lorem ipsum' > f$i.txt; "
                            "done; echo NEEDLE >> f50.txt; ls /tmp/tree | wc -l")
        print("tree:", json.dumps(r)[:200], flush=True)
        names = [f"/tmp/tree/f{i}.txt" for i in range(1, 201)]
        out["grep_200_perfile"] = bench(
            lambda: [get(f"/files?path={n}&username=root", base=E) for n in names], n=3, warmup=1)
        out["grep_200_one_exec"] = bench(
            lambda: connect_exec(E, "grep -rl NEEDLE /tmp/tree | wc -l"), n=10)
    except Exception as ex:
        out["batch_error"] = str(ex)[:200]

    out["ok"] = True
finally:
    try:
        urllib.request.urlopen(urllib.request.Request(f"{API}/sandboxes/{SID}", method="DELETE"), timeout=30)
    except Exception:
        pass

print(json.dumps(out, indent=2, ensure_ascii=False))

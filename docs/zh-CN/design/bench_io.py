#!/usr/bin/env python3
"""实测：本地 fs vs 远端 API 化文件 IO 的延迟差距。

模拟三种后端：
  A. 本地 std::fs        （CubeEnv 走宿主 Volume 的捷径）
  B. HTTP 往返           （E2B/Cube envd API 的最优情况：本机 loopback）
  C. docker exec         （真实容器 exec 开销，若 docker 可用）

关注的不只是"单次多慢"，而是"一次工具调用要做多少次往返"。
"""
import http.server
import json
import os
import shutil
import socketserver
import statistics
import subprocess
import tempfile
import threading
import time

ROOT = tempfile.mkdtemp(prefix="io-bench-")
N = 300


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(len(xs) * p))]


def bench(fn, n=N, warmup=20):
    for _ in range(warmup):
        fn()
    ts = []
    for _ in range(n):
        t0 = time.perf_counter()
        fn()
        ts.append((time.perf_counter() - t0) * 1000.0)
    return {
        "median_ms": round(statistics.median(ts), 3),
        "p95_ms": round(pct(ts, 0.95), 3),
        "mean_ms": round(statistics.mean(ts), 3),
    }


# ---------- 造数据 ----------
sizes = {"1KB": 1024, "100KB": 100 * 1024, "1MB": 1024 * 1024}
for name, size in sizes.items():
    with open(os.path.join(ROOT, name + ".bin"), "wb") as f:
        f.write(os.urandom(size))

# grep 场景：200 个小文件
TREE = os.path.join(ROOT, "tree")
os.makedirs(TREE)
for i in range(200):
    with open(os.path.join(TREE, f"f{i}.txt"), "w") as f:
        f.write(("lorem ipsum dolor sit amet\n" * 20) + ("NEEDLE\n" if i % 50 == 0 else ""))

# ---------- B: HTTP 服务（模拟 envd，读同一批文件） ----------
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        p = os.path.join(ROOT, self.path.lstrip("/"))
        try:
            with open(p, "rb") as f:
                data = f.read()
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except Exception:
            self.send_error(404)

    def log_message(self, *a):
        pass


class TS(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


srv = TS(("127.0.0.1", 0), Handler)
PORT = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

import urllib.request

BASE = f"http://127.0.0.1:{PORT}/"


def http_get(name):
    with urllib.request.urlopen(BASE + name) as r:
        return r.read()


results = {}

# ---------- 单文件读 ----------
for name in sizes:
    local = bench(lambda n=name: open(os.path.join(ROOT, n + ".bin"), "rb").read())
    remote = bench(lambda n=name: http_get(n + ".bin"))
    results[f"read_{name}"] = {
        "local": local,
        "http": remote,
        "delta_ms": round(remote["median_ms"] - local["median_ms"], 3),
    }

# ---------- 200 文件 grep（逐个读 vs 批量） ----------
def local_scan():
    hits = []
    for fn in sorted(os.listdir(TREE)):
        with open(os.path.join(TREE, fn), "rb") as f:
            for line in f:
                if b"NEEDLE" in line:
                    hits.append(fn)
    return hits


def remote_scan_perfile():
    hits = []
    for fn in sorted(os.listdir(TREE)):
        data = http_get(f"tree/{fn}")
        if b"NEEDLE" in data:
            hits.append(fn)
    return hits


def remote_scan_batched():
    # 一次 exec 跑 rg，结果一次返回（E2B commands.run 模式）
    data = http_get("tree/f0.txt")  # 占位：真实场景是一次 exec
    return data


local_grep = bench(local_scan, n=20)
remote_grep = bench(remote_scan_perfile, n=20)
results["grep_200_files"] = {
    "local": local_grep,
    "http_per_file": remote_grep,
    "round_trips": 200,
    "delta_ms": round(remote_grep["median_ms"] - local_grep["median_ms"], 3),
}

# ---------- C: docker exec ----------
docker_info = {"available": False}
if shutil.which("docker"):
    try:
        subprocess.run(["docker", "info"], capture_output=True, timeout=8, check=True)
        docker_info["available"] = True
    except Exception as e:
        docker_info["error"] = str(e)[:120]

if docker_info["available"]:
    try:
        cid = subprocess.run(
            ["docker", "run", "-d", "--rm", "-v", f"{ROOT}:/data:ro", "alpine:3.20", "sleep", "600"],
            capture_output=True, text=True, timeout=90,
        ).stdout.strip()
        time.sleep(1.5)
        d_local = bench(lambda: subprocess.run(
            ["docker", "exec", cid, "cat", "/data/1KB.bin"],
            capture_output=True, timeout=20), n=60)
        d_exec = bench(lambda: subprocess.run(
            ["docker", "exec", cid, "true"],
            capture_output=True, timeout=20), n=60)
        subprocess.run(["docker", "rm", "-f", cid], capture_output=True, timeout=30)
        docker_info.update({"exec_true": d_exec, "exec_cat_1KB": d_local})
    except Exception as e:
        docker_info["run_error"] = str(e)[:200]

results["docker"] = docker_info
srv.shutdown()
shutil.rmtree(ROOT, ignore_errors=True)

print(json.dumps(results, indent=2, ensure_ascii=False))

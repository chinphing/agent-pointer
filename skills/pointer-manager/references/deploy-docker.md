# Docker 后端部署

服务端让 agent 执行命令、读写文件、开浏览器，先要有能跑的容器镜像。

## 宿主前提

| 项 | 要求 | 怎么查 |
|---|---|---|
| Docker | Engine 或 Desktop，**CLI 在 PATH 上** | `docker version` |
| 权限 | 服务用户能访问 socket | `docker ps` 不报 permission |
| 磁盘 | 镜像 1.5 GB + 每容器可写层 | `df -h` |
| 内存 | 每会话 4 GB（默认配额） | `free -g` |

Pointer 走 `docker` 命令行（不是 SDK），**只有守护进程没有 CLI 不行**。

## 镜像从哪来

### A. 本机构建

```bash
docker build -f docker/sandbox-container.Dockerfile -t pointer-sandbox-container:latest .
docker build -f docker/sandbox-light.Dockerfile     -t pointer-sandbox-light:latest     .
```

- Dockerfile 里 apt 源默认 `mirrors.aliyun.com`（buildkit 直连 deb.debian.org 常失败）
- 首次约 3 分钟，之后增量十几秒
- 构建完 `docker images | grep pointer-sandbox` 确认

服务端包**自带 `docker/` 构建上下文**（zip 解压目录下；deb 在
`/usr/share/pointer-server/docker/`），目标机只要有 Docker 和网络就能直接构建，
不用再拿仓库：

```bash
cd pointer-server
docker build -f docker/sandbox-container.Dockerfile -t pointer-sandbox-container:latest .
```

仍然需要联网拉基础镜像（`python:3.12-slim` / `node:24-bookworm-slim`）和 apt 包。

### B. 私有仓库

```bash
docker pull registry.example.com/pointer/sandbox-container:0.1.3
docker tag  registry.example.com/pointer/sandbox-container:0.1.3 pointer-sandbox-container:latest
```

### C. 离线 tar（内网 / 无仓库）

```bash
# 有网的机器
docker save pointer-sandbox-container:latest | gzip > sandbox-container.tar.gz
# 目标机
gunzip -c sandbox-container.tar.gz | docker load
```

## 配置

```toml
[environment]
provider = "container"
backend  = "docker"
template = "pointer-sandbox-container:latest"
cpus      = 2
memory_mb = 4096
disk_mb   = 10240
```

`provider = "light"` 时用 `pointer-sandbox-light:latest`（无桌面，更小）。

对应环境变量：`POINTER_SERVER_ENVIRONMENT_PROVIDER` / `_BACKEND` / `_TEMPLATE` /
`_CPUS` / `_MEMORY_MB` / `_DISK_MB`。

## 验证

```bash
# 1. 默认用户不是 root
docker run --rm pointer-sandbox-container:latest id
# 期望：uid=1000(pointer) gid=1000(pointer)

# 2. 桌面能起来（container 档位）
docker run -d --name sbx-smoke pointer-sandbox-container:latest sleep infinity
sleep 6
docker exec sbx-smoke sh -lc 'DISPLAY=:99 xdpyinfo | grep dimensions'
# 期望：1920x1080 pixels
docker rm -f sbx-smoke

# 3. 浏览器能起（可选）
docker run -d --name sbx-br pointer-sandbox-container:latest sleep infinity
sleep 6
docker exec sbx-br sh -lc 'DISPLAY=:99 nohup browser about:blank >/dev/null 2>&1 & sleep 8;
  DISPLAY=:99 xdotool search --class chromium | head -1'
docker rm -f sbx-br
```

服务端侧：开会话让 agent 跑 `pwd`，返回 `/workspace` 即通。

## 容量规划

每会话一个容器，空闲 30 分钟回收。默认 2 核 / 4 GB：

| 并发会话 | 内存 | 建议 |
|---|---|---|
| 5 | 20 GB | 默认配额即可 |
| 20 | 80 GB | 降到 2048 MB |
| 50 | 200 GB | 2048 MB + 关桌面（light 档位） |

实测参考：空桌面容器 49 MB，开浏览器 230 MB，可写层 3.8 MB。
配额是上限不是占用，按并发留余量。

## 常见问题

| 现象 | 原因 | 处理 |
|---|---|---|
| `No such file or directory (os error 2)` | 没装 Docker / CLI 不在 PATH | 装 Docker |
| `permission denied ... docker.sock` | 不在 docker 组 | `usermod -aG docker <user>` 后重登 |
| 改了镜像行为没变 | 指纹只 hash 镜像名 | `docker rm -f pointer-sbx-*`，或用版本 tag |
| 桌面起不来 | `/tmp/.X11-unix` 缺失 | 镜像需预建并 `chmod 1777` |
| 浏览器 "profile in use" | 旧容器 `SingletonLock` 残留 | entrypoint 已处理，确认镜像版本 |
| 构建报 `input/output error` | 宿主磁盘满 | 清空间：`cargo clean`、`docker builder prune -af` |

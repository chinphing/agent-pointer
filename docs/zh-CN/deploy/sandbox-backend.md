# 沙箱执行环境部署（docker / envd）

[English](../../en/deploy/sandbox-backend.md) | 简体中文

服务端要让 agent 真正执行命令、读写文件、开浏览器，得先给它一个**执行环境**。
Pointer 支持两种后端，选错会在上线后返工，所以先看第 0 节。

---

<a id="choose"></a>
## 0. 先选后端

| 场景 | 档位 | 后端 | 理由 |
|---|---|---|---|
| 桌面 App（用户自己电脑） | `local` | `host` | 直接用宿主，不隔离 |
| 单机 web，使用者是同事 | `container` / `light` | `docker` | 隔离边界就是这台机器，风险可接受 |
| **多用户 web（互不可信）** | `container` / `light` | `envd` + Cube | 每人独立内核，容器共享内核不够 |
| 已有 K8s 集群 | `container` | `docker`（或自建 Kata/gVisor） | 别重复造轮子 |

判断口诀：**同一台机器上跑的是不是"别人的代码"**。
是 → 上 Cube；否 → Docker 够用。

> 档位（`tier`）决定"给不给桌面"：`container` 有 GUI（浏览器、截图、VNC），
> `light` 只有命令行。后端（`backend`）决定"谁来跑"。

---

<a id="config"></a>
## 1. 配置项总表

`pointer-server` 的配置在 `server.toml`，也可以用同名环境变量覆盖。

```toml
[environment]
provider = "container"          # local | light | container
backend  = "docker"             # host | envd | docker（全局默认，**不作用于 local**）
template = "pointer-sandbox-container:latest"
# sandbox_url        = "http://10.0.0.5:3000"   # envd 必填
# volume_host_path   = "/data/pointer/volumes"  # envd 可选
cpus      = 2                   # 0 = 不限制
memory_mb = 4096
disk_mb   = 10240
desktop   = false               # 可选覆盖；默认由档位推导（container 带桌面）
accounts  = { alice = "light" } # 按用户覆盖档位

# 按档位指定后端（可选）：优先级高于上面的全局 backend
[environment.backends]
local     = "host"
light     = "docker"
container = "docker"
```

| 配置 | 环境变量 | 说明 |
|---|---|---|
| `provider` | `POINTER_SERVER_ENVIRONMENT_PROVIDER` | 档位：`local` / `light` / `container` |
| `backend` | `POINTER_SERVER_ENVIRONMENT_BACKEND` | 全局默认后端：`host` / `envd` / `docker`；**不作用于 `local` 档位** |
| `backends` | `POINTER_SERVER_ENVIRONMENT_BACKENDS` | 按档位覆盖后端：`[environment.backends] light = "docker"`，等价 env `local=host,light=docker` |
| `template` | `POINTER_SERVER_ENVIRONMENT_TEMPLATE` | 镜像名（docker）或模板 ID（envd） |
| `sandbox_url` | `POINTER_SERVER_ENVIRONMENT_SANDBOX_URL` | envd 控制面地址，`envd` 后端必填 |
| `volume_host_path` | `POINTER_SERVER_ENVIRONMENT_VOLUME_HOST_PATH` | envd 卷根目录；不填走宿主路径直挂 |
| `desktop` | `POINTER_SERVER_ENVIRONMENT_DESKTOP` | 可选覆盖：默认由档位推导（`container` 带桌面，`light` 不带），配了则以配置为准，与档位矛盾时打警告 |
| `cpus` | `POINTER_SERVER_ENVIRONMENT_CPUS` | 每沙箱 CPU 核数 |
| `memory_mb` | `POINTER_SERVER_ENVIRONMENT_MEMORY_MB` | 每沙箱内存 |
| `disk_mb` | `POINTER_SERVER_ENVIRONMENT_DISK_MB` | 每沙箱磁盘 |
| `accounts` | `POINTER_SERVER_ENVIRONMENT_ACCOUNTS` | 按用户覆盖档位，格式 `user=tier,user2=tier` |
| `admin_tier` | `POINTER_SERVER_ENVIRONMENT_ADMIN_TIER` | 命中 `[auth] admins` 名单时给什么档位：`local`（缺省，宿主直跑）/ `inherit`（跟随部署默认档位）。`accounts` 覆盖始终最高优先 |

**档位与后端必须自洽**，否则启动直接报错：

| 档位 | 允许的后端 |
|---|---|
| `local` | `host` |
| `light` | `envd` / `docker` |
| `container` | `envd` / `docker` |

### 1.1 后端是怎么选出来的

同一个档位的后端按下面的优先级决定（从高到低）：

1. `[environment.backends].<tier>` 显式值
2. 全局 `backend`——**但不作用于 `local` 档位**
3. 档位推导默认：`local` → `host` / `light` → `envd` / `container` → `docker`

第 2 条那个例外很关键：管理员账号被钉在 `local` 档位（见 `admin_tier`），
所以「全局 `backend = "docker"` + 管理员」现在解析成 `local + host`，
不会再去撞「`local` 只能用 `host`」。要让 `light` / `container` 用别的后端，
要么改全局 `backend`，要么用 `[environment.backends]` 单独指定。

### 1.2 校验时机

**启动时**就把实际会用到的档位（部署默认、`[environment.accounts]` 覆盖、
`[environment.backends]` 显式列出的档位）各解析一次并校验，配错直接退出进程，
不会留到用户发起对话时才变成「助手消息异常」。

`[environment]` 段写了不存在的键（拼错的 `provder` / `bakend`）**启动即报错**
并指出是哪个键——以前是静默忽略，看起来生效其实没生效。

> **桌面能力由档位决定，`desktop` 只是覆盖开关。** `container` 档位默认带桌面，
> `light` 默认不带，两个后端（`docker` / `envd`）都遵守同一条规则。
> 只有在确实需要「非 `container` 档位也带桌面」这类例外时才配 `desktop`，
> 与档位矛盾时会打警告。

---

<a id="docker"></a>
## 2. Docker 后端

### 2.1 宿主需要什么

| 项 | 要求 |
|---|---|
| Docker | Engine 或 Docker Desktop，**CLI 在 PATH 上**（Pointer 走 `docker` 命令，不用 SDK） |
| 权限 | 服务进程用户在 `docker` 组，或能访问 `/var/run/docker.sock` |
| 磁盘 | 镜像 1.5 GB + 每容器可写层 + 挂载目录 |
| 内核 | Linux 宿主最佳；macOS 用 Docker Desktop / OrbStack |

> **镜像 ≠ 运行时**。有镜像还必须装 Docker——镜像是文件系统快照，跑起来要守护进程。

### 2.2 镜像从哪来

三种方式，按环境选：

**A. 本机构建**（开发 / 单机）

分层镜像（推荐）：

```bash
# 一次构建 base → light，版本进 tag（默认 YYYY.MM）
./docker/build.sh
POINTER_SANDBOX_VERSION=2026.12 ./docker/build.sh all
```

| 镜像 | 体积 | 内容 |
|---|---|---|
| `pointer-sandbox-base:<ver>` | 202 MB | OS + 工具调用依赖 + 源码 git（无 perl） |
| `pointer-sandbox-light:<ver>` | 358 MB | base + python3.12 / venv / pip + 中文字体 |

light 层**不装任何第三方 pip 包**：技能依赖由各自的 venv 承担（服务器上
`setup.sh` 建好，随技能目录挂进沙箱）。所以同一个镜像能跑任意 python 技能。

单体重构：

```bash
./docker/build.sh base     # 只重建基础层
./docker/build.sh light    # 只重建执行环境层
```

容器档位（带桌面）目前仍用旧镜像：

```bash
./docker/build.sh desktop    # 产出 pointer-sandbox-desktop:<ver>，同时打 pointer-sandbox-container:latest 别名
```

构建前确认宿主能连上 apt 源（Dockerfile 里默认用 `mirrors.aliyun.com`）。

> **服务端包自带构建上下文**：`pointer-server-*.zip` 解压后目录里有 `docker/`
> （两个 Dockerfile + 浏览器入口 + 桌面菜单 + entrypoint，约 20 KB），
> 所以目标机只要有 Docker、能联网拉基础镜像，就能直接：
>
> ```bash
> cd pointer-server
> ./docker/build.sh desktop    # 产出 pointer-sandbox-desktop:<ver>，同时打 pointer-sandbox-container:latest 别名
> ```
>
> deb 包同位置：`/usr/share/pointer-server/docker/`。
> 不需要再拿仓库源码，但**仍然需要网络**（拉 `python:3.12-slim` / `node:24` 和 apt 包）。

**B. 私有仓库拉取**（生产推荐）

```bash
docker pull registry.example.com/pointer/sandbox-container:0.1.3
docker tag  registry.example.com/pointer/sandbox-container:0.1.3 pointer-sandbox-container:latest
```

**C. 离线 tar 导入**（内网无仓库）

```bash
# 有网的机器上
docker save pointer-sandbox-container:latest | gzip > sandbox-container.tar.gz
# 目标机
gunzip -c sandbox-container.tar.gz | docker load
```

> **版本漂移风险**：容器规格指纹只 hash 镜像**名字**，不是 digest。
> 用 `latest` 时镜像更新了，老容器不会重建、继续跑旧镜像。
> 生产建议用不可变 tag（版本号），或换镜像后手动清掉旧容器。

### 2.3 配置

```toml
[environment]
provider = "container"
backend  = "docker"
template = "pointer-sandbox-container:latest"   # light 档位用 pointer-sandbox-light:latest
cpus      = 2
memory_mb = 4096
```

### 2.4 验证

```bash
# 1. 镜像在
docker images | grep pointer-sandbox

# 2. 默认用户不是 root
docker run --rm pointer-sandbox-container:latest id
# 期望：uid=1000(pointer)

# 3. 桌面能起来（container 档位）
docker run -d --name sbx-smoke pointer-sandbox-container:latest sleep infinity
sleep 6
docker exec sbx-smoke sh -lc 'DISPLAY=:99 xdpyinfo | grep dimensions'
# 期望：1920x1080 pixels
docker rm -f sbx-smoke
```

服务端侧再确认一次：开一个会话让 agent 跑 `pwd`，能返回 `/workspace` 即通。

**接管电脑的端口**：容器档位会额外映射两个宿主端口，都只绑 `127.0.0.1`、由 Docker
随机分配，**不需要在防火墙上开任何口**：

| 容器内 | 用途 |
|---|---|
| 5900 | x11vnc（系统屏幕共享直连，`open_sandbox_desktop` 用） |
| 6080 | websockify + noVNC 静态文件（面板内「接管电脑」走这个） |

验证接管通道：

```bash
docker run -d --name sbx-vnc -p 127.0.0.1::6080 -p 127.0.0.1::5900 \
  -e POINTER_VNC_PASSWORD=test1234 pointer-sandbox-container:latest sleep infinity
sleep 8
P=$(docker port sbx-vnc 6080/tcp | sed 's/.*://')
curl -s -o /dev/null -w 'vnc.html %{http_code}\n' "http://127.0.0.1:$P/vnc.html"   # 期望 200
docker exec sbx-vnc sh -lc 'pgrep -af websockify'                                  # 期望有进程
docker rm -f sbx-vnc
```

镜像升级后老容器不会自动带 6080（规格指纹含 `SPEC_VERSION`，改契约已 +1，正常会重建）；
如果手工保留过旧容器，`docker rm -f` 掉让它重建即可。

### 2.5 容量

每个会话一个容器，空闲 30 分钟回收。默认 **2 核 / 4 GB**：

| 并发会话 | 内存 |
|---|---|
| 5 | 20 GB |
| 20 | 80 GB |
| 50 | 200 GB |

并发高时下调 `memory_mb`（2048 够跑命令行；开浏览器留 4096）。

---

<a id="envd"></a>
## 3. envd 后端（Cube / e2b-local）

### 3.1 宿主要求

| 项 | 要求 |
|---|---|
| 架构 | **x86_64**（PVM 内核只出 x86_64；ARM64 需裸金属原生 KVM） |
| 虚拟化 | `/dev/kvm` 可用（普通云主机要装 PVM 宿主内核） |
| 系统 | OpenCloudOS 9 / TencentOS 4 推荐；Ubuntu 20.04+ 可用但需手工建 XFS |
| glibc | ≥ 2.31 |
| 磁盘 | `/data/cubelet` 必须 **XFS**，≥ 50 GB，推荐 200 GB |
| 内存 | 最低 8 GB，推荐 64 GB |
| Docker | **仍然需要**（MySQL / Redis / CubeProxy / CoreDNS 都跑容器） |
| 时间同步 | **必须**，否则节点心跳过期被判不健康 |

### 3.2 单机安装

```bash
# 普通云主机先装 PVM 内核（裸金属跳过）
dnf install -y kernel-*.cubesandbox.oc9 ...
curl -sL .../deploy/pvm/grub/host_grub_config.sh | bash && reboot
modprobe kvm_pvm && echo 'kvm_pvm' > /etc/modules-load.d/kvm-pvm.conf

# 一键安装
curl -sL https://cnb.cool/CubeSandbox/CubeSandbox/-/git/raw/master/deploy/one-click/online-install.sh \
  | CUBE_PVM_ENABLE=1 MIRROR=cn bash
```

装完的端口：

| 服务 | 端口 |
|---|---|
| CubeAPI（E2B 兼容 REST） | 3000 |
| CubeMaster | 8089 |
| network-agent | 19090 |
| Web 控制台 | 12088 |

### 3.3 加计算节点（多机）

```bash
tar -xzf cube-sandbox-one-click-<version>.tar.gz && cd cube-sandbox-one-click-<version>
cp env.example .env
# ONE_CLICK_DEPLOY_ROLE=compute
# CUBE_SANDBOX_NODE_IP=<本机IP>
# ONE_CLICK_CONTROL_PLANE_IP=<控制节点IP>
# CUBE_S3_* ← 从控制节点 /usr/local/services/cubetoolbox/.one-click.env 拷
sudo ./install-compute.sh
```

**三个必踩的坑**：

1. **时间不同步** → 心跳超出 15 秒窗口 → 节点被判不健康，模板不下发
2. **`CUBE_S3_*` 缺失** → 只告警不失败，但 S3 卷插件不可用（等你要持久化才发现）
3. **不配调度评分** → 新沙箱全堆第一台节点

```yaml
scheduler:
  priority_select_num: 3
  score:
    enable_scorers: [real_time_weighted_average]
    resource_weights: { mvm_num: 2, local_create_num: 3, quota_cpu_usage: 1, quota_mem_usage: 1 }
```

### 3.4 制作模板

模板镜像里**必须跑着 envd**（端口 49999 是 exec、49983 是 files）。直接拿
`pointer-sandbox-light` 做模板不行——它没装 envd。

```bash
cubemastercli tpl create-from-image \
  --image <带 envd 的镜像> \
  --writable-layer-size 1G \
  --expose-port 49999 --expose-port 49983 \
  --probe 49999
```

等状态变 `READY`，记下 `template_id`（`tpl-xxxxxxxx`）。

### 3.5 配置

```toml
[environment]
provider   = "light"                       # envd 无桌面
backend    = "envd"
sandbox_url = "http://<控制节点IP>:3000"
template    = "tpl-xxxxxxxx"
# volume_host_path = "/data/pointer/volumes"   # 需要卷挂载时填
```

### 3.6 验证

```bash
curl -fsS http://<控制节点>:3000/health          # CubeAPI
curl -fsS http://<控制节点>:8089/notify/health   # CubeMaster
curl -fsS http://<控制节点>:19090/healthz        # network-agent
```

服务端侧：开会话让 agent 跑 `id`，期望 **uid ≥ 100000**（不是宿主用户）。

---

<a id="unify"></a>
## 4. 单机与多机的统一

单机只是"集群里只有 1 个计算节点"。**代码一份，差异只在基础设施**：

| 项 | 单机 | 多机 |
|---|---|---|
| 代码 | 同一套 | 同一套 |
| `scheduler.score` | 无所谓 | 必须配 |
| 时间同步 | 可忽略 | 必须 |
| `CUBE_S3_*` | 用内置 MinIO | 每台计算节点都要配 |
| CubeProxy / DNS | 单点即可 | 泛域名 + 负载 |

**持久化统一走 Volume API**（`POST /volumes` 拿 volume_id，再挂到沙箱）。
`metadata["host-mount"]` 那条路只在单机可用——它固定源节点，多机调度必失败。

---

<a id="state"></a>
## 5. 用户状态保存

| 数据 | 放哪 | 跨节点 |
|---|---|---|
| 工作区 `/workspace` | 宿主目录（单机）/ 共享卷（多机） | 看存储 |
| 用户 home `/home/pointer`（浏览器登录态、pip/npm 缓存） | Volume 挂载 | ✅ 后端是 S3 / JuiceFS / NFS 时 |
| 技能 `/skills` | 每台节点本地同一份，只读挂载 | 部署时分发 |

**浏览器登录态**靠 home 卷：Chromium profile 在 `$HOME/.config/chromium`，
容器/VM 重建后仍在。

三个注意点：

1. **raw host mount 跨不了节点**——多机必须用 S3 / JuiceFS / NFS
2. **调度器不检查卷可达性**——每台计算节点都要配同一个 driver，否则沙箱创建失败
3. **同一卷一个沙箱只能挂一次**；多沙箱共享同一卷会互相看到写入，
   浏览器 profile 是单写者结构，同用户多沙箱并发会锁冲突

---

<a id="trouble"></a>
## 6. 故障排查

| 现象 | 原因 | 处理 |
|---|---|---|
| `No such file or directory (os error 2)` | 没装 Docker / CLI 不在 PATH | 装 Docker，确认 `docker ps` 能跑 |
| `permission denied ... docker.sock` | 用户不在 docker 组 | `usermod -aG docker <user>` 后重登 |
| 容器起来了但工具报"执行环境未绑定" | 档位与后端不自洽 | 检查 `provider` / `backend` 组合 |
| 改了镜像但行为没变 | 指纹只认镜像名 | 删旧容器，或用不可变 tag |
| 容器里 Xvfb 起不来 | 非 root 下 `/tmp/.X11-unix` 缺失 | 镜像里预建并 `chmod 1777` |
| Cube 节点不在集群里 | 时间不同步 / 网络不通 | 查 `/data/log/Cubelet/`，确认 3010 可达 |
| 沙箱创建失败但节点健康 | 卷挂不到该节点 | 检查 `volume_plugins` driver 是否全节点一致 |
| 浏览器打不开（"profile in use"） | 上一个容器的 `SingletonLock` 残留 | entrypoint 已处理；确认镜像版本 |

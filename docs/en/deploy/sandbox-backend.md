# Sandbox Runtime Deployment (docker / envd)

English | [简体中文](../../zh-CN/deploy/sandbox-backend.md)

For the server to actually execute commands, read/write files, and drive a browser,
the agent needs an **execution environment**. Pointer supports two backends.
Picking wrong means rework after launch, so start with section 0.

---

<a id="choose"></a>
## 0. Pick a backend first

| Scenario | Tier | Backend | Why |
|---|---|---|---|
| Desktop app (user's own machine) | `local` | `host` | Runs on the host, no isolation |
| Single-host web, trusted colleagues | `container` / `light` | `docker` | The isolation boundary is that one machine |
| **Multi-user web (mutually untrusted)** | `container` / `light` | `envd` + Cube | Per-user kernel; a shared kernel is not enough |
| Existing K8s cluster | `container` | `docker` (or bring Kata/gVisor) | Don't rebuild the wheel |

Rule of thumb: **is someone else's code running on the same machine?**
Yes → Cube. No → Docker is enough.

> Tier decides whether there is a GUI: `container` has a desktop (browser, screenshots, VNC),
> `light` is command-line only. Backend decides who runs it.

---

<a id="config"></a>
## 1. Configuration reference

Configuration lives in `server.toml`; every key has an environment-variable override.

> **Recommended default: `provider = "local"`.** A sandbox that is not fully set up
> (image not built, backend unreachable) fails at startup or on the first message,
> so **stay on `local` and switch only when the sandbox is actually needed**.
>
> ⚠️ **Platform-mode trap**: with `provider` unset the default tier is derived from
> `[deployment] mode` — standalone derives `local`, but **platform derives `light`**,
> and `light` defaults to the `envd` backend, which requires `sandbox_url`. So a
> platform deployment with no `[environment]` section fails on the first message.
> Set `provider = "local"` explicitly to avoid it.

```toml
[environment]
provider = "local"              # local (recommended default) | light | container; switch once the sandbox is ready
backend  = "docker"             # host | envd | docker (global default, **not applied to local**)
template = "pointer-sandbox-container:latest"
# sandbox_url        = "http://10.0.0.5:3000"   # required for envd
# volume_host_path   = "/data/pointer/volumes"  # optional for envd
cpus      = 2                   # 0 = unlimited
memory_mb = 4096
disk_mb   = 10240
desktop   = false               # optional override; defaults to the tier (container has one)
accounts  = { alice = "light" } # per-user tier override

# Per-tier backends (optional): wins over the global `backend` above
[environment.backends]
local     = "host"
light     = "docker"
container = "docker"
```

| Key | Environment variable | Notes |
|---|---|---|
| `provider` | `POINTER_SERVER_ENVIRONMENT_PROVIDER` | Tier: `local` / `light` / `container` |
| `backend` | `POINTER_SERVER_ENVIRONMENT_BACKEND` | Global default backend: `host` / `envd` / `docker`; **not applied to the `local` tier** |
| `backends` | `POINTER_SERVER_ENVIRONMENT_BACKENDS` | Per-tier backend: `[environment.backends] light = "docker"`, env form `local=host,light=docker` |
| `template` | `POINTER_SERVER_ENVIRONMENT_TEMPLATE` | Image name (docker) or template ID (envd) |
| `sandbox_url` | `POINTER_SERVER_ENVIRONMENT_SANDBOX_URL` | envd control-plane URL; required for `envd` |
| `volume_host_path` | `POINTER_SERVER_ENVIRONMENT_VOLUME_HOST_PATH` | envd volume root; empty means raw host mounts |
| `cpus` | `POINTER_SERVER_ENVIRONMENT_CPUS` | vCPUs per sandbox |
| `memory_mb` | `POINTER_SERVER_ENVIRONMENT_MEMORY_MB` | Memory per sandbox |
| `disk_mb` | `POINTER_SERVER_ENVIRONMENT_DISK_MB` | Disk per sandbox |
| `accounts` | `POINTER_SERVER_ENVIRONMENT_ACCOUNTS` | Per-user tier, e.g. `user=tier,user2=tier` |
| `admin_tier` | `POINTER_SERVER_ENVIRONMENT_ADMIN_TIER` | Tier for accounts listed in `[auth] admins`: `local` (default, run on host) / `inherit` (follow the deployment default tier). `accounts` overrides always win |

**Tier and backend must agree** — a mismatch fails at startup:

| Tier | Allowed backends |
|---|---|
| `local` | `host` |
| `light` | `envd` / `docker` |
| `container` | `envd` / `docker` |

### 1.1 How the backend is chosen

For a given tier, the backend comes from the first match:

1. `[environment.backends].<tier>` (explicit)
2. the global `backend` — **but never for the `local` tier**
3. the tier default: `local` → `host` / `light` → `envd` / `container` → `docker`

That second exception matters: listed admins are pinned to the `local` tier
(see `admin_tier`), so "global `backend = "docker"` + admin" now resolves to
`local + host` instead of tripping the "`local` only allows `host`" check.
To give `light` / `container` a different backend, change the global `backend`
or pin it per tier with `[environment.backends]`.

### 1.2 When it is validated

**At startup**: every tier that can actually be resolved (deployment default,
`[environment.accounts]` overrides, and any tier listed in
`[environment.backends]`) is resolved and validated once. A bad combination
exits the process immediately instead of surfacing later as a broken assistant
message.

Unknown keys under `[environment]` (a typo like `provder` / `bakend`) now
**fail at startup** and name the offending key. They used to be ignored
silently — which looked like it worked.

> **Desktop capability follows the tier; `desktop` is only an override.** The
> `container` tier ships a desktop, `light` does not, and both backends
> (`docker` / `envd`) follow the same rule. Configure `desktop` only for the
> rare exception (e.g. a desktop on a non-`container` tier); a value that
> contradicts the tier logs a warning.

---

<a id="docker"></a>
## 2. Docker backend

### 2.1 Host requirements

| Item | Requirement |
|---|---|
| Docker | Engine or Docker Desktop; **the `docker` CLI must be on PATH** (Pointer shells out, no SDK) |
| Permission | Service user in the `docker` group, or able to access `/var/run/docker.sock` |
| Disk | 1.5 GB image + per-container writable layer + mounted dirs |
| Kernel | Linux host preferred; macOS via Docker Desktop / OrbStack |

> **An image is not a runtime.** Having the image still requires Docker — the image is a
> filesystem snapshot, running it needs a daemon.

### 2.2 Where the image comes from

**A. Build locally** (dev / single host)

```bash
./docker/build.sh desktop    # 产出 pointer-sandbox-desktop:<ver>，同时打 pointer-sandbox-container:latest 别名
./docker/build.sh            # 全部：base → light → desktop
```

> **The server package ships the build context**: the unzipped `pointer-server-*.zip`
> contains a `docker/` directory (two Dockerfiles, browser entry, desktop menu, entrypoint;
> about 20 KB), so a target host with Docker and network access can build directly:
>
> ```bash
> cd pointer-server
> ./docker/build.sh desktop    # 产出 pointer-sandbox-desktop:<ver>，同时打 pointer-sandbox-container:latest 别名
> ```
>
> The deb package puts it at `/usr/share/pointer-server/docker/`.
> No repo checkout needed — but **network is still required** (base images and apt packages).

**B. Pull from a private registry** (recommended for production)

```bash
docker pull registry.example.com/pointer/sandbox-container:0.1.3
docker tag  registry.example.com/pointer/sandbox-container:0.1.3 pointer-sandbox-container:latest
```

**C. Offline tarball** (air-gapped intranet)

```bash
# on a networked machine
docker save pointer-sandbox-container:latest | gzip > sandbox-container.tar.gz
# on the target host
gunzip -c sandbox-container.tar.gz | docker load
```

> **Version drift risk**: the container spec fingerprint hashes the image **name**, not its
> digest. With `latest`, an updated image will not recreate existing containers — they keep
> running the old one. Pin an immutable tag in production, or remove old containers by hand.

### 2.3 Configure

```toml
[environment]
provider = "container"
backend  = "docker"
template = "pointer-sandbox-container:latest"   # use pointer-sandbox-light:latest for tier=light
cpus      = 2
memory_mb = 4096
```

### 2.4 Verify

```bash
docker images | grep pointer-sandbox

docker run --rm pointer-sandbox-container:latest id
# expect: uid=1000(pointer)

docker run -d --name sbx-smoke pointer-sandbox-container:latest sleep infinity
sleep 6
docker exec sbx-smoke sh -lc 'DISPLAY=:99 xdpyinfo | grep dimensions'
# expect: 1920x1080 pixels
docker rm -f sbx-smoke
```

Then open a session and have the agent run `pwd`; `/workspace` means it works.

**Ports used by desktop takeover**: the container tier publishes two extra host ports,
both bound to `127.0.0.1` with a random host port — **no firewall opening needed**:

| In container | Purpose |
|---|---|
| 5900 | x11vnc (system screen sharing, used by `open_sandbox_desktop`) |
| 6080 | websockify + noVNC static files (the in-panel "Take over" tab) |

Verifying the takeover channel:

```bash
docker run -d --name sbx-vnc -p 127.0.0.1::6080 -p 127.0.0.1::5900 \
  -e POINTER_VNC_PASSWORD=test1234 pointer-sandbox-container:latest sleep infinity
sleep 8
P=$(docker port sbx-vnc 6080/tcp | sed 's/.*://')
curl -s -o /dev/null -w 'vnc.html %{http_code}\n' "http://127.0.0.1:$P/vnc.html"   # expect 200
docker exec sbx-vnc sh -lc 'pgrep -af websockify'                                  # expect a process
docker rm -f sbx-vnc
```

Containers created before the image upgrade will not have 6080 (the spec fingerprint
includes `SPEC_VERSION`, bumped for this contract change, so they are recreated
automatically). If you kept an old container by hand, `docker rm -f` it.

### 2.5 Capacity

One container per conversation, idle containers are reclaimed after 30 minutes.
Default is **2 vCPU / 4 GB**:

| Concurrent sessions | Memory |
|---|---|
| 5 | 20 GB |
| 20 | 80 GB |
| 50 | 200 GB |

Lower `memory_mb` under load (2048 is fine for CLI work; keep 4096 when a browser runs).

---

<a id="envd"></a>
## 3. envd backend (Cube / e2b-local)

### 3.1 Host requirements

| Item | Requirement |
|---|---|
| Arch | **x86_64** (the PVM kernel ships x86_64 only; ARM64 needs bare metal with native KVM) |
| Virtualization | `/dev/kvm` available (plain cloud VMs need the PVM host kernel) |
| OS | OpenCloudOS 9 / TencentOS 4 recommended; Ubuntu 20.04+ works but needs manual XFS |
| glibc | ≥ 2.31 |
| Disk | `/data/cubelet` must be **XFS**, ≥ 50 GB, 200 GB recommended |
| Memory | 8 GB minimum, 64 GB recommended |
| Docker | **Still required** (MySQL / Redis / CubeProxy / CoreDNS run in containers) |
| Time sync | **Required** — skewed clocks push node heartbeats past the 15 s window |

### 3.2 Single-node install

```bash
# plain cloud VM: install the PVM kernel first (skip on bare metal)
dnf install -y kernel-*.cubesandbox.oc9 ...
curl -sL .../deploy/pvm/grub/host_grub_config.sh | bash && reboot
modprobe kvm_pvm && echo 'kvm_pvm' > /etc/modules-load.d/kvm-pvm.conf

curl -sL https://cnb.cool/CubeSandbox/CubeSandbox/-/git/raw/master/deploy/one-click/online-install.sh \
  | CUBE_PVM_ENABLE=1 MIRROR=cn bash
```

Ports after install:

| Service | Port |
|---|---|
| CubeAPI (E2B-compatible REST) | 3000 |
| CubeMaster | 8089 |
| network-agent | 19090 |
| Web console | 12088 |

### 3.3 Add compute nodes (multi-node)

```bash
tar -xzf cube-sandbox-one-click-<version>.tar.gz && cd cube-sandbox-one-click-<version>
cp env.example .env
# ONE_CLICK_DEPLOY_ROLE=compute
# CUBE_SANDBOX_NODE_IP=<this node IP>
# ONE_CLICK_CONTROL_PLANE_IP=<control node IP>
# CUBE_S3_* <- copy from /usr/local/services/cubetoolbox/.one-click.env on the control node
sudo ./install-compute.sh
```

**Three traps:**

1. **Clock skew** → heartbeat expires → node marked unhealthy, templates are not delivered
2. **Missing `CUBE_S3_*`** → warning only, install continues, but the S3 volume plugin is unusable
3. **No scheduler scoring** → every new sandbox lands on the first node

```yaml
scheduler:
  priority_select_num: 3
  score:
    enable_scorers: [real_time_weighted_average]
    resource_weights: { mvm_num: 2, local_create_num: 3, quota_cpu_usage: 1, quota_mem_usage: 1 }
```

### 3.4 Build a template

The template image **must run envd** (49999 for exec, 49983 for files).
`pointer-sandbox-light` will not work as-is — it does not ship envd.

```bash
cubemastercli tpl create-from-image \
  --image <image with envd> \
  --writable-layer-size 1G \
  --expose-port 49999 --expose-port 49983 \
  --probe 49999
```

Wait for `READY` and record the `template_id` (`tpl-xxxxxxxx`).

### 3.5 Configure

```toml
[environment]
provider   = "light"                       # envd has no desktop
backend    = "envd"
sandbox_url = "http://<control-node-ip>:3000"
template    = "tpl-xxxxxxxx"
# volume_host_path = "/data/pointer/volumes"   # only when volume mounts are needed
```

### 3.6 Verify

```bash
curl -fsS http://<control-node>:3000/health          # CubeAPI
curl -fsS http://<control-node>:8089/notify/health   # CubeMaster
curl -fsS http://<control-node>:19090/healthz        # network-agent
```

Server side: have the agent run `id`; expect **uid ≥ 100000** (not a host user).

---

<a id="unify"></a>
## 4. Single node vs. cluster

A single node is just a cluster with one compute node. **One code path; only infrastructure differs.**

| Item | Single node | Multi-node |
|---|---|---|
| Code | same | same |
| `scheduler.score` | irrelevant | required |
| Time sync | can ignore | required |
| `CUBE_S3_*` | built-in MinIO | every compute node |
| CubeProxy / DNS | single point is fine | wildcard DNS + load balancing |

**Persistence goes through the Volume API** (`POST /volumes` → volume_id → mount).
The `metadata["host-mount"]` path only works on a single node — it pins the source node,
so multi-node scheduling fails.

---

<a id="state"></a>
## 5. User state

| Data | Location | Cross-node |
|---|---|---|
| Workspace `/workspace` | host dir (single) / shared volume (cluster) | depends on storage |
| Home `/home/pointer` (browser logins, pip/npm caches) | Volume mount | yes, when backed by S3 / JuiceFS / NFS |
| Skills `/skills` | identical read-only copy on every node | distributed at deploy time |

**Browser login state** lives in the home volume: the Chromium profile sits at
`$HOME/.config/chromium`, so it survives container/VM recreation.

Three caveats:

1. **A raw host mount cannot cross nodes** — multi-node needs S3 / JuiceFS / NFS
2. **The scheduler does not check volume reachability** — every compute node needs the same driver
3. **A volume can be mounted only once per sandbox**; sharing across sandboxes exposes each
   other's writes, and a browser profile is single-writer, so concurrent sandboxes for one
   user will fight over the lock

---

<a id="trouble"></a>
## 6. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `No such file or directory (os error 2)` | Docker not installed / CLI not on PATH | Install Docker, confirm `docker ps` |
| `permission denied ... docker.sock` | User not in the docker group | `usermod -aG docker <user>`, re-login |
| Tools report "execution environment not bound" | Tier/backend mismatch | Check the `provider` / `backend` pair |
| Rebuilt image, behavior unchanged | Fingerprint only tracks the image name | Remove old containers, or pin immutable tags |
| Xvfb fails inside the container | `/tmp/.X11-unix` missing as non-root | Pre-create it and `chmod 1777` in the image |
| Cube node never joins | Clock skew / network | Check `/data/log/Cubelet/`, confirm port 3010 |
| Sandbox creation fails on a healthy node | Volume cannot attach there | Ensure `volume_plugins` drivers match on all nodes |
| Browser won't start ("profile in use") | Stale `SingletonLock` from a previous container | Handled by the entrypoint; confirm the image version |

# envd / Cube 后端部署

envd 后端让沙箱跑在远端 microVM 里，宿主机不需要共享内核。
**但宿主仍然要装 Docker**——MySQL / Redis / CubeProxy / CoreDNS 都是容器。

## 宿主前提

| 项 | 要求 | 怎么查 |
|---|---|---|
| 架构 | **x86_64**（PVM 内核只出 x86_64） | `uname -m` |
| 虚拟化 | `/dev/kvm` 存在 | `ls -l /dev/kvm` |
| 系统 | OpenCloudOS 9 / TencentOS 4 推荐；Ubuntu 20.04+ 需手工建 XFS | `cat /etc/os-release` |
| glibc | ≥ 2.31 | `ldd --version \| head -1` |
| 磁盘 | `/data/cubelet` **必须 XFS**，≥ 50 GB，推荐 200 GB | `df -hT /data` |
| 内存 | 最低 8 GB，推荐 64 GB | `free -g` |
| 时间同步 | 必须启用 | `timedatectl` |

ARM64 只能用裸金属原生 KVM，普通云主机不行（PVM 不支持 ARM）。

## 单机安装

### 1. 普通云主机先装 PVM 内核（裸金属跳过）

```bash
dnf install -y kernel-*.cubesandbox.oc9 kernel-core-* kernel-modules-*
curl -sL .../deploy/pvm/grub/host_grub_config.sh | bash
reboot

# 重启后确认
lsmod | grep kvm_pvm
ls -l /dev/kvm
```

模块没自动加载时：

```bash
modprobe kvm_pvm
echo 'kvm_pvm' > /etc/modules-load.d/kvm-pvm.conf
```

### 2. 一键安装

```bash
curl -sL https://cnb.cool/CubeSandbox/CubeSandbox/-/git/raw/master/deploy/one-click/online-install.sh \
  | CUBE_PVM_ENABLE=1 MIRROR=cn bash
```

| 服务 | 端口 |
|---|---|
| CubeAPI（E2B 兼容 REST） | 3000 |
| CubeMaster | 8089 |
| network-agent | 19090 |
| Web 控制台 | 12088 |

安装目录 `/usr/local/services/cubetoolbox`；CLI 软链到 `/usr/local/bin`
（`cubemastercli`、`cubecli`、`cube-runtime`、`containerd-shim-cube-rs`）。

### 3. 验证

```bash
sudo ./smoke.sh
curl -fsS http://127.0.0.1:3000/health
curl -fsS http://127.0.0.1:8089/notify/health
curl -fsS http://127.0.0.1:19090/healthz
```

## 加计算节点（多机）

控制节点自带一个 Cubelet；扩容在新机器上：

```bash
tar -xzf cube-sandbox-one-click-<version>.tar.gz && cd cube-sandbox-one-click-<version>
cp env.example .env
```

`.env` 必改：

```bash
ONE_CLICK_DEPLOY_ROLE=compute
CUBE_SANDBOX_NODE_IP=<本机IP>
ONE_CLICK_CONTROL_PLANE_IP=<控制节点IP>
# S3 卷插件需要（从控制节点拷）
CUBE_S3_ENDPOINT=http://<控制节点IP>:9000
CUBE_S3_ACCESS_KEY_ID=...
CUBE_S3_SECRET_ACCESS_KEY=...
CUBE_S3_BUCKET=cube-volumes
CUBE_S3_S3FS_EXTRA_OPTS=-ouse_path_request_style
```

```bash
sudo ./install-compute.sh
curl http://<控制节点IP>:3010/internal/v1/nodes   # 确认注册
```

### 三个必踩的坑

1. **时间不同步** → 心跳超出 15 秒窗口 → 节点被判不健康，**模板不下发**
2. **`CUBE_S3_*` 缺失** → 只告警不失败，节点起来了但 S3 卷插件不可用
3. **不配调度评分** → 新沙箱全堆第一台节点

```yaml
# 控制节点 cubemaster.yaml
scheduler:
  priority_select_num: 3
  score:
    enable_scorers: [real_time_weighted_average]
    resource_weights: { mvm_num: 2, local_create_num: 3, quota_cpu_usage: 1, quota_mem_usage: 1 }
```

## 做模板

**模板镜像里必须跑着 envd**（49999 = exec，49983 = files）。
`pointer-sandbox-headless` 不含 envd，不能直接当模板。

```bash
cubemastercli tpl create-from-image \
  --image <带 envd 的镜像> \
  --writable-layer-size 1G \
  --expose-port 49999 --expose-port 49983 \
  --probe 49999
```

等状态变 `READY`，记下 `template_id`（`tpl-xxxxxxxx`）。端口对不上，模板起来也是死的。

## 配置

```toml
[environment]
provider    = "headless"                    # envd 无桌面
backend     = "envd"
sandbox_url = "http://<控制节点IP>:3000"
template    = "tpl-xxxxxxxx"
# volume_host_path = "/data/pointer/volumes"
```

环境变量：`POINTER_SERVER_ENVIRONMENT_SANDBOX_URL` / `_TEMPLATE` / `_VOLUME_HOST_PATH`。

## 用户状态与持久化

| 数据 | 放哪 | 跨节点 |
|---|---|---|
| 工作区 `/workspace` | 宿主目录（单机）/ 共享卷（多机） | 看存储 |
| home `/home/pointer`（浏览器登录态、pip/npm 缓存） | Volume 挂载 | S3 / JuiceFS / NFS 才跨 |
| 技能 `/skills` | 每节点本地同一份，只读挂载 | 部署时分发 |

浏览器登录态在 `$HOME/.config/chromium`，容器/VM 重建后靠 home 卷保留。

三条约束：

1. **raw host mount 跨不了节点** —— 多机必须用 S3 / JuiceFS / NFS
2. **调度器不检查卷可达性** —— 每台计算节点都要配同一个 driver，否则创建失败
3. **同一卷一个沙箱只能挂一次**；多沙箱共享会互相看到写入，
   浏览器 profile 是单写者，同用户多沙箱并发会锁冲突

单机与多机是同一套代码：`POST /volumes` 拿 volume_id 再挂载。
`metadata["host-mount"]` 那条路只在单机可用。

## 本地验证（macOS）

Mac 跑不了 Cube（需要 KVM）。验证 envd 协议用 e2b-local：

```bash
scripts/dev-sandbox.sh up      # 起网关
scripts/dev-sandbox.sh smoke   # 冒烟
scripts/dev-sandbox.sh status
```

**必须用 OrbStack**：Docker Desktop 的 `--network host` 是 VM netns，不转发到 macOS。

## 常见问题

| 现象 | 原因 | 处理 |
|---|---|---|
| `/dev/kvm` 不存在 | 云主机屏蔽了虚拟化扩展 | 装 PVM 内核 + `modprobe kvm_pvm` |
| 装了内核仍无 kvm | 模块没自动加载 | 写 `modules-load.d/kvm-pvm.conf` |
| 节点不在集群 | 时间不同步 / 3010 不通 | 查 `/data/log/Cubelet/` |
| 模板一直不 READY | 端口 / probe 配错 | 确认 49999 暴露且能探活 |
| 沙箱创建失败但节点健康 | 卷挂不到该节点 | 各节点 `volume_plugins` driver 一致 |
| 首次 create 报 no kernel component | 模板池未就绪 | 等预热后重试 |

日志：`/data/log/Cubelet/`、`/data/log/CubeShim/`、`/data/log/CubeVmm/`、
`/var/log/cube-sandbox-one-click/`。

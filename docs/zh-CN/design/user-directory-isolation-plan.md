# 用户目录隔离实现计划

> 状态：细化版  
> 方案：[user-directory-isolation.md](user-directory-isolation.md)  
> 范围：Client 与 Web 共用的 `pointer-core`。不搬旧文件，不改设置页外观，不重新打开对象存储上传。  
> 完成标准：会话读写和进程都越不出该用户的 home 与 data。路径由当前环境的 `home_root()` / `data_root()` 拼接。系统库只存「根 + 相对段」。

物理布局：`{系统目录}/users/{user_key}/home` 与 `data`。`user_key` 沿用账号哈希。未登录为 `anon/{会话}`，一会话一棵。在线环境不解析旧的 `{user_key}-home`、`{user_key}-data`。

观察性：绑定、建目录、拷贝打 info。越界、缺环境、用户不匹配、布局或拷贝失败返回错误。不影响后续结果的跳过（没有会话 id 的调试图）打 warn。

## P0 两个根和相对段

环境对外只有两个根。本地路径表和容器挂载用同一份 `MountPlan`。

改动：

- `env/mounts.rs`：`MountPlan::for_session` 只生成 home、data。去掉 `/skills`、`/plugins`。`DATA_LAYOUT_DIRS` 补上 `memories`、`turn-baselines`。`ensure_data_layout` 创建失败返回错误，不再警告后继续。
- `env/local.rs`：用户会话的 `LocalEnv` 使用这份路径表。`home_root()` 指向该用户的 `home/`，`data_root()` 指向 `data/`，不再使用 trait 默认的空 `data_root`。`ReadScope` 不再是整机 `Host`。恒等映射只留给系统维护入口，聊天绑定到不了它。
- `env/docker.rs`、`env/remote.rs`：`home_root()` / `data_root()` 由各自实现返回。容器里是 `/home/pointer` 和 `/data`。remote 返回远端的两个根。
- `env/session_env.rs`：`docker_spec_for`、`skill_roots_for_mounts` 不再把技能根和插件根加进挂载。活动工作目录用 `data_root` 拼接 `workspaces/...`，不再把镜像的 `/workspace` 当作 cwd。
- `user_data_paths.rs`：解析 `{系统目录}/users/{user_key}/home|data`。删除 `guest_project_path`、`guest_worktree_path` 里写死的 `/data/...`。对外只提供相对段，调用方用当前环境拼接。旧卷名不作为在线读回退。
- 新增一处相对路径类型（可放在 `user_data_paths.rs`）：`home` 或 `data`，加相对段。禁止 `..`。这是库存和媒体引用的唯一路径形状。

完成标准：

- 本地与 Docker 的挂载列表都只有两项。
- 把别的用户目录、系统目录、安装目录交给这个环境的 `to_host`，返回路径未映射。规范化后的符号链接若离开这两个根，同样未映射。
- 用户会话上 `data_root()` 非空，`ReadScope` 不是整机。
- `local_binding_it`、`headless_binding_it`、`mounts` 单测按新路径表改完并通过。

## P1 第一次进入时创建用户目录并拷贝

`bind_and_resolve_workspace` 在解析档位之后、创建或复用环境之前调用 `ensure_user_layout(session_user_id, conversation_id)`。系统账号和普通账号走同一函数。空用户 id 使用 `anon/{会话}`，禁止再落到共用的匿名段。

`ensure_user_layout`：

- 创建 `home/`、`home/.pointer/`、`skills/{system,org,user}`、`plugins/{system,user}`、`agents/system`、`plugin-data`、`data/` 以及 `DATA_LAYOUT_DIRS`。
- 从安装目录拷贝技能、插件、内置智能体到对应的 `system/`。目标里已有同名目录则跳过该份。
- 目标规范化后必须落在这个 `user_key` 的 `home/` 内。不跟随指向该目录之外的符号链接。
- 拷贝或建目录失败则本次绑定失败。
- info 日志：用户、是否新建、技能 / 插件 / 智能体各拷贝了多少。

运行时发现改为用户目录：

- `skills/external.rs` 的 `skill_roots` 接受用户标识，只返回该用户 `home/.pointer/skills` 的子目录。会话绑定后忽略 `POINTER_HOME`、`POINTER_SKILLS_DIR`。`apply_skill_dir_env` 把 `SKILL_DIR` 设为当前环境里的技能库路径。
- `plugins/mod.rs` 的 `user_plugins_dir` 改为该用户的 `home/.pointer/plugins`。`discover_plugin_dirs` 在已有用户环境时只扫这个目录。进程启动、尚未知道用户时不从操作系统家目录加载插件。
- `agents/mod.rs` 的 `agent_roots` 只返回该用户 `home/.pointer/agents`。不再读进程用户的 `~/.agents` 和系统目录下的 `agents/`。
- `plugins/agents_md.rs`：全局文件改为该用户 `home_root` 下的 `.pointer/AGENTS.md`，经环境读取。向上查找停在两个根。
- `skills/org_sync.rs`、`skills/curator.rs`：组织技能和策展写入该用户的 `skills/org` 或用户拷贝，不再读写 `{系统目录}/org-skills` 作为会话根。
- `chat_service/app_state.rs` 里按进程插件目录导入的调用改为当前用户的插件目录。

完成标准：

- 同一用户连续两次 `ensure_user_layout`，第二次不改已有文件。
- 两个匿名会话得到两棵目录，记忆和设置不共用。
- 挂载列表里没有技能根和插件根。本地与容器用各自的 `home_root` 都能读到同一份拷贝。
- 会话内 `SKILL_DIR` 落在该用户的 home 根下。

## P2 新写入进 home 和 data

设置：

- `storage.rs` 的 `user-settings/{uid}.json` 并入该用户 `home_root` 下的 `.pointer/settings.json`，读写走环境。
- 全局 `user_settings.json` 里属于使用者本人的字段（界面语言、模型选择、该用户的 provider 密钥，以及同类「改了只影响这个人」的项）一并进入这份文件。部署级 `settings.json`、通道、模型目录缓存、`local_platform_settings.json` 留在系统目录。
- 切换完成后，会话不再读取系统目录里的 `user_settings.json` 和 `provider_keys.enc`。
- 系统目录根上的 `.env` 只注入服务进程。用户终端变量来自 `home_root` 下的 `.pointer/.env` 和设置中的终端覆盖。
- Client 与 Web 的设置接口改走这份文件。页面布局不动。

记忆和会话文件：

- `user_storage.rs`、`memory/store.rs`：记忆目录改为 `data_root` 下的 `memories/`。空用户 id 不再使用共用匿名段。
- `media/store.rs`：`sandbox_attachment_rel` 去掉 `SessionSandbox` 回退。数据目录创建失败就返回错误。新文件写入 `data` 下的 `attachments/`、`media/chat/`、`media/generated/`。
- `agents/computer/capture_debug.rs`、`locate/engine.rs`：有会话时只写 `data_root` 下的 `captures/`。没有会话 id 记 warn 并跳过，不写 `computer-captures/`。
- `models/message.rs` 和设置里指向 `computer-captures/` 的字段改为相对段或媒体引用。Client 与 Web 按引用加载。
- `turn_file_baseline.rs`：本地档位也写 `data_root` 下的 `turn-baselines/`。缺少环境返回错误，不调用 `host_env()`。
- `worktree/session.rs`、`worktree/settings.rs`：根固定为该用户 `data` 下的 `worktrees/`。列举走当前环境。

授权和密钥：

- `plugins/registry.rs` 停止读写系统目录根上的 `plugin-auth.json`，只读写该用户 home 下的那份。
- `secret_store.rs`、`plugin_data_host_dir` 改到新的用户根。写入路径规范化后必须在该用户 `home/` 内。

清单和库：

- `media/manifest.rs` 给模型的路径改为当前环境拼接的结果。`attachment_local_abs_path` 只留在服务端把字节交给浏览器之前，不进入模型上下文。
- 对话库、任务、附件元数据里的路径列改存「根 + 相对段」。已有行里的宿主绝对路径不在本计划迁移；智能体读不到它们。属主下载走 P4 的过渡读。

完成标准：

- 新上传的附件落在 `{user_key}/data/attachments/{会话}/`。数据目录不可写时上传失败，磁盘上不出现 `session-sandboxes/{原始用户 id}/attachments`。
- 本地档位保存设置、记忆、截图、回合基线、worktree、插件授权之后，文件出现在该用户的 `home/` 或 `data/` 下，系统目录根上不出现新的同类文件。
- 模型看到的路径以当前环境的 `home_root` 或 `data_root` 为前缀。库里的列是相对段。

## P3 关掉宿主桥

这一阶段之后，会话路径的读取失败只能是「环境里没有这个文件」或「路径不在 home/data」。不能再因为宿主磁盘上碰巧有文件就读成功。

媒体：

- `media/resolve.rs`、`media/media_ref.rs`：会话读取接收环境。路径先限制在该环境的两个根，再 `env.read_file`。`read_media_ref_bytes` 不再 `std::fs::read`。
- `media/image_dir.rs` 改为 `env.list_dir`。
- `media/store.rs` 的 `read_media_ref_preview`、`media_ref_body_in_env`：会话媒体先走环境。宿主解析成功不再短路。
- `media/exec.rs`、`media/ffmpeg.rs`、`media/video.rs`：处理进程和字节都走当前用户环境。环境里没有 ffmpeg 就返回错误。
- `media/public_download.rs`：令牌载荷改为用户、根、相对段、过期时间。兑换时只打开该用户 `data` 下的相对段。
- `media/oss.rs` 保持关闭。不新增宿主读取。
- `server/src/main.rs` 的下载和预览：`to_host` 结果规范化后必须位于该会话用户的 `home/` 或 `data/`，否则拒绝。远程环境继续把环境读出的字节写入响应。不为预览单独启动容器。
- `tools/media_understand/dispatch.rs`：图片、PDF、音频都从当前用户环境读。`media_env()` 在环境缺失或用户不匹配时返回错误。

文件和进程：

- `tools/file/mod.rs`、`tools/file/path.rs`：绝对路径先经路径表。两个根之外拒绝。去掉用户会话里 `host_env_at` 和 `ReadScope::Host` 放行任意绝对路径的分支。`tools/context.rs` 的生产入口保持必须已绑定；测试用的 `local_default` 不进入生产路径。
- 用户会话的 `exec` / `spawn` / `pty` 只在两个根的视图里运行。headless、desktop 用只含这两个挂载的容器。本地用户会话在 Linux 上用容器或等价目录沙箱，在 macOS 上用容器或 seatbelt。Windows 没有可用沙箱时，终端返回错误，不启动宿主 `Command`。
- `tools/terminal_elevated.rs`、`tools/terminal_askpass.rs`：用户会话返回错误。
- `console_session.rs`、`workspace_read.rs`：去掉 `host_env` 回退，使用会话用户的环境。
- `agents/coder/read_lints/detect.rs`：`probe_env` 使用当前会话环境。没有环境就报错。
- `plugins/hooks.rs`、`plugins/mcp.rs`：进程只在该用户环境里启动。不复用进程启动时用宿主拉起的客户端。环境不存在就失败。
- `workspace_service.rs`：列目录和建目录只通过传入的用户环境，范围是该环境的两个根。客户端传入的宿主项目路径返回明确错误。
- `worktree/git.rs`：没有 `with_git_env` 时返回错误，不再默认 `host_env()`。
- 定时任务、IM、webhook、子代理、电脑操控沿用会话属主重新绑定。环境守卫是线程局部的，`.await` 之后的入口调用 `bind_cached_or_resolve`，不沿用可能串用户的线程缓存。

守卫：

- `env/mod.rs` 的 `tool_and_media_layers_do_not_bypass_environment` 扩大扫描到 `agents/`、`plugins/`、`worktree/`、`skills/`、`memory/`、`workspace_service.rs`、`workspace_read.rs`、`console_session.rs`。
- 从白名单去掉服务于会话读写的 `media/store.rs`、`media/media_ref.rs`、`media/image_dir.rs`、`media/exec.rs`、`media/ffmpeg.rs`。
- 系统库、日志、安装目录的一次性拷贝、管理员的系统目录接口可以保留直连。白名单理由写明「不进入会话环境」。

完成标准：

- 非 admin 线程上残留整机 `LocalEnv` 时，读自己的附件会重新绑到该用户环境；读另一用户 `data/` 下的文件失败。
- 模型传入系统目录下的宿主绝对路径时，`media_understand`、文件工具、终端都拒绝。
- 下载接口对不属于该用户 home/data 的翻译结果返回拒绝。公开下载令牌不能兑换系统目录里的其它文件。
- 符号链接指向用户目录以外时，读取失败。
- Windows 上无沙箱时，用户会话的终端得到明确错误，而不是读到用户目录以外的文件。
- `cargo test -p pointer-core --lib env::mounts env::session_env::tests media::` 与文件工具、worktree、公开下载相关单测通过。不跑会改写全 crate 的 `cargo fmt -p pointer-core`。

## P4 文案和旧路径

- 提示词（含 PDF 提取说明）改为使用环境已经给出的附件路径，也就是当前 `data_root` 下的相对段。不写 `localPath`，不写固定的 `/data/attachments/...`。
- `skills/pointer-manager/references/data-directory.md`、`docs/zh-CN/design/unified-workspace-mount.md` 中仍把 `/skills`、`/plugins`、`/workspace`、`PointerApp/computer-captures` 写成会话位置的句子，改成两个根加相对段。
- 旧目录只保留属主下载的过渡读：校验会话属主后，允许服务端读 `conversation-media/`、`generated-media/`、`session-sandboxes/{该用户}/`。智能体工具和公开令牌不走这条过渡读。

## 不在本计划内

- 把旧文件搬进新的用户目录，或把旧卷当作在线环境的读回退。
- 安装包升级后合并用户已经改过的 `skills/system`、`plugins/system`、`agents/system`。
- 设置页的文案和布局。
- 匿名目录的清理周期。
- Composer 视频的对象存储上传。
- 管理员系统设置界面的具体页面。接口边界按方案执行：系统目录只由服务进程读写，不绑进聊天环境。

## 建议顺序

P0 和 P1 接连做。做完之后三档已经指向同一棵用户目录，技能不再从进程家目录加载。P2 让新文件和库存不再携带宿主绝对路径。P3 是阻止跨用户读取的部分，其中进程隔离依赖 P0 的路径表，并且要单独处理 Windows 没有沙箱时的失败行为。P4 跟着 P3 改提示词。

P0、P1 大约一个短迭代。P2 一个短迭代。P3 是主要工作量，按媒体、文件工具、进程（终端 / hook / MCP / git / 控制台）三小步合并。每步都要有拒绝越界路径的测试。

## 执行面实现

目标见 [user-directory-isolation.md](user-directory-isolation.md) 的「执行面」。一次改完线程槽的类型，不在现有 `Environment` 上再加范围判断。系统账本保持现在的写法：`ConversationStore` 直接打开系统目录里的 SQLite，不为它新建环境。

### 1. 换线程槽

新增 trait `SessionEnv`。方法只保留会话需要的：`home_root`、`data_root`、`workspace_root`、`path_map`、文件读写、`exec`、`spawn`、`pty_open`。不包含恒等映射，不包含 `ReadScope::Host`。

三个实现：

- `LocalSession::open(user, conversation)`：`ensure_user_layout` 之后包一层现在的 `LocalEnv::for_user`。`exec` / `spawn` / `pty` 继续走目录沙箱。
- `DockerSession`、`RemoteSession`：包现在的容器和远端实现。客人根仍是 `/home/pointer` 和 `/data`，宿主目录记在实现内部。
- 构造失败就返回错误，不造出一个能读整机的替身。

`CURRENT_ENV` 改成 `Option<Arc<dyn SessionEnv>>`。`EnvironmentGuard::enter` 只接受 `SessionEnv`。对外函数改名为 `current_session`、`require_session`。`host_env`、`host_env_at`、`effective_env`、`LocalEnv::new` 从公共接口删掉，不能再放进这个槽。

容器翻译路径之后的宿主读取改成 `env` 模块私有的 `VolumeIo`。它只提供「读这一条已经映射过的宿主路径」，不实现 `SessionEnv`。`docker.rs` 里现在的 `LocalEnv::new("/tmp")` 和 `LocalEnv::new(workspace_host)` 换成它。

没有会话 id 的本地工作区请求不再调用 `local_env_for`。调用方必须带会话；没有会话就返回「缺少会话标识」。

这一步会让下面这些文件编译失败，在同一次改动里改完：

| 调用点 | 改成 |
| --- | --- |
| `session_env.rs` 的绑定、缓存、复用 | 缓存和线程槽都是 `SessionEnv`。复用只比较这两份是不是同一次 `open` 的结果。工作区取 `workspace_root()` |
| `tools/file`、`tools/terminal`、`tools/context` | 入口参数或 `require_session()`。删掉 `host_env_at` 和 `kind == Local` 时直接 `path.exists()` 的分支 |
| `media/store.rs`、`media_ref.rs`、`exec.rs`、`ffmpeg.rs`、`video.rs` | 会话读写用 `require_session()`。对象存储上传保持关闭 |
| `plugins/hooks.rs`、`plugins/mcp.rs` | 没有 `SessionEnv` 就返回错误。stdio 用这份环境 `spawn`，进程启动时不连接用户插件 |
| `console_session.rs`、`workspace_read.rs`、`workspace_service.rs` | 有会话就用该会话的 `SessionEnv`。没有会话返回错误 |
| `capture_debug.rs`、`turn_file_baseline.rs` | 写 `data_root` 下的 `captures`、`turn-baselines`。没有会话环境就返回错误，调试截图记 warn 后跳过 |
| `read_lints`、`worktree/git.rs` | 使用已经绑定的 `SessionEnv`。没有就返回错误 |
| 键鼠、剪贴板、截屏 | 不进入 `SessionEnv`。用户会话调用时返回错误 |
| 单测里 `EnvironmentGuard::enter(LocalEnv::new(...))` | 改为 `LocalSession::open` 到临时用户树 |

完成时，`host_env()` 不再出现在 `tools/`、`media/`、`plugins/`、`agents/`、`workspace_*.rs`、`console_session.rs`。线程上无法残留一份整机环境，因为槽的类型放不进去。

### 2. 用户树

新增 `UserTree`。`open(user, conversation)` 得到这个用户的 `home` 和 `data`，读写前把路径规范化，结果必须仍在这两棵目录里。没有 `exec`。

设置接口和插件导入用它写 `{home}/.pointer/settings.json`、`provider_keys.enc`、插件目录。home 里还没有设置文件就用默认值，不读取系统目录里的 `user_settings.json`。部署配置仍由现有的系统目录读写完成，不经过 `UserTree`。

`MemoryStore::user_base_dir` 改为 `Result`，目录用 `UserTree` 或当前 `SessionEnv` 的 `data_root/memories`。失败直接返回，不再写系统目录的 `memories/{segment}`。

### 3. 路径只留一种

`SessionEnv` 上只留一个解析：`to_host` 成功就用它返回的宿主路径读字节；只有 `to_agent` 成功时入参已经是宿主路径。都不中返回 `PATH_NOT_MAPPED`。删掉 `ensure_inside_user_env` 里「没有环境或恒等映射就放行」，也删掉文件工具里 `starts_with(home/data)` 的另一套判断。

新的附件、截图、基线、令牌和库列使用已有的 `UserRelPath`。落库形式是 `data:相对段` 或 `home:相对段`。给模型的路径用当前 `SessionEnv` 的根拼出来。读到旧的宿主绝对路径时返回错误。截图的旧键 `{日期}/{会话}/{文件}` 仍然能预览。回合基线的旧绝对路径不打开，列表里跳过。

公开下载令牌的载荷是 `user`（`users` 下的目录名，匿名为 `anon/{会话}`）、`root`（`home` 或 `data`）、`rel`、`exp`。兑换时拼到该用户的树上，规范化后必须仍在这棵树里。只有 `rel` 的旧令牌，以及 `rel` 本身是宿主绝对路径的令牌，兑换失败。

插件注册表按 `open` 时的用户键分桶，`scan` 只替换这一桶。

### 4. 命令边界

`LocalSession` 的进程入口只有沙箱后的实现。macOS 用 `sandbox-exec -p` 带上本次的配置文本：默认拒绝文件读写，只读放行 `/usr`、`/bin`、`/sbin`、`/System`、`/Library`、`/opt`、`/dev`，读写放行这两棵目录和本次临时目录。Linux 的 `bwrap` 继续只读挂载运行所需的系统目录，只绑定这两棵用户目录。Windows 返回「当前平台没有可用的目录沙箱」。

### 5. 守卫

扫描扩到 `agents/`、`plugins/`、`worktree/`、`memory/`、`workspace_service.rs`、`workspace_read.rs`、`console_session.rs`。这些目录禁止出现 `host_env`、`host_env_at`、`LocalEnv::new`。仍直接碰文件系统的位置留在白名单，理由写「不进入会话环境」。工具层、媒体层和 coder 的原有扫描继续生效。

### 验收

- 把 `LocalEnv::new` 放进 `EnvironmentGuard::enter` 时编译失败。
- 未绑定就调用文件工具、`media_understand`、钩子，返回「缺少当前用户的执行环境」，宿主上存在该文件也不读取。
- 两个用户的会话先后跑在同一个工作线程上，后一个读不到前一个的 `data/`。
- 对话记录仍只出现在系统目录的 `conversations.db`，不经过 `SessionEnv`。
- 设置保存在该用户 `home/.pointer/settings.json`。
- `cargo test -p pointer-core --lib env::session_env::tests env::mounts user_data_paths`，加上文件工具、媒体、钩子相关测试。不跑会改写全 crate 的 `cargo fmt -p pointer-core`。

## 收口步骤

P0、P1 已经接上用户树。下面八步关掉还敞着的宿主读取。会话代码只接受带范围的环境。整机 `LocalEnv`（`path_map` 恒等、`session_scope` 为空）只留给系统目录维护和容器里翻译宿主路径，不作为会话失败后的退路。

范围怎么认：

- 在 `Environment` 上增加 `session_scope()`。`LocalEnv::for_user`、创建容器和远端环境时写入。内容是这份用户树的宿主 `home` 与 `data`（已规范化），再加上登录用户的 `user_key`，或匿名会话的会话 id。
- `LocalEnv::new`、`from_settings`、`host_env()`、`host_env_at` 的范围是空。空范围不是任何会话的环境。
- 容器里的 `home_root()` / `data_root()` 仍是 `/home/pointer` 和 `/data`，每个用户都一样，不能用来区分用户。区分用户看存在环境上的宿主树。`DockerEnv::skills_root()` 改为 `/home/pointer/.pointer/skills`，不再返回没有挂载的 `/skills`。
- `env_matches_session_user` 改为同时看用户和会话：空范围返回 false；登录用户只比对 `user_key`；匿名会话还要比对会话 id。用户 id 为空不再直接算匹配。
- `thread_env_reusable` 要求线程上的环境和该会话缓存里的环境范围相同，且都不是空范围。档位相同不够。
- 复用成功时，工作区只用 `env.workspace_root()`。调用方传入的宿主项目路径只在 `materialize_env` 里经过 `local_workspace`，落在两棵目录外就改到 `data/workspaces`。
- `bind_cached` 看到线程上已有环境时，范围对不上当前会话就不当作已绑定，交给 `bind_and_resolve_workspace` 重绑。

读文件只留一条：

- 新增 `session_host_file(env, path)`。范围为空就返回「缺少当前用户的执行环境」。`to_host` 成功就读它返回的宿主路径；只有 `to_agent` 成功时，入参已经是宿主路径，读入参。两条都不中就返回 `PATH_NOT_MAPPED`。不要对原始的容器路径做 `std::fs::read`。
- `read_media_ref_bytes`、预览、图片目录、ffmpeg 和视频处理都走这个函数。没有环境或整机环境时直接失败，不再因为宿主磁盘上有文件就读成功。
- `runtime_env_for`：工具上下文里有用户或会话时，宿主绝对路径也不再返回 `Ok(None)`。当前环境范围不符就用会话缓存；缓存也没有就返回错误。
- `env_exists`、`env_is_dir`、`env_canonicalize`：范围非空的本地环境走 `env.metadata` / `env.canonicalize`，不再因为 `kind == Local` 就调用 `path.exists()`。
- `read_allowed` 的用户范围改为问路径表，不再 `starts_with(home/data)`。未绑定且带有会话上下文时，在落到默认的整机读范围之前就返回错误。
- `tool_env`：会话上下文存在时，整机环境和缺环境都返回「文件工具缺少当前用户的执行环境」。

进程不再退回宿主：

- `plugins/hooks.rs` 删掉拿不到环境后调用 `host_env()` 的分支，改为返回错误。
- MCP 在进程启动时不连接用户插件。会话里的工具调用用该会话的环境 `spawn`。判断标准改为「范围非空」，不再用 `kind != Local`。本地用户会话因此不会复用宿主上已经拉起的 stdio 进程。
- 键鼠、剪贴板、截屏继续走 `ensure_host_execution_allowed`。用户会话里这些调用返回错误。它们不读取用户文件，也不改成 `host_env()` 去读。
- `console_session.rs`、`workspace_read.rs`、`worktree/git.rs`、`read_lints` 的探测：没有范围匹配的会话环境就返回错误，不再调用 `host_env()`。

失败就返回错误：

- `MemoryStore::user_base_dir` 改为返回 `Result`。`memories_host_dir` 失败时把错误交回去，不再拼到系统目录的 `memories/{segment}`。空用户 id 必须带会话 id，否则就是现在 `user_tree_dir` 已有的那个错误。
- `capture_store` 在环境缺失或范围为空时返回错误，删除会话路径上的 `local_capture_store()`。截图只写 `data_root/captures`。没有会话 id 的调试调用记 warn 并跳过，不写 `computer-captures/`。
- `env_for_baseline_store` 删除「工作区不像容器路径就用 `host_env()`」。基线只写 `data_root/turn-baselines`。
- 设置接口按登录用户调用 `ensure_user_layout`，读写 `{home}/.pointer/settings.json` 和 `{home}/.pointer/provider_keys.enc`。写之前把规范化后的路径限制在这个 home 里。这个接口用进程自己的文件 IO，不把整机环境绑进聊天。会话不再读系统目录里的 `user_settings.json` 和 `provider_keys.enc`。home 里还没有设置文件时用默认值，不把旧的全局文件抄过来。部署级 `settings.json`、通道、模型目录、`local_platform_settings.json` 留在系统目录。
- 公开下载令牌的载荷改为 `user_key`、根类型、相对段、过期时间。兑换时拼到该用户的树上，规范化后必须仍在这棵树里。带宿主绝对路径的旧令牌兑换失败。
- 新写入的附件和会话路径列使用 `UserRelPath`。给模型的路径是当前环境的根加上相对段。读到旧的宿主绝对路径时，工具返回错误，不打开那个文件。

同一进程里的用户状态：

- 插件注册表按范围 id 分桶。`scan` 只替换当前用户那一桶。进程启动时不插入任何用户插件。

命令的可见范围：

- macOS 每次执行用 `sandbox-exec -p` 带上独立的配置文本，不再写 `pointer-sandbox-{pid}.sb`。配置默认拒绝文件读写，然后只读放行加载器和可执行文件所需的 `/usr`、`/bin`、`/sbin`、`/System`、`/Library`、`/opt`、`/dev`、`/private/etc`、`/etc`、`/.vol`。根目录只放行这一项本身，用来解析 `/var`、`/tmp`、`/etc` 符号链接。`/var` 与 `/private/var` 只放行元数据，另外只读 `/var/select` 和 `/var/db/xcode_select_link`，这样 `/usr/bin/git` 能找到开发者目录，同时读不到 `/var/folders` 里的其他文件。读写放行这两个用户目录、`/tmp`、`/private/tmp` 和 `/dev/null`。去掉 `(allow default)`。
- Linux 的 `bwrap` 继续只读挂载 `/usr`、`/lib`、`/lib64`、`/bin`、`/sbin`、`/etc`、`/opt`，并只绑定这两棵用户目录。这是运行命令所需的系统目录，不包含任何用户树的父目录。不把这说成完整的文件系统监狱。
- Windows 维持现在的拒绝，并补一条断言：`confine_argv` 返回「当前平台没有可用的目录沙箱」。

守卫最后改。媒体、文件、钩子、截图、基线不再为会话字节调用 `std::fs` 之后，把这些文件从 `tool_and_media_layers_do_not_bypass_environment` 的白名单去掉，并把扫描扩到 `agents/`、`plugins/`、`worktree/`、`memory/`、`workspace_service.rs`、`workspace_read.rs`、`console_session.rs`。系统库、日志、安装目录的一次性拷贝、设置接口按用户树做的前缀检查保留，理由写「不进入会话环境」。

每步至少留下这些测试：线程上残留整机 `LocalEnv`、会话缓存里是该用户的 `for_user` 时，重新绑定后读另一用户的 `data/` 失败，工作区位于该用户的 `data/workspaces`。`read_media_ref_bytes` 和文件工具在无环境、整机环境、越界宿主路径三种情况下都返回错误。钩子和 MCP 在本地用户会话里不调用 `host_env()`。记忆目录创建失败时系统目录下不出现新的 `memories/`。两个匿名会话的记忆目录不同。macOS 的包装参数是 `sandbox-exec -p`，不写按进程号共用的配置文件。

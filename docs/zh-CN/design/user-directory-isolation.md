# 用户目录隔离

> 状态：细化版  
> 范围：安装目录、系统目录、用户目录三分。本地 / headless / desktop / remote 共用同一套相对布局。技能、插件、内置智能体按用户拷贝。  
> 不在范围：旧文件搬迁、设置页外观、技能升级合并、匿名目录的清理周期、Composer 视频压缩和对象存储上传。

## 目标

除这一台部署共享的系统数据外，每个用户的文件和进程都只落在自己的用户目录里。宿主路径不进入模型上下文，工具也不把宿主文件系统当作第二条读写通道。

Client 与 Web 走同一套 `pointer-core` 逻辑。

## 执行面

系统账本和会话文件是两种类型。会话用的类型里没有「整机」这一种实现。缺了会话环境是编译错误或显式的 `Result` 错误，不是再去拿一份能读整台磁盘的环境。

现在的 `Environment` 同时表示这两件事。`LocalEnv::for_user` 只看见一个用户的两棵目录。`LocalEnv::new`、`host_env()`、`host_env_at` 是整机视图，路径表恒等。线程槽的类型是 `Arc<dyn Environment>`，两种都能放进去。媒体、文件、钩子、MCP 在槽是空的或者放进了整机视图时，就直接读宿主。每补一个调用点，下一个调用点仍能写出 `host_env()`。对话库本身没有走这条类型，它直接打开系统目录里的 SQLite。

之后只留三条通道。

**系统账本。** 对话记录、任务板、用量、定时任务表、部署配置、登录密钥、日志。函数形如 `conversations.append(user, …)`。实现打开系统目录里的库或文件，查询带用户条件。不接受模型传来的路径，也不实现 `Environment`。库里指向用户文件的列只存「根是 home 还是 data」加相对段。

**会话环境。** 智能体的文件、进程、媒体、钩子、MCP。线程槽里只能放这一种值。本地、容器、远端各自实现它，构造时就已经带上这个用户的 `home` 和 `data`。没有空范围的构造函数，也没有恒等路径表。工具入口把这份环境当作参数传下去。进程隔离写在实现内部：本地用目录沙箱，容器用挂载，远端用远端返回的两个根。功能代码不再写「本地就用 `std::fs`，否则走环境」。

**用户树。** 设置页和第一次拷贝技能用它。`UserTree::open(user)` 只能读写这个用户的 `home` 和 `data`，不能执行命令，不能放进会话线程槽。设置接口用它写 `{home}/.pointer/settings.json`，不借用整机环境。

容器把客人路径翻译成宿主路径之后，用实现私有的读取去拿字节。这个读取只接受已经落在该用户挂载点里的路径，不实现会话环境，也不能放进线程槽。键鼠和截屏是桌面能力开关：用户会话里返回错误。它不提供读文件的方法，避免再变出第四种环境。

新功能按这个归类。要碰用户文件或起进程，函数签名上就有会话环境，写不出宿主退路。要记一笔全机共享的账，加在系统账本上，碰不到用户目录。要在聊天之外改这个用户自己的设置，用用户树。新增一种档位只加一个会话环境实现，功能代码不跟着分支。

## 评估

方案成立，工作量集中在「关掉宿主桥」，不在目录搬家。

只留 `home` 和 `data` 两个可写根。子目录名固定。完整路径在拿到当前用户的环境之后，用 `env.home_root()` 或 `env.data_root()` 拼接。容器实现里这两个根是 `/home/pointer` 和 `/data`。本地实现里这两个根是该用户目录下的 `home/` 和 `data/`。功能代码、提示词、数据库都不写死这两段前缀。

切档位不搬文件，只更换环境实现。同一个 `user_key` 在各档看到的相对布局一致。

按这个目标核对当前代码，下面这些口子还开着。实现计划按它们逐项收口：

- 用户会话的 `LocalEnv` 仍是整机恒等映射，`data_root()` 还是空路径，`ReadScope::Host` 放行任意绝对路径。文件工具在没有会话环境时会退到 `host_env_at`。
- 路径表限制的是文件 API。终端、hook、MCP、git、ffmpeg 走的是进程。本地档位若仍用宿主 `Command`，路径表挡不住 `cat` 或 Python 打开用户目录以外的文件。
- 媒体解析用宿主绝对路径，宿主上 `stat` 成功就会跳过环境。`read_media_ref_bytes`、图片目录列举、预览、公开下载都还有这条短路。
- 新附件的回退目录仍是未挂载的 `session-sandboxes/{原始用户 id}/`。截图、回合基线、worktree 在本地档位仍写在系统目录根上。
- 技能运行时根是进程的 `~/.pointer/skills`、`{系统目录}/org-skills`、进程用户的 `~/.agents/skills`、以及系统目录里的内置副本。插件从进程家目录加载。内置智能体还会读进程用户的 `~/.agents` 和系统目录下的 `agents/`。`SKILL_DIR`、`POINTER_HOME`、`POINTER_SKILLS_DIR` 是进程级变量。
- 插件授权有两份：`plugins/registry.rs` 写系统目录根上的 `plugin-auth.json`；`user_data_paths` 又写用户 home 的 `.pointer/plugin-auth.json`。密钥文件已经落在用户 home，但解析的是旧卷名，并且直接 `std::fs`。
- 记忆在空用户 id 时共用一个匿名段，没有按会话拆开。用户运行时设置只有终端覆盖和并行度进了 `user-settings/{uid}.json`，模型密钥和界面偏好仍在全体共用的 `user_settings.json`。
- 控制台、工作区读取、回合基线、视频处理在缺少环境时退回宿主。提权终端和 askpass 是宿主能力。公开下载令牌的 `rel` 相对整个系统目录。
- 架构守卫只扫 `tools/`、`media/`、`agents/coder/`，扫不到 `agents/computer`、`plugins/`、`worktree/`、`workspace_service`、`skills/`、`memory/`。白名单还允许会话媒体直连宿主。
- 数据库和清单里存的是宿主绝对路径或写死的 `/data/...`。本地档位的根并不是这个字符串，库存不能带这个前缀。

技能和插件在用户第一次进入时拷进该用户的 `home`，之后只读这份拷贝。安装目录不被会话运行时打开。系统账号走同一条创建路径。每个用户一份磁盘占用。安装包升级后不改写用户已经拿走的拷贝。升级合并以后另定。

## 不变式

1. 先得到当前会话用户的环境，再拼接路径。调用形状是 `env.home_root().join(段)` 或 `env.data_root().join(段)`。
2. 环境实现内部才能出现容器根。`docker` / `mounts` 把用户目录翻译成容器里的 `/home/pointer` 和 `/data`。`user_data_paths` 里现有的 `guest_project_path` 这类写死前缀的函数删掉，改由环境拼接。
3. 库存、附件清单、媒体引用只存两样东西：根是 `home` 还是 `data`，加上相对段，例如 `attachments/{conversation_id}/{file}`。读的时候用当前环境的对应根拼回去。
4. 没有环境、环境用户与会话属主不一致、路径落在两个根之外，都返回错误。不调用 `host_env()`、`effective_env()`，也不因为宿主磁盘上有这个文件就读成功。
5. 文件、目录、删除、进程的 cwd，都先经路径表。符号链接用规范化后的真实路径判断，真实路径必须仍在这个用户的 `home/` 或 `data/` 里。
6. 用户会话的进程也只看见这两个根。只改 `read_file` 不够。
7. 布局目录创建失败、首次拷贝失败，都让本次绑定失败并打错误日志。不警告后继续，避免会话带着半套目录去读宿主。
8. 查询系统库必须带用户条件。日志可以记相对段，不记宿主绝对路径，不记附件正文。

## 三棵目录

**安装目录。** 随程序版本发布，只读。放二进制、内置技能、内置插件、内置智能体、原生扩展。用户创建时由服务进程从这里拷贝一次。会话环境不挂载它，智能体工具打不开它。

**系统目录。** 这一台 Pointer 一份，不挂进用户环境。沿用现有应用数据根（`POINTER_APP_DATA_DIR`，否则按平台默认：Linux `~/.local/share/PointerApp`，macOS `~/Library/Application Support/PointerApp`，Windows `%APPDATA%\PointerApp`）。进程以哪个操作系统用户运行，这个默认位置就在那个用户下。这是部署事实，会话工具看不到这棵树。

留在系统目录的内容：

- `conversations.db`、`task_boards.db`、`token_usage.db`、`token_usage_archives/`
- 部署配置 `settings.json`、通道配置、模型目录缓存、`local_platform_settings.json`
- 登录与机器密钥：`auth.dat`、`key.dat`，以及只给服务进程用的下载签名密钥
- 运行日志、沙箱注册表
- 对象存储等部署级凭证

`cron_jobs` 表留在对话库里。行上带属主。触发时绑定该用户的环境，工作文件写进该用户的 `data`。

**用户目录。** 一个 Pointer 账号一棵。目录名用现有 `user_key`（账号哈希），不用原始用户 id。未登录会话使用 `anon/{会话}`，每个会话一棵，记忆、设置、技能拷贝都不共用。

物理位置是 `{系统目录}/users/{user_key}/`。已有的 `{user_key}-home`、`{user_key}-data` 不作为在线环境的读路径，也不自动搬进来。

```text
{系统目录}/users/{user_key}/
  home/
    .pointer/
      settings.json
      .env                         该用户终端要注入的变量
      AGENTS.md
      plugin-auth.json
      plugin-secrets.enc.json
      skills/
        system/                    从安装目录拷贝
        org/                       组织下发时拷进这里
        user/                      用户自己添加的技能
      plugins/
        system/
        user/
      agents/
        system/
      plugin-data/{plugin_id}/
  data/
    workspaces/{project}/
    worktrees/{conversation_key}/
    attachments/{conversation_id}/
    media/chat/{conversation_id}/
    media/generated/{conversation_id}/
    captures/{date}/{conversation_id}/
    turn-baselines/{conversation_id}/{turn_id}/
    memories/
```

`home` 和 `data` 都可读写。用户可以改自己的技能、插件、智能体拷贝。安装目录保持不变。

## 两个根

| 用户目录 | 容器里环境返回的根 | 本地环境返回的根 |
|---|---|---|
| `{user_key}/home` | `/home/pointer` | `{用户目录}/home` |
| `{user_key}/data` | `/data` | `{用户目录}/data` |

本地档位用路径表做同样的限制，可见范围就是这两棵。headless 和 desktop 只挂这两个目录。remote 的根由远端环境返回，调用方仍是拼接，不另写一套前缀。

固定的是子目录名：

- 设置：`home_root` 下 `.pointer/settings.json`
- 终端变量：`home_root` 下 `.pointer/.env`
- 全局指令：`home_root` 下 `.pointer/AGENTS.md`
- 技能：`home_root` 下 `.pointer/skills/{system|org|user}/{name}`
- 插件：`home_root` 下 `.pointer/plugins/{system|user}/{id}`
- 智能体：`home_root` 下 `.pointer/agents/system/{name}`
- 插件数据、授权、密钥：`home_root` 下 `.pointer/plugin-data/`、`plugin-auth.json`、`plugin-secrets.enc.json`
- 记忆：`data_root` 下 `memories/`
- 附件、会话媒体、生成媒体、截图、回合基线、工作区、worktree：`data_root` 下对应子目录

工作区的活动目录是 `data_root` 下的 `workspaces/{project}` 或 `worktrees/{conversation_key}`。容器里这会表现为 `/data/workspaces/...`，因为容器的 `data_root` 就是 `/data`。本地则是该用户 `data/workspaces/...`。

## 设置怎么拆

部署级配置留在系统目录的 `settings.json`：监听地址、档位、通道、模型目录、对象存储、登录。全体用户不能通过改自己的文件影响这些项。

以下内容进入该用户 `home_root` 下的 `.pointer/settings.json`，经该用户环境读写：

- 现在的 `user-settings/{uid}.json`（终端环境覆盖、并行工具数、并行子代理数）
- 现在全局 `user_settings.json` 里属于使用者本人的字段：界面语言、模型选择、该用户自己的 provider 密钥、以及其它「改了只该影响这个人」的项

系统目录里的 `user_settings.json` 和 `provider_keys.enc` 在切换完成后不再作为会话读取源。密钥仍用现有的机器绑定加密，换机器需要重新填写。这不影响隔离：别的用户的环境读不到这份文件。

系统目录根上的 `.env` 只留给服务进程。用户终端注入的变量来自该用户 `home_root` 下的 `.pointer/.env` 和设置里的终端覆盖。

设置页外观不改。Client 与 Web 的设置接口改为读写当前用户的这份文件。

## 技能、插件、智能体

用户第一次进入时创建用户目录，并从安装目录拷贝：

- 技能 → `home/.pointer/skills/system/`
- 插件 → `home/.pointer/plugins/system/`
- 内置智能体 → `home/.pointer/agents/system/`

系统账号同样走这次创建。拷贝由服务进程完成，目标必须落在这个 `user_key` 的 `home/` 内。不跟随指向安装目录之外的符号链接。同名目录已存在则跳过该份，不覆盖。打一条 info 日志：用户、是否新建、各拷贝了多少。

会话运行时只扫描该用户 `home/.pointer/skills`、`plugins`、`agents` 下的子目录。不再把下面这些当作会话根：进程的 `~/.pointer`、`POINTER_HOME`、`POINTER_SKILLS_DIR`、`{系统目录}/org-skills`、`{系统目录}/skills`、进程用户的 `~/.agents`。这些环境变量可以继续给测试和安装布局使用，会话绑定之后忽略它们。

组织下发的技能拷进该用户的 `home/.pointer/skills/org/`，不另挂只读目录。策展、归档作用在这份用户拷贝上。

`AGENTS.md` 的全局文件是该用户 `home_root` 下的 `.pointer/AGENTS.md`，经环境读取。沿工作区向上的链条停在 `data_root` 或 `home_root`，不走到宿主的 git 根。

终端子进程的 `SKILL_DIR` 设为该用户环境里技能库的路径（`home_root` 拼 `.pointer/skills`），不设成进程家目录。

插件、stdio MCP、hook 在该用户环境里启动。进程启动、尚未知道用户时不加载插件。不把一个宿主 MCP 客户端交给其它用户的会话复用。插件授权只写该用户的 `plugin-auth.json`。密钥只写该用户的 `plugin-secrets.enc.json`。

## 进程隔离

路径表能挡住文件 API，挡不住宿主进程。用户会话的 `exec`、`spawn`、`pty` 必须在只含这两个根的视图里跑。

- headless、desktop：现有容器，挂载表只留 home 和 data。工作目录用 `data_root` 拼出的工作区，不再使用镜像里的 `/workspace`。
- 本地档位的用户会话：同样不能用无限制的宿主 `Command`。Linux 上用现有容器或等价的目录沙箱。macOS 上用容器或 seatbelt。Windows 上有 Docker 时走同一份挂载表；没有可用沙箱时，终端、hook、MCP、git 返回错误，不退成整机进程。这一点做不到静默兼容，界面上要给出明确失败。
- 管理员维护系统目录的入口使用服务进程自己的宿主 IO，不把这棵系统目录绑进聊天环境。管理员自己的会话仍只使用管理员的用户目录。
- 用户会话不提供宿主提权。`terminal_elevated` 和 askpass 在用户会话里返回错误。

远程环境没有本机文件。下载和预览由环境读出字节再写入响应，不先为了预览去启动一个容器。

## 媒体、下载、引用

模型看到的路径是当前环境拼出来的路径。`attachment_local_abs_path` 只留在服务端内部，用来把字节交给浏览器，不写进模型上下文，也不出现在提示词里。

`pointer-media://` 新引用保存「用户 + 根 + 相对段」。解析时绑定该用户环境再读。宿主 `is_file` 成功不能跳过环境。

浏览器下载和预览：先确认会话属于该用户，`to_host` 的结果规范化之后必须位于这个用户的 `home/` 或 `data/`，再流式发送。越界就拒绝。

公开下载令牌（IM 大文件）的载荷改为用户、根、相对段和过期时间。兑换时只打开该用户数据根下的这个相对段。令牌不再携带「相对整个系统目录」的路径。

ffmpeg、视频处理在该用户环境里执行，输入输出都在 `data_root` 下。本地档位不再为了找 ffmpeg 去扫宿主 `PATH` 以外的位置；环境里没有 ffmpeg 就返回错误。

对象存储上传保持现有的关闭状态。重新启用时，字节必须来自该用户环境的读取，而不是宿主路径。

## 系统库

对话索引、任务板、用量、定时任务继续放在系统目录。库存归属、用户、会话、根、相对段。不存宿主绝对路径，不把附件正文放进共享库。

定时任务、IM、webhook、子代理、电脑操控、控制台，都按会话属主绑定环境。子代理继承父会话的用户环境。线程局部环境不会跟着 tokio 任务穿过 `.await`，每个恢复执行的入口都要按属主和档位重新绑定。

## 旧路径

在线环境不打开这些位置，也不把它们挂进容器：

- `{user_key}-home`、`{user_key}-data`
- `session-sandboxes/{原始用户 id}/`
- `conversation-media/`、`generated-media/`
- 系统目录根上的 `computer-captures/`、`turn-baselines/`、`worktrees/`
- `/skills`、`/plugins`、镜像里的 `/workspace`

属主的浏览器下载在确认归属后，可以暂时读 `conversation-media/`、`generated-media/`、`session-sandboxes/{该用户}/`。没有文件就报缺失。智能体工具、公开下载令牌、预览的宿主短路都不走这条过渡读。

没有会话 id 的定位调试图不再写系统目录。没有环境就记 warn 并跳过。

客户端若传入宿主项目目录，绑定拒绝这个路径并返回明确错误。工作区只能建在该用户的 `data/workspaces` 或 `home` 下。选择器列出的是当前环境里的这两个根，不是跑服务的操作系统用户的家目录。

## 跨平台和两端

相对段用 `Path` 拼接，不用字符串拼 `/data/...`。容器内的 POSIX 路径只存在于容器环境实现里。Windows 上本地环境返回 Windows 路径。

Client 与 Web 都通过同一绑定入口进入用户环境。界面展示媒体引用或环境内路径，不展示宿主绝对路径。

## 实施顺序

任务、文件和完成标准见 [user-directory-isolation-plan.md](user-directory-isolation-plan.md)。

1. 环境只保留两个根，本地路径表与容器挂载使用同一份计划。用户会话的本地环境实现 `data_root()`，并去掉整机读范围。
2. 第一次进入时创建用户目录并拷贝技能、插件、智能体。运行时只读这份拷贝。
3. 新的设置、记忆、附件、媒体、截图、基线、worktree、授权和密钥写入 home 或 data。库存改存相对段。
4. 媒体、下载、公开链接、文件工具、终端进程、lint、hook、MCP、控制台、目录选择和 git 只走当前用户的环境。
5. 提示词和技能说明改成「当前环境的 data 根 / home 根」，并收紧架构守卫的扫描范围。

## 仍待单独立项

- 安装包升级后，如何把新的系统技能、插件、智能体合并进已有用户的 `system/`，同时留下用户改过的文件。
- 未登录的 `anon/{会话}` 目录随会话删除，还是按时间清理。本方案只保证各会话单独建树。
- IM 的 `user_key` 仍由通道里的发送者标识决定。它和 Pointer 登录名不是同一个字符串时，用户目录是两棵。通道历史若含有该用户的消息正文，跟随该 IM 用户的数据根，不留在一个全体可列的目录里。
- 管理员维护系统目录的界面放在哪里。边界已经确定：那些界面调用服务端的系统目录接口，不把系统目录绑给聊天环境。

# Pointer

**桌面版开源企业智能体基座。**

把数字员工做出来，跑在员工日常用的电脑上，也能在云上长期跑。桌面端和 Web 端能力一致，支持 Windows、macOS、Linux。

| 你想要 | 怎么拿 |
| --- | --- |
| 装上就能用 | [官网下载](https://pointer.readflowai.com/download) |
| 放进公司自己的环境 | [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) |

[English](README.md)

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

## 为什么选择 Pointer

数字员工要在真实办公环境里干活：协议能商用、资源占用低、扛得住长时间跑，还得看得见屏幕、动得了鼠标键盘，模型也得你自己选。

<table>
  <thead>
    <tr>
      <th></th>
      <th>常见智能体</th>
      <th>Pointer</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>产品形态</td>
      <td>开源的多半是命令行，主要给开发用</td>
      <td>桌面版，而且开源。企业里谁都能上手</td>
    </tr>
    <tr>
      <td>开源商用</td>
      <td>桌面版多半按席位收费</td>
      <td>完全免费，想怎么改就怎么改。<a href="LICENSE">Apache 2.0</a></td>
    </tr>
    <tr>
      <td rowspan="3">资源占用</td>
      <td>安装包下载约 300 MB</td>
      <td>约 25 MB</td>
    </tr>
    <tr>
      <td>常态内存 1 GB 以上</td>
      <td>约 300 MB</td>
    </tr>
    <tr>
      <td>一个任务就能把 CPU 打满。多开几个任务，本机其他工作就卡了</td>
      <td>运行大约吃单核 10%。十几个子智能体同时跑也没问题，其他任务基本不受影响</td>
    </tr>
    <tr>
      <td>超长会话</td>
      <td>上下文一满就得新开，以前聊过的容易丢</td>
      <td>想一直聊就一直聊。自动分页、自动压缩上下文，重要节点可标里程碑，随时跳回去</td>
    </tr>
    <tr>
      <td>电脑操控</td>
      <td>没有手和眼，或者只能搞浏览器、靠无障碍接口</td>
      <td>自带纯视觉电脑操控：看截图，用鼠标键盘操作。电脑上有界面的软件都能控（还在打磨），补上数字员工缺的那一块</td>
    </tr>
    <tr>
      <td>供应商</td>
      <td>绑死某一家模型，或绑死某一家生态</td>
      <td>模型随便换，生态也不绑死</td>
    </tr>
  </tbody>
</table>

数字员工不该跟真人抢电脑。上面这些内存和 CPU 数字，就是为了让它安心待在日常办公的机器上。Pointer 已经在真实业务里高强度跑了三个多月，每天差不多用掉 1000M Token。

## Pointer 能做什么

### 通用 Agent 能力

| 场景 | 怎么做 |
| --- | --- |
| 大任务拆开一起干 | [子智能体](docs/user/subagents.md) 可以去翻代码、改代码、操控电脑，主对话不用停 |
| 看得见、点得动 | 纯视觉电脑操控：截屏理解当前界面，再点击、输入、滚动、打开应用。有界面的软件都能控（还在打磨） |
| 人在哪，它就在哪 | 桌面端本地跑，也可以挂在[云主机](docs/user/cloud-host.md)上长期跑。还能接[飞书、钉钉、企业微信、微信](docs/user/im-channels.md) |
| 外部系统来触发 | [Webhook](docs/user/webhook.md)、定时任务都行 |
| 模型和工具自己定 | 不绑死某一家模型，也不绑死某一家生态。内置千问、深度求索，也可接豆包、Kimi、智谱、OpenRouter，以及任意 OpenAI 兼容接口。外部工具走 [MCP](docs/user/mcp.md) |

### 为 Skill 开发与测试补上的能力

岗位流程写成可复用 Skill 之后，要改得动、装得上、改完马上就能再跑。

| 场景 | 怎么做 |
| --- | --- |
| 按通用格式来写 | [Skills](docs/user/skills.md) 用常见的技能说明格式，和 Codex 一类目录兼容。说明、参考资料、脚本、资源分开放 |
| 装进技能库再打开 | 技能库里勾选启用，也支持 zip 导入。本机已有的 Codex、Claude、OpenClaw、Hermes 技能可以一键迁过来 |
| 对着对话就能改 | 新建、修改、审查、打包交给写代码的子智能体。也可以打进[插件](docs/user/plugins.md)一起发 |
| 改完立刻验 | 技能说明每次现读磁盘，不用重启，下一轮就是新内容 |
| 脚本和界面都要验 | 终端里跑脚本，代码检查看改动对不对；要动界面时，用电脑操控或浏览器自动化看结果成不成 |
| 缺环境就补环境 | 缺 Node、Python 之类，对话里装好再继续测 |

想上手，看 [快速上手](docs/user/getting-started.md)。

## 架构

桌面端、Web 端、IM 通道共用一套 `pointer-core`：界面只负责交互，编排、工具、Skills、会话都在核心层。

![Pointer 架构](docs/design/pointer-architecture.zh-CN.svg)

分层说明见 [docs/developer/architecture.md](docs/developer/architecture.md)。

## 文档

| 读者 | 入口 |
| --- | --- |
| 用户 | [docs/user/](docs/user/README.md) |
| 开发者 | [docs/developer/](docs/developer/README.md) |
| 贡献者 | [CONTRIBUTING.md](CONTRIBUTING.md) · [DEVELOPMENT.md](DEVELOPMENT.md) |
| 安全 | [SECURITY.md](SECURITY.md) |
| 变更 | [CHANGELOG.md](CHANGELOG.md) |

完整索引：[docs/README.md](docs/README.md)。

## 开发

桌面端是 Tauri 2 + Vue 3，Web 端由 `pointer-server` 提供同一套界面。对话、工具、Skills、Agent 都在 `crates/pointer-core`。

```bash
npm install
npm run tauri:dev
npm run server:dev
npm run web:dev
```

日常开发不用设 `POINTER_EDITION`。细节见 [DEVELOPMENT.md](DEVELOPMENT.md)。

## 发布

改根目录 `VERSION`，同步版本号，再推一个 `v*.*.*` 标签。CI 会打 Windows / macOS / Linux 包，并在 [GitHub Releases](https://github.com/chinphing/agent-pointer/releases) 生成草稿。

```bash
# 编辑 VERSION 后：
npm run version:sync
npm run version:check

git tag v0.1.2
git push origin v0.1.2

# 或在本机打安装包：
npm run tauri:build
npm run server:build
```

打包细节：[docs/contributing/cross-platform-build.md](docs/contributing/cross-platform-build.md)；版本号规则：[docs/contributing/versioning.md](docs/contributing/versioning.md)。

## 致谢

Pointer 的不少设计受到这些项目启发，在此致谢：

- [OpenClaw](https://github.com/openclaw/openclaw)（Peter Steinberger / [OpenClaw Foundation](https://openclaw.org)）
- [Hermes Agent](https://github.com/NousResearch/hermes-agent)（Nous Research）

## 许可证

Apache License 2.0。版权所有 © 2026 刘新星（https://pointer.readflowai.com）。商标与官方域名见 [NOTICE](NOTICE)。

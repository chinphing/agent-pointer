[English](../en/README.md) | 简体中文

## 现在做这一件事

| 我是… | 这一件事 |
|--------|------------|
| **用户** | 打开 [官网下载](https://pointer.readflowai.com/download)，安装后发一句「你好」。在仓库里用 `./scripts/quick-deploy.sh start standalone-client --runtime bin` 打开桌面程序，不需要先编译。细节在 [`user/getting-started.md`](user/getting-started.md) |
| **开发者** | 仓库根目录执行 `npm install`，然后 `npm run tauri:dev`。说明在 [`developer/`](developer/README.md) |

## 跑起来之后

| 我是… | 再看 |
|--------|------------|
| **部署 / 打包** | [`deploy/`](deploy/README.md) |
| **仓库贡献者** | [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)、[`../../DEVELOPMENT.md`](../../DEVELOPMENT.md) |
| **核心维护者** | [`internals/`](internals/README.md)；设计记录见 [`design/`](design/README.md) |

## 这是什么

把数字员工做出来，跑在员工日常用的电脑上，也能在服务器上长期跑。智能体和模型都可以留在你自己的环境里，也不按坐席收费。桌面端和 Web 端能力一致，支持 Windows、macOS、Linux。

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
      <td><strong>桌面版 + Web 服务端</strong>。<strong>部署简单，企业里谁都能上手</strong></td>
    </tr>
    <tr>
      <td>技能开发</td>
      <td>技能在开发环境跑通，上线却因环境不同失败</td>
      <td><strong>同一套 harness</strong>：开发和运行技能一致性更高。<strong>专为 Skill 批量测试优化</strong>：后台高性能并发执行，单核支持 20 个子智能体</td>
    </tr>
    <tr>
      <td>企业特性</td>
      <td>各人各自配账号和密钥，没有统一管控</td>
      <td><strong>统一登录、统一 API Key 管理、统一 Skill 分发、对话记录上报，还有更多</strong></td>
    </tr>
    <tr>
      <td>开源商用</td>
      <td>桌面版多半按席位收费</td>
      <td><strong>完全免费，想怎么改就怎么改。</strong><a href="https://github.com/chinphing/agent-pointer/blob/main/LICENSE">Apache 2.0</a></td>
    </tr>
    <tr>
      <td rowspan="3">资源占用</td>
      <td>安装包下载约 300 MB</td>
      <td><strong>约 25 MB</strong></td>
    </tr>
    <tr>
      <td>常态内存 1 GB 以上</td>
      <td><strong>约 300 MB</strong></td>
    </tr>
    <tr>
      <td>一个任务就能把 CPU 打满。多开几个任务，本机其他工作就卡了</td>
      <td><strong>运行大约吃单核 5%</strong>。<strong>20 个以上子智能体同时跑也没问题</strong>，其他任务基本不受影响</td>
    </tr>
    <tr>
      <td>超长会话</td>
      <td>上下文一满就得新开，以前聊过的容易丢</td>
      <td><strong>想一直聊就一直聊。</strong>自动分页、自动压缩上下文，重要节点可标里程碑，随时跳回去</td>
    </tr>
    <tr>
      <td>供应商</td>
      <td>绑死某一家模型，或绑死某一家生态</td>
      <td><strong>模型随便换</strong>，生态也不绑死</td>
    </tr>
  </tbody>
</table>

数字员工不该跟真人抢电脑。上面这些内存和 CPU 数字，就是为了让它安心待在日常办公的机器上。Pointer 已经在真实业务里高强度跑了三个多月，每天差不多用掉 1000M Token。

---

*仓库根目录还有 [LICENSE](../../LICENSE)、[SECURITY.md](../../SECURITY.md)、[CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md)、[CHANGELOG.md](../../CHANGELOG.md)。*

[English](../en/README.md) | 简体中文

## 入口

| 读者 | 从这里开始 |
|--------|------------|
| **用户** | 打开 [官网下载](https://pointer.readflowai.com/download)，安装，登录，发一句「你好」。社区版和自行编译见 [快速上手](user/getting-started.md) |
| **开发者** | 客户端：`npm install` 然后 `npm run tauri:dev`。Web Server：两个终端分别执行 `npm run server:dev` 和 `npm run web:dev`。说明在 [`developer/`](developer/README.md) |

## 相关文档

| 文档 | 说明 |
|--------|------------|
| **使用指南** | [界面、设置、Skills、IM](user/README.md) |
| **架构说明** | [桌面和 Web 共用一套核心](developer/architecture.md) |
| **部署** | [打包与部署](deploy/README.md) |
| **贡献** | [贡献者指南](contributing/README.md)、[开发调试说明](../../DEVELOPMENT.md) |
| **内部机制** | [内部机制](internals/README.md)。设计记录在仓库 [design/](design/README.md)，不上文档站 |

## 这是什么

把数字员工做出来，跑在员工日常用的电脑上，也能在服务器上长期跑。智能体和模型都可以留在你自己的环境里，也不按坐席收费。桌面端和 Web 端能力一致，支持 Windows、macOS、Linux。

## 为什么选择 Pointer

数字员工要在真实办公环境里干活：协议能商用，桌面客户端和 Web 同一套能力，资源占用低，只靠截图和鼠标就能操作屏幕，一场对话也可以一直接着聊。

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
      <td>企业特性</td>
      <td>开源多半是命令行，按席位收费，技能各管各的</td>
      <td><strong><a href="https://github.com/chinphing/agent-pointer/blob/main/LICENSE">Apache 2.0</a></strong>，可商用、可自行修改。<strong>桌面客户端和 Web 同一套能力。</strong>技能统一管理和开发</td>
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
      <td>电脑操控<br>看得见的软件都能操作</td>
      <td>靠浏览器调试协议或系统无障碍树</td>
      <td><strong>只看截图、只操作鼠标。</strong>不依赖 CDP，也不依赖无障碍树。你能操作的软件，它也能。<strong>Qwen3.8-flash 即可用</strong>，同样的任务，成本大约是 Codex 的 <strong>1/20</strong></td>
    </tr>
    <tr>
      <td>超长会话</td>
      <td>上下文一满就得新开，以前聊过的容易丢</td>
      <td><strong>想一直聊就一直聊。</strong>自动分页、自动压缩上下文，重要节点可标里程碑，随时跳回去</td>
    </tr>
  </tbody>
</table>

<p class="why-pointer-shots">
  <img src="../why-pointer/home.zh-CN.png" alt="Pointer 主页" width="49%" />
  <img src="../why-pointer/models.zh-CN.png" alt="Pointer 模型设置" width="49%" />
</p>

---

*仓库根目录还有 [LICENSE](../../LICENSE)、[SECURITY.md](../../SECURITY.md)、[CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md)、[CHANGELOG.md](../../CHANGELOG.md)。*

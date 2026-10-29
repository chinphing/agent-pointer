English | [简体中文](../zh-CN/README.md)

## Entry

| Reader | Start here |
|--------|------------|
| **A user** | Open the [download page](https://pointer.readflowai.com/download), install, sign in, and send "Hello". The community build and compiling it yourself are in [Getting started](user/getting-started.md) |
| **A developer** | Client: `npm install`, then `npm run tauri:dev`. Web Server: `npm run server:dev` and `npm run web:dev` in two terminals. Notes: [`developer/`](developer/README.md) |

## Related documents

| Document | Where |
|--------|------------|
| **User guide** | [The window, settings, Skills, and IM](user/README.md) |
| **Architecture** | [Desktop and web share one core](developer/architecture.md) |
| **Deployment** | [Packaging and deployment](deploy/README.md) |
| **Contributing** | [Contributor guide](contributing/README.md), [Development notes](DEVELOPMENT.md) |
| **Internals** | [Internals](internals/README.md). Design records live in the repository at [design/](design/README.md) and are not on this site |

## What is this

Build and run digital employees on the computer people already use, and keep them running stably on a server. Both the agent and the model can stay inside your own environment, with no per-seat fees. The desktop app and the web app share the same capabilities, on Windows, macOS, and Linux.

## Why Pointer

A digital employee has to work inside a real office setup: a license that allows commercial use, a desktop client and a web app, a small footprint, eyes and a mouse on the screen, and one conversation that can keep going.

<table>
  <thead>
    <tr>
      <th></th>
      <th>Typical agents</th>
      <th>Pointer</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Enterprise Feature</td>
      <td>Open-source agents are mostly a CLI, priced per seat, and each person keeps their own skills</td>
      <td><strong><a href="https://github.com/chinphing/agent-pointer/blob/main/LICENSE">Apache 2.0</a></strong>, free to use and free to customize. <strong>A desktop client and a web app, with the same capabilities.</strong> Skills are managed and developed in one place</td>
    </tr>
    <tr>
      <td rowspan="3">Resource use</td>
      <td>About 300 MB installer download</td>
      <td><strong>About 25 MB</strong></td>
    </tr>
    <tr>
      <td>1 GB+ resident memory</td>
      <td><strong>About 300 MB</strong></td>
    </tr>
    <tr>
      <td>One task can take 100%+ CPU. Extra tasks crowd out other work</td>
      <td><strong>About 5% of one CPU core</strong> while running. <strong>20+ sub-agents at once</strong>. Other tasks are completely unaffected</td>
    </tr>
    <tr>
      <td>Computer use</td>
      <td>Depends on a browser debug protocol or the OS accessibility tree</td>
      <td><strong>Screenshots and the mouse only.</strong> No CDP, and no accessibility tree. <strong>Qwen3.8-flash is enough.</strong> The same task costs about <strong>1/20</strong> of Codex</td>
    </tr>
    <tr>
      <td>Long conversations</td>
      <td>When the context fills up, you start a new chat, and earlier work is easy to lose</td>
      <td><strong>Stay in one conversation for as long as you want.</strong> It pages automatically, compresses context automatically, and you can mark milestones for quick recall</td>
    </tr>
  </tbody>
</table>

<p class="why-pointer-shots">
  <img src="why-pointer/home.png" alt="Pointer home" width="49%" />
  <img src="why-pointer/models.png" alt="Pointer model settings" width="49%" />
</p>

---

*The repository root also holds [LICENSE](../../LICENSE), [SECURITY.md](../../SECURITY.md) and [CHANGELOG.md](../../CHANGELOG.md).*

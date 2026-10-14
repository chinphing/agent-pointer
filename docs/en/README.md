English | [简体中文](../zh-CN/README.md)

## From using it to changing it

| Depth | This step | Go to |
|------|--------|------|
| **Use** | Open the [download page](https://pointer.readflowai.com/download), install, sign in, and send "Hello" | [Getting started](user/getting-started.md) |
| **Learn** | The window, settings, Skills, IM | [User guide](user/README.md) |
| **Understand** | How an official package differs from a local build, and how desktop and web share one core | [Official packages vs. local builds](user/which-build.md), [Architecture](developer/architecture.md) |
| **Change** | Client: `npm run tauri:dev`. Web Server: `npm run server:dev` and `npm run web:dev` in two terminals | [Developer documentation](developer/README.md) |

To build an installer, read [Packaging and deployment](deploy/README.md). Repository conventions are in the [contributor guide](contributing/README.md) and [Development notes](DEVELOPMENT.md). Core implementation is in [Internals](internals/README.md).

## What is this

Build and run digital employees on the computer people already use, and keep them running stably on a server. Both the agent and the model can stay inside your own environment, with no per-seat fees. The desktop app and the web app share the same capabilities, on Windows, macOS, and Linux.

## Why Pointer

A digital employee has to work inside a real office setup. It needs a license that allows commercial use, a small footprint, a long stretch of real use, eyes and hands on the desktop, and a model vendor you choose.

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
      <td>Product form</td>
      <td>Open-source agents are mostly CLI, for developers</td>
      <td><strong>A desktop app &amp; web server</strong>. <strong>Easy to distribute, and easy for everyone in the company to use</strong></td>
    </tr>
    <tr>
      <td>Skill development</td>
      <td>Skills pass in development and fail in production because the environments differ</td>
      <td><strong>One harness</strong>: developing and running Skills stay consistent. <strong>Built for batch Skill testing</strong>: high-concurrency background execution, 20 sub-agents on a single CPU core</td>
    </tr>
    <tr>
      <td>Enterprise features</td>
      <td>Each person sets up their own account and keys; there is no central control</td>
      <td><strong>Unified login, unified API-key management, unified Skill distribution, and conversation-record reporting — and more</strong></td>
    </tr>
    <tr>
      <td>Open source, commercial use</td>
      <td>Desktop apps are usually priced per seat</td>
      <td><strong>Free to use, and free to customize.</strong> <a href="https://github.com/chinphing/agent-pointer/blob/main/LICENSE">Apache 2.0</a></td>
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
      <td>Long conversations</td>
      <td>When the context fills up, you start a new chat, and earlier work is easy to lose</td>
      <td><strong>Stay in one conversation for as long as you want.</strong> It pages automatically, compresses context automatically, and you can mark milestones for quick recall</td>
    </tr>
    <tr>
      <td>Vendors</td>
      <td>Tied to a specific vendor's models, or a specific vendor's ecosystem</td>
      <td><strong>Not tied to a specific vendor's models</strong>, and not tied to a specific vendor's ecosystem</td>
    </tr>
  </tbody>
</table>

A digital employee should not compete with a person for the computer. The memory and CPU figures above are there so it can stay on an everyday work machine. Pointer has already seen more than 3 months of high-intensity use in real business work, nearly 1000M tokens a day.

---

*The repository root also holds [LICENSE](../../LICENSE), [SECURITY.md](../../SECURITY.md) and [CHANGELOG.md](../../CHANGELOG.md).*

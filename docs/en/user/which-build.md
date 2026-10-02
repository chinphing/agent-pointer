# Official packages vs. local builds

English | [简体中文](../../zh-CN/user/which-build.md)

The open-source repository is called **agent-pointer**. Pointer has one source tree and two installation forms.

## Official signed package

Installers downloaded from the [official download page](https://pointer.readflowai.com/download) and signed by the maintainers.

- Optional: sign in to a readflowai.com account (**after signing in the platform delivers model services, so you do not need to configure a Key yourself**)
- Automatic update checks
- After signing in you can report usage, open the cloud host, and go to the top-up page
- The official `pointer-server` package verifies a License when deployed standalone

## Local build

Enterprise internal deployments use the installers published in [GitHub Releases](https://github.com/chinphing/agent-pointer/releases). You can also compile from this repository's source.

- Fill in the model Base URL and API Key yourself in Settings
- Unbound to a control plane by default: no automatic updates and no usage reporting
- A self-hosted `pointer-server` does not need a License from the issuer
- No cloud host entry — that section only appears on the **desktop** and when it is **not a standalone deployment**

## How to choose

| You want | Use this |
| --- | --- |
| Ready to use out of the box | [Official download](https://pointer.readflowai.com/download) |
| Enterprise internal deployment | [GitHub Releases](https://github.com/chinphing/agent-pointer/releases); for the server see [standalone-server.md](../user/standalone-server.md) |

## Automatic updates

The **official signed package** ships an updater; **local builds do not** — this is another hard line between the two installation forms.

| When | Behaviour |
| --- | --- |
| About 30 seconds after launch | One silent check; if there is a new version it downloads in the background |
| Then every 6 hours | Another silent check |
| Download finished | Prompts **Update now / Later / Skip this version** |
| To check manually | "Check for updates" on the "About" page (**managed desktop build only**) |

- Checking and downloading are both **silent and in the background** and do not interrupt your work; you are only prompted when "a new version is ready"
- Choosing "Skip this version" remembers that version number and will not prompt for it again; a newer version will still prompt
- If you check manually and are already on the latest version, it shows "You're up to date"
- **standalone / local builds have no updater**: "Check for updates" does not appear in the UI, and they do not connect to any update service

The update address is provided by the control plane (`POINTER_API_BASE`). A self-hosted build has no update source to connect to, so it should not update in the first place.

For how developers bind a control plane or build a local package with the machine's `pointer.local.env`, see [`../deploy/editions.md`](../deploy/editions.md).

# Official packages vs. local builds

English | [简体中文](../../user/which-build.md)

The open-source repository is called **agent-pointer**. Pointer has one source tree and two installation forms.

## Official signed package

Installers downloaded from the [official download page](https://pointer.readflowai.com/download) and signed by the maintainers.

- Optional: sign in to a readflowai.com account
- Automatic update checks
- After signing in you can report usage, open the cloud host, and go to the top-up page
- The official `pointer-server` package verifies a License when deployed standalone

## Local build

Enterprise internal deployments use the installers published in [GitHub Releases](https://github.com/chinphing/agent-pointer/releases). You can also compile from this repository's source.

- Fill in the model Base URL and API Key yourself in Settings
- Unbound to a control plane by default: no automatic updates and no usage reporting
- A self-hosted `pointer-server` does not need a License from the issuer
- The cloud host page opens, but nothing can be purchased from it while no control plane is bound

## How to choose

| You want | Use this |
| --- | --- |
| Ready to use out of the box | [Official download](https://pointer.readflowai.com/download) |
| Enterprise internal deployment | [GitHub Releases](https://github.com/chinphing/agent-pointer/releases); for the server see [standalone-server.md](../user/standalone-server.md) |

For how developers bind a control plane or build a local package with the machine's `pointer.local.env`, see [`../deploy/editions.md`](../deploy/editions.md).

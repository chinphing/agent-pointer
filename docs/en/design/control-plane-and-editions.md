# Pointer client / server: the account and control-plane rework

English | [简体中文](../../zh-CN/design/control-plane-and-editions.md)

Status: P0 (gating and visibility) has landed; P1 (a bindable control plane) awaits review.  
Repository: agent-pointer. Product name: Pointer.  
The official-site control plane will be open-sourced separately later; this repository only consumes its HTTP API.

This document is a rework design, not a user tutorial.
User-facing copy uses "three ways to use it"; packaging uses "client / server";
`managed` appears only as a packaging flavour in the build instructions (the word "official" only means "released by Pointer", not a value of `POINTER_EDITION`).

> Terminology update: the flavour value has been renamed from `official` to `managed`; with no flavour set the build is standalone. The four-cell build / deployment steps are in [`../deploy/editions.md`](../deploy/editions.md).

## 1. Why the rework

After open-sourcing we must satisfy all of these at once:

- Official-site individual: sign in and go; the local Agent and the cloud host hang off the same account
- Local individual: use your own model Key on your machine without signing in; connect to the official site or a self-hosted control plane when needed
- Company: deploy it yourself with unified sign-in; you may want just one server, or a server plus cloud hosts

Today "you must sign in to a Pointer account" is written into desktop sending and the core gate.
Unbound builds also leave the official-site address empty by default, so the result is: you cannot sign in, and you cannot chat with a local Key alone.
The cloud host code is still there, yet the docs say an unbound build has no purchase entry.

Goal: keep the official-site capabilities in the same tree and distinguish the usages by binding and gating, instead of deleting features per build.

## 2. Outward-facing: three ways to use it

The user answers only two things: **whose account**, and **where the Agent runs**.
Do not present an additive enumeration like "official site × individual × enterprise × local × cloud" to users.

| Usage | Account | What the user installs | Where the Agent runs |
| --- | --- | --- | --- |
| **Use Pointer** | Pointer account | Official client | Their own computer, or a Pointer cloud host |
| **Self-hosted use** | no sign-in | Local client (unbound); add the server only if you want the browser | This machine (or a server you run yourself) |
| **Company deployment** | Company account | The company installs the server; employees use a browser or the client the company ships | Company machines or company cloud hosts |

Ticking a Pointer account under "self-hosted use" is not a fourth product — it just binds to the official-site control plane.
Nor is "company deployment" a third build: the company provides the entry point and the accounts.

The order in which Settings and the docs ask:

```text
Whose account?
  no sign-in → self-hosted use
  Pointer account → Use Pointer
  company account → company deployment

Where does the Agent run?
  this computer / company server → local
  cloud host → cloud
```

## 3. Packaging: client / server

This repository produces only two kinds of artifact. `managed` is the single "bound control plane" flavour, not a third package (with no flavour set the build is standalone).

| Artifact | Command | Responsibility |
| --- | --- | --- |
| **Client** | `npm run tauri:build` | Desktop App. The local Agent lives in this process and does not depend on pointer-server. |
| **Server** | `npm run server:build` | pointer-server + the same Web interface. For browsers or cloud hosts. |

The sign-in site, the shop and billing live in the official-site control-plane repository and are not built here.

| Flavour `POINTER_EDITION` | Client | Server |
| --- | --- | --- |
| `managed` | Official-site domains injected at build time; automatic updates | standalone needs a License |
| unset (standalone) | unbound; no updates | no License needed |

Pick the cells you need at release time; do not talk about "one official package / one local package":

```text
Official client    the default install for "Use Pointer"
Local client       the default install for "self-hosted use" (unbound)
Official server    the official-site cloud-host image; officially issued standalone
Local server       the browser entry point for "self-hosted use"; the default install for "company deployment"
```

Users of "Use Pointer" normally install only the official client, not the server.

## 4. Principles

1. Artifact, flavour, control-plane binding and server authentication mode are four orthogonal questions; do not merge them into one enumeration.
2. The control plane is a runtime binding, not a compile switch. Sign-in, top-up, cloud hosts and balance go through the same code.
3. Sending a message depends on credentials: a local Key and a control-plane session are both valid. The official client may additionally require sign-in.
4. Company cloud hosts go through control-plane OAuth, not standalone. `?sso=` only serves "server only, no shop".
5. Do not add `POINTER_EDITION=enterprise`. Do not add long-lived commercial branches.
6. Do not change the product name, bundle id, crates or data directories.
7. Desktop and Web, and all three desktop systems, share one set of gating and binding rules.
8. Branches and exceptions must log; what should fail must raise an error to the user, not fail silently.

## 5. Four orthogonal concepts

```text
Artifact                 client or server
POINTER_EDITION          the artifact's flavour (`managed` / unset): default domains, client updates, server License
control_plane binding    whether the current process has a usable web_base + api_base
deployment_mode          server only: platform (control-plane OAuth)
                         or standalone (account+password + portal short-lived ticket)
```

The client has no `deployment_mode`.
The client is either local-only or an OAuth client of the control plane.
Do not use a hardcoded `authMode = platform` to mean "this is the desktop".

### 5.1 The flavour only decides defaults and commercial constraints

| | managed | unset (standalone) |
| --- | --- | --- |
| Default control plane | domains injected at build time | empty (unbound) |
| Client auto-update | on | off (standalone build) |
| Usage reporting default | on when bound and not standalone | off |
| Server standalone License | required | not required |
| Sign-in / cloud host / balance / top-up code | kept | kept |

Official domains are injected at build time; the source carries no constants. Nothing is written into the binding when unset.

### 5.2 Control-plane binding

Resolution order (first match wins; an empty string counts as unset):

1. `POINTER_API_BASE` / `POINTER_WEB_BASE` / `VITE_POINTER_WEB_BASE`
2. User settings (P1)
3. Build defaults: `managed` uses the domains injected at build time; unset means empty

Bound: both `api_base` and `web_base` are non-empty.

P1 settings entries (user-facing, no developer jargon):

- Do not use an online account
- Pointer official site (fills in the official constants, the user must choose it explicitly)
- Custom (company or preview environment)

Buying a cloud host, listing and opening them: only callable when bound.
Unbound: the page still opens, explains that you must bind first, logs a warn on the request, and does not pretend to succeed.

### 5.3 How the server identifies people

| `deployment_mode` | Identity comes from | Used for |
| --- | --- | --- |
| `platform` | control-plane OAuth (official site or a company-run official site) | Use Pointer; self-hosted but bound to a control plane; a company that also wants cloud hosts |
| `standalone` | local account+password or the portal's `?sso=` | a company that only wants the server; a fully offline self-hosted Web |

standalone turns off cloud-host OAuth and the shop.
A company that wants both unified sign-in and cloud hosts: deploy the open-sourced control plane, run the server with `platform`, and bind the client to that company's address.

Do not merge the two sign-in paths into one SSO:

```text
Company wants the server only      portal short-lived ticket → standalone server
Company also wants cloud hosts     company IdP → control plane → the existing OAuth
```

## 6. The three usages in implementation terms

| Usage | Default artifact flavour | Control plane | Authentication |
| --- | --- | --- | --- |
| Use Pointer | official client (`managed`) | official site by default | sign in to a Pointer account |
| Self-hosted use | local client (unset) | unbound | no sign-in, local Key |
| Self-hosted use, official site ticked | local client (unset) | user binds the official site | the same sign-in and shop as "Use Pointer" |
| Company wants the server only | local server (unset) | unbound | standalone account+password or short-lived ticket |
| Company also wants cloud hosts | local client + company control plane | bound to the company control plane | unified sign-in on the control plane (after the official site is open-sourced) |

## 7. The send gate

Desktop sending / attachments and the core conversation entry must follow the same rule; changing only one side is forbidden.

```text
has_local_llm         a usable API Key is already in Settings
has_identity          the control plane is signed in, or a standalone local/SSO session exists
control_plane_bound   a control plane is bound
```

| Condition | Result |
| --- | --- |
| Has identity (control-plane session, or standalone local/SSO session) | allow |
| No identity, has a local Key, and no control plane bound (standalone) | allow |
| Control plane bound and no identity | reject: please sign in to your Pointer account first |
| Unbound and no local Key | reject: for self-hosted use fill in a model; for Use Pointer sign in |
| Has a control-plane identity and not standalone | the current balance gate; the top-up page uses the bound web_base |
| Automation trigger | a local Key works; otherwise prompt to sign in or fill in a Key |

Usage reporting and balance requests: only when bound, signed in and not standalone.
An unbound build must not call the official balance API.

## 8. Phases

Each phase can be merged and rolled back on its own. It does not depend on renaming or splitting the repository.

### P0 Gating and visibility

Make "self-hosted use" work on an unbound client; "Use Pointer" behaviour is unchanged.

Changes:

- core conversation entry: unbound with a local Key → allow signed-out use
- Desktop / Web sending and attachments: the same rule; extract it into one shared decision used by both ends, tested separately in core and in the frontend
- Cloud host entry: look at whether a control plane is bound, not at "is this the desktop and not standalone"
- Still calling the shop while unbound: warn + explain in the UI
- User / contributor docs stay consistent with this document: unbound means standalone, not "the cloud host code is gone"

Not doing: entering a control plane in Settings; changing the License; changing the updater.

Acceptance:

- Unbound client, no environment variables set, model Key filled in: can send messages
- Official client, not signed in: cannot send messages
- Unbound client: the cloud host page opens, nothing can be bought, no request goes to the official domains
- Local development with no flavour set: the same as today

### P1 A bindable control plane

Let "self-hosted use" tick a Pointer account or a company address, without editing environment variables or recompiling.

- Settings gains the three online-account entries; they are written into user settings and take part in the resolution in section 5.2
- Once bound, sign-in, top-up and cloud hosts go through the existing platform authentication and shop APIs
- Environment variables still override settings
- The desktop OAuth success page still jumps to the existing agreed query on `{web_base}`

Acceptance:

- After an unbound client picks the Pointer official site, sign-in and cloud-host purchase match the official client (while the official site is online)
- After setting a custom address, requests only go to that domain
- After switching back to "do not use an online account", the shop is unavailable while local-Key chat still works

### P2 Company wants the server only

The standalone account+password and `?sso=` already exist. This phase only closes off expectations:

- Employees open the company server in a browser and complete unified sign-in there
- Do not wire the short-lived ticket into the desktop loopback
- The docs state clearly: if you only want the server, use standalone and do not configure the shop

### P3 Company also wants cloud hosts (depends on the official site being open-sourced)

The custom binding from P1 in this repository is enough.
The IdP, organisations, shop and billing are built in the control-plane repository.

- The company deploys the open-sourced control plane
- The server on the cloud host uses `platform`
- The client binds the company `web_base` / `api_base`
- This repository keeps the existing OAuth ticket exchange and does not invent a third sign-in

## 9. Explicitly not doing

- Do not remove sign-in, cloud hosts or top-up from unbound builds
- Do not hardcode official domains in the source (`managed` flavours are injected at build time; see `docs/en/deploy/editions.md`)
- Do not add a shop to standalone just to fake company cloud hosts
- Do not add long-lived commercial branches
- Do not write file names or developer comments into prompts
- UI copy does not contain developer words such as edition, control plane or deployment_mode

## 10. Cross-entry and cross-platform

Gating and binding resolution are shared by desktop and Web.
Frontend environment variables on the Web only affect display links; real requests follow the server's environment variables and settings.
The OAuth loopback is client-only; the Web keeps redirecting.
Linux / Windows / macOS share one set of settings entries.

## 11. Observability

| Event | Level |
| --- | --- |
| Resolved artifact role, flavour, deployment_mode, whether bound (domains may be logged, keys may not) | info |
| Gate allowed: `local_llm` / `control_plane_session` / `standalone_session` | info |
| Gate rejection reason | warn |
| Calling the shop or balance while unbound | warn |
| Binding failure, ticket exchange failure | warn, and raise an error to the user when necessary |

## 12. Gap to the current state

P0 has landed: `standalone` is derived from "is a control plane bound", and it means the same for client and server; the send gate is unified as
"has identity ∨ (has a local model Key ∧ unbound)"; the Settings entry is open under standalone; the desktop auth mode is no longer hardcoded
to platform; and an unbound build no longer tries to refresh the platform session.

Known leftovers (for a later dedicated pass):

- **An unbound instance still shows the control-plane model catalogue cached from the last binding** (`{data_dir}/platform_model_catalog.json`):
  providers in that catalogue can be used with your own Key, but the UI gives the impression that "there are a dozen models and none of them works".
  The approach is undecided (keep and mark the source / clear it when unbound / a combination of both).
- **The Web side cannot get providers' `apiKey`** (the backend strips keys when sending them down), so the composer gate can only fall back to the merged
  `hasKey` (the default-provider reading), and can still wrongly block when "the default provider has no key while another provider does".
  Fix: have the backend expose the provider resolved per "target + tier" so the frontend gate can reuse it.

## 13. Documents updated alongside each phase

- [../deploy/editions.md](../deploy/editions.md)
- [../contributing/cross-platform-build.md](../contributing/cross-platform-build.md)
- [../user/which-build.md](../user/which-build.md)
- [../user/cloud-host.md](../user/cloud-host.md)
- [../user/standalone-server.md](../user/standalone-server.md)
- [../developer/standalone-deployment.md](../developer/standalone-deployment.md)
- [../developer/architecture.md](../developer/architecture.md)
- [../../developer/desktop-oauth-web-integration.md](../../zh-CN/developer/desktop-oauth-web-integration.md)

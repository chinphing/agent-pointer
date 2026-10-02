# Cloud host

English | [简体中文](../../zh-CN/user/cloud-host.md)

A cloud host is **a complete `pointer-server` the platform runs for you**. You click "Open" in the desktop and it connects in a separate window — as if the server and the whole web build had moved to the cloud, so your machine does not have to stay on.

```
Your desktop                                Platform ECS
┌───────────────────────┐  one-time code   ┌───────────────────────┐
│ Settings → Cloud host │ ───────────────▶ │ pointer-server        │
│ Buy / Renew           │                  │ (full web build)      │
│ Open ─────────────────┼── separate window ─▶│ Chat / attachments    │
└───────────────────────┘                  └───────────────────────┘
```

## When this entry is visible

| Your setup | Is "Cloud host" in Settings? |
| --- | --- |
| **Desktop** + control plane bound (not standalone) | Yes |
| Web (in a browser) | **No** |
| Standalone deployment | **No** |

In other words, this section only appears in the **desktop client**, and only in builds with a control plane bound. Not seeing it does not mean something is broken.

## Prerequisite: sign in to a platform account first

When signed out, the section shows just one hint and one button:

> Sign in to manage cloud hosts　　[ Sign in in browser ]

"Sign in in browser" runs the platform account sign-in flow. Only after signing in are the balance and instance list loaded — while signed out no cloud host request is sent.

## Balance and top-up

After signing in, the top shows "**Balance {amount} CNY**" and a "**Top up**" button; top-up opens the platform's billing page.

When the balance is insufficient the operation fails outright, and the error bar offers a "**Top up**" shortcut. To top up in advance, use the platform billing page.

## Buying one

Click "**Buy cloud host**":

| Field | Description |
| --- | --- |
| **Region** | Which region the instance lands in |
| **Plan** | One of four, see the table below |
| **Purchase duration** | Entered in the plan's unit (hours / months / years), minimum 1 |
| **Amount due** | The price is calculated once the duration is filled in; "Balance {amount} CNY" is shown below |

| Plan | Billing unit | Good for |
| --- | --- | --- |
| **Trial** | CNY / hour | Trying it out, billed by the hour |
| **Standard hourly** | CNY / hour | Short-term use, unsure how long |
| **Monthly plan** | CNY / month | Long-running |
| **Yearly plan** | CNY / year | Long-running, cheaper per unit |

The amount area shows "Calculating…"; when the duration has no valid value it shows "Enter a valid duration to calculate". With a discount, one more line appears: "Saved {amount} CNY ({percent}%)".

Before confirming, check the region, plan and amount — clicking "**Confirm purchase**" **confirms the charge and creates the instance**.

## Instance cards

One card per instance:

| Position | Content |
| --- | --- |
| Title | Region name; "Cloud host" when there is no region info |
| Expiry | "Expires {when}" |
| Status badge | Running / Starting / Releasing / Released / Start failed / Error |
| Health-check error | Shown in red with the reason when present |
| Buttons | See below |

| Button | When it appears | Behaviour |
| --- | --- | --- |
| **Open** | When the instance is truly ready | Opens a separate window and connects (detailed below) |
| **Switch to cloud window** | The window is already open | Brings the cloud window to the front |
| **Close cloud window** | The window is already open | Closes the window (**the instance is unaffected** and keeps running) |
| **Renew** | Status is running | Opens the renew dialog with the current plan type and duration; confirming charges |
| **Release** | Instance not released | Opens the release confirmation |

The instance list can be filtered by status: **Running / Starting / Released / All**.

> **Release is irreversible.** The confirmation is blunt: "The instance will be destroyed; data cannot be recovered." After a successful release the cloud window closes automatically.

## What you get after opening

After clicking "Open":

1. The desktop asks the platform for a **one-time code**
2. It opens a **separate window** with that code (titled "Pointer · {region}") and loads this instance's own address
3. The `pointer-server` on the instance exchanges the code for a session and redirects to its home page

So the cloud window is the **full web build of Pointer** — chat, attachments, skills and view desktop are all there, the same UI as visiting a self-hosted server in a browser.

The cloud window and the main window are separate windows: closing the cloud window does not stop the instance; click "Open" again next time.

## Known limitations

- **No machine tier at purchase**: the plan and duration are selectable, but the machine spec currently takes the platform default
- **Stop-and-keep-data is not implemented**: there is no "stop the instance but keep the data" action yet (marked as a later phase in the developer docs)
- **No main-window tabs**: local and cloud host cannot be switched as tabs in one window; today they are separate windows

## FAQ

**"Sign in to your Pointer platform account first"**

You are signed out. Click "Sign in in browser" in the section, or sign in from the "Account" menu at the bottom left.

**"Balance exhausted" / "Insufficient balance"**

The balance is too low. Click "Top up", or top up on the platform billing page.

**"Platform sign-in has expired; sign in again from Account and retry"**

The session expired. Sign in again from the "Account" menu.

**"The agent is not ready yet; cannot open"**

The instance is still starting (or not ready yet). Wait for the status to become "Running" before clicking "Open".

**"Cloud host window is not open"**

You clicked "Switch to cloud window" for a window that had been closed. Click "Open" again.

**"Console address is invalid"**

The instance address the platform returned is not valid, usually a platform-side issue. Refresh the list and retry; if it still fails, contact the platform administrator.

**"This page cannot call desktop APIs; open the cloud host from the main window or use a browser directly"**

This action must be done in the **main window**. The cloud window loads a remote page and, for security, cannot call desktop APIs — to buy, renew or release, go back to Settings in the main window.

**Clicked "Open" but the window is blank**

First check that the instance status is "Running" and there is no health-check error. A starting instance has no reachable page even if the window opens.

**"Cloud host" does not appear**

Check the table above: neither the web nor a standalone deployment has this section; the desktop needs a control plane bound.

## Related

- [Settings overview](settings.md) — how the Cloud host section is gated, and its neighbouring sections
- [Official packages vs. local builds](which-build.md) — which installation form has the cloud host and automatic updates
- [Usage](settings.md) — Token usage history (under "Settings → Usage")

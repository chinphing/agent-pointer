# Cloud host

English | [简体中文](../../user/cloud-host.md)

Available only with an **officially signed build** and after signing in to the official website account. Builds not bound to a control plane have no purchase entry point.

The desktop client can purchase and open a cloud Pointer instance, using the remote Web UI in a separate window for conversations.

## Before you start

1. Complete the OAuth sign-in under **Settings → Account**
2. Make sure the account has a balance or a bound payment method (the product UI is authoritative)

## Common operations

| Operation | Description |
|------|------|
| **Settings → Cloud host** | View the balance and the instance list |
| **Purchase** | Create a new cloud instance |
| **Open** | Issue an OAuth code and load the remote console in a new WebView window |
| **Switch to cloud window / Close cloud window** | Return to the local main window and continue local conversations |
| **Renew / Release** | Manage the instance lifecycle in the list |

After closing the cloud window, conversations in the local main window are unaffected.

## Extra capabilities on the web side

In a cloud instance's Web UI you can also:

- Download and preview chat attachments
- **View desktop**: a sidebar button captures a screenshot of the cloud host screen

## Deploying a cloud instance yourself

To deploy `pointer-server`, an ALB, environment variables and multi-user isolation on your own ECS, see **[`../../developer/cloud-host-integration.md`](../../developer/cloud-host-integration.md)**.

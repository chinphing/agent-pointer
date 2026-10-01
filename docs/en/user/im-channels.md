# IM channels

English | [简体中文](../../user/im-channels.md)

Connect Feishu, DingTalk, WeCom and WeChat under **Settings → IM channels** to talk to Pointer inside IM.

## Recommended: long connections

| Platform | Mode | Description |
|------|------|------|
| Feishu | WSS long connection | Supports **one-click creation by QR scan** of an app |
| DingTalk | Stream long connection | Supports **one-click creation by QR scan** of a bot |
| WeCom | Bot WSS | Must be configured in the enterprise admin console |
| WeChat | iLink long polling | Configure per the platform's guide |

After enabling a channel, the long connection is established only when you click **Connect** (or automatically when the QR flow finishes); **a public IP is usually not required**.

The configuration is not reconnected automatically after saving; after changing credentials or the enabled state you must click Connect again.

## Webhook mode (fallback)

If a platform requires an HTTP callback:

1. A publicly reachable `pointer-server`, or in development expose the port with ngrok
2. Fill in **publicBaseUrl** in settings
3. Put the generated Webhook URL into the platform's admin console

## Pairing and security

- **dmPolicy**: controls who may send direct messages to the bot (e.g. `open` / `pairing`)
- **pairing**: only user IDs on the pairing list are allowed (exact match)
- When someone requests pairing, the desktop shows an approval prompt; with nothing pending it does not keep polling the API

For per-platform step-by-step configuration, event subscription order, log keywords and troubleshooting see the full guide **[`../../developer/channel-integration.md`](../../developer/channel-integration.md)**.

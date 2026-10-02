# Configuring model services

English | [简体中文](../../zh-CN/user/model-providers.md)

Pointer's code **bundles no model provider**. Where models come from depends on which package you use:

| What you use | Where models come from | Configure it yourself? |
| --- | --- | --- |
| **Official package** (signed in to a platform account) | The platform **issues** providers (Qwen, DeepSeek, …) | **No** — ready to use once you sign in |
| **Local build / self-hosted server** | No platform provisioning | Yes — fill it in yourself |

## Two kinds of provider

Managed in **Settings → Models**:

| Type | Where it comes from | Editable? |
| --- | --- | --- |
| **Platform services** | Issued by the platform after you sign in | **Read-only** — maintained centrally by the platform |
| **Custom services** | Added by you | Add, edit and delete; any OpenAI-compatible endpoint |

Entries marked **Platform default** are the providers currently in effect.

> In a standalone deployment, the "Platform services" slot shows "this instance's model services", with the same parameters and capabilities as custom services.

## Using your own models: add a custom service

**Settings → Models → Custom services → Add service**

| Field | Description |
| --- | --- |
| **Service type** | Determines the default parameters and protocol details (the same family as the built-in Qwen / DeepSeek) |
| **Service ID** | Unique on this machine. A duplicate shows "Service ID already exists; choose another" |
| **Service name** | The name shown in the list; anything you like |
| **API URL** | The OpenAI-compatible base URL. Requests go to `{API URL}/chat/completions` |
| **API Key** | The key the provider gave you |
| **Models** | The model names you can use under this service |

**Service ID, name and API URL are required**; you cannot save if one of them is missing.

After filling them in, click **Test connection** to confirm it works, then save.

## Two layers of parameters: provider defaults + per-model

Pointer's parameters have **two layers**, which differs from many tools:

```
Provider default parameters      ← this layer is the fallback
  └─ Per-model parameters        ← optional; overrides the layer above
```

- **Provider level**: the default parameters and capabilities set while editing the service
- **Model level**: in the model list, click **Configure** on a **single model** to set its parameters and capabilities separately; **Restore default** goes back to "follow provider default"

**Configuring one layer is enough.** Use the model level only when a particular model needs different parameters.

## Model capabilities: what decides where a model appears

**Model capabilities** are checkboxes — **a model only appears in the matching scene's model list once the capability is checked**:

| Capability | Once checked |
| --- | --- |
| Vision | Can read images |
| Audio | Can listen to audio |
| Image generation | Can produce images |
| Video generation | Can produce video |

**If a model is missing from a dropdown, come back and check here first.**

## Thinking effort

Each model can have its own **thinking** level:

```
off → low → medium → high → max
```

A higher level makes the model think longer before answering, which suits hard problems; a lower level is faster for everyday use.

You can also fill in **Extra params (`extra_body`)** — raw JSON passed through to the provider, for the provider's private fields.

## Default and tier mapping

### Default global provider

Click **Set as default** in the list; new chats use it by default from then on.

### Scene tiers

**Settings → Agents → Scene tiers**: each scene (General assistant / Vibe coding / Computer use) **picks its tier independently**; click **Models** to adjust **the model behind each tier**.

A tier marked **Overridden** in the dialog means its model was changed by hand and is not the default. **Changes apply immediately** and sync with **Agents → Scene tiers**.

Web search tier models are configured separately (**Agents → More tools**).

## FAQ

**Sending a message says no Key is configured**

There is no usable model service. Add a custom service as described above, or sign in to a platform account.

**Test connection fails**

- Check the API URL: it must be the **base URL**, without `/chat/completions` (the app appends that itself)
- Check the Key for stray spaces
- Check that the network can reach that address

**A model does not show up in the dropdown**

Go back to **Model capabilities** and confirm the matching capability is checked — unchecked models never appear in scene lists.

**Duplicate service ID**

Pick another ID. The ID is only used locally to tell services apart; it does not affect calls.

**Changed parameters have no effect**

Parameters take effect **after saving**; if you only changed model-level parameters, make sure you saved that model and not the provider.

## Related

- [Getting started](getting-started.md) — your first conversation
- [Official packages vs. local builds](which-build.md) — how the two installation forms differ

# Configuring model services

English | [简体中文](../../zh-CN/user/model-providers.md)

## Custom service

Open **Settings → Models → Custom services → Add**. Pick DeepSeek, Qwen, MiMo, Kimi, or GLM, then enter the API key. The URL and models are already filled in.

An install from the website has to sign in first. After signing in, the platform delivers a model and you can skip this. Your own Key does not replace that sign-in.

## Where the model comes from

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

**Settings → Models → Custom services → Add**

| What you fill in | Description |
| --- | --- |
| **Service** | DeepSeek, Qwen, MiMo, Kimi, or GLM. OpenRouter and Other cover any other endpoint |
| **API Key** | The key from the provider. This is the field you normally change |
| **Advanced** | Service ID, name, API URL, model list, and parameters. Leave these as they are |

The URL is an OpenAI-compatible base URL. Requests go to `{API URL}/chat/completions`. Choosing a service fills in the URL and the model list.

**Service ID, name, and API URL are required.** A preset fills them in. Other leaves them blank, and Advanced opens so you can fill them in. Saving stays disabled until they are set.

## Two layers of parameters: provider defaults + per-model

Pointer's parameters have **two layers**, which differs from many tools:

```
Provider default parameters      ← this layer is the fallback
  └─ Per-model parameters        ← optional; overrides the layer above
```

- **Provider level**: edit the service, open Advanced, and set the default parameters and capabilities
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

Web search tier models are configured separately (**Agents → More tools**). A DeepSeek chat uses DeepSeek web search; a Qwen chat still uses DashScope.

## FAQ

**Sending a message says no Key is configured**

There is no usable model service. Add a custom service as described above, or sign in to a platform account.

**A request fails**

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

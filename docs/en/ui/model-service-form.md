# Custom model service form

English | [简体中文](../../zh-CN/ui/model-service-form.md)

Desktop and web share the add / edit dialog in `ModelServiceSection`.

- The main surface is the service picker and the API key.
- DeepSeek, Qwen, MiMo, Kimi, and GLM preset the API URL and model list. OpenRouter does the same.
- Service ID, name, URL, model list, and runtime parameters sit under Advanced, collapsed by default.
- Other has no preset ID. Opening it expands Advanced, because the ID and name must be typed.
- A preset whose default ID is already in use is hidden from the add list, so it does not collide with an existing service.
- The URL is shown under the key so it can be checked without opening Advanced.

# 自定义模型服务表单

[English](../../en/ui/model-service-form.md) | 简体中文

桌面和 Web 共用 `ModelServiceSection` 里的添加 / 编辑弹窗。

- 主界面只放服务选择和 API Key。
- DeepSeek、Qwen、MiMo、Kimi、GLM 预设 API 地址和模型列表。OpenRouter 同样预设。
- 服务 ID、名称、地址、模型列表、运行参数放在「高级」里，默认收起。
- 「其他」没有预设 ID，打开时展开「高级」，因为 ID 和名称要手填。
- 已占用的预设 ID 不再出现在添加列表里，避免和已有服务撞 ID。
- 地址写在 Key 下面，方便核对，不必先打开「高级」。

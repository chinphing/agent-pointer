# skills 工具组

## load_skill_instructions

用于按需加载指定 Skill 的 `SKILL.md` 正文说明，对应 Skill 渐进式披露的第二层。

使用规则：
- 当已启用 Skill 的 `name` / `description` 表明它与当前任务相关时调用。
- 只传入已启用 Skill 的 `skill_id`。
- 加载正文后，按正文说明继续完成任务。
- 不要反复加载同一个 Skill，除非上下文中缺失其正文内容。

## read_skill_resource

用于按需读取指定 Skill 的资源文件内容，对应 Skill 渐进式披露的第三层。

使用规则：
- 仅当 Skill 正文明确引用 `references/`、`assets/` 或 `scripts/` 下的资源文件，且当前任务确实需要该文件内容时调用。
- `path` 必须使用资源相对路径，例如 `references/api-guide.md`。
- 该工具只读取文件内容，不执行脚本或二进制。
- 不要读取与当前任务无关的资源文件。

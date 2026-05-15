# Coder Agent：Git（`terminal`）使用约定

**对模型的约束**以嵌入提示词为准，勿在别处写冲突规则：

- 主流程与详细说明：`crates/pointer-core/src/agents/coder/AGENT.md` → **Git for history and attribution**
- 会话侧摘要：`crates/pointer-core/src/agents/coder/COMMUNICATION.md` → **Git (via `terminal`)**

## 产品意图（摘要）

- **主要场景**：用户问「**什么时候引入的**」「**谁改的**」「**哪次提交**」等——用只读 git（`blame`、`log`、`show`、`-S`、`--follow`、在支持的 git 上用 `-L`）给出**可复核**的结论（短 hash、subject、作者与日期来自命令输出）。
- **定位**：（**1**）已知文件 — **如何**：文件所在目录为 **`PARENT`**；**工具**：**`terminal`**；**命令**：**`git -C "<PARENT>" rev-parse --show-toplevel`**（失败则向上一级重试）。（**2**）已知 **`ROOT`** — **如何**：本目录 + 一级子目录（含隐藏）+ 子模块；**工具**：**`terminal`**；**命令**：嵌入 **Case 2** 单行。**勿用 `file:glob` 替代 Case 2**：工作区内 **`file:glob`** 在 Linux/macOS/Windows 上行为一致，**只产出文件路径**（不是 shell 的 `*`）；**`.git` 多为目录**；**`file:list`** 默认跳过隐藏。
- **与 `file` 的分工**：当前行为与代码内容用 **`file`**；**时间线与归属**用 git；不要用 git 替代测试或 **`read_lints`**。
- **浅克隆 / 合并 / 重命名**：要在回复里如实说明局限（例如 shallow、merge、rename 导致 blame 易误解）。
- **查阅**：**工具 `terminal`**，**命令 `git <cmd> -h`**。
- **写操作**：仅当用户**明确要求**提交/推送/PR 等；禁止未要求就提交、强推、硬重置等危险操作；`add` 要窄范围，不纳入密钥类文件。

修改上述行为时，请同步更新 **AGENT.md** 与 **COMMUNICATION.md** 中的对应段落，并保持英文表述（嵌入模型上下文）。

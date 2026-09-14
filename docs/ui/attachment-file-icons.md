# 附件芯片图标

非预览类附件用**彩色类型徽标**（XLS / PDF / DOC 等），贴近常见文件管理器观感；不用抽象线框图标。图片 / 音视频仍用缩略图或 kind 图标。

映射入口：`src/lib/attachmentFileIcon.ts`；芯片：`AttachmentFileIcon.vue`。

| 类型 | 示例 | 徽标 |
|------|------|------|
| 表格 | xlsx / csv | 绿底 XLS / CSV |
| 文稿 | docx / doc | 蓝底 DOC |
| 演示 | pptx / ppt | 橙底 PPT |
| PDF | pdf | 红底 PDF |
| 压缩包 | zip / rar / 7z | 琥珀 ZIP 等 |
| 代码 / JSON | ts / py / json | 青底扩展名 |
| 文本 | md / txt | 灰底 MD / TXT |
| 其他 | — | 灰底 FILE 或扩展名 |

无扩展名时回退 MIME。Composer、用户气泡、助手交付画廊共用（App / Web 一致）。

#
# 财务报销专用沙箱镜像（无界面）。
#
# 只装 cwpt-reimburse-submit 技能真正用到的依赖——它的 `.venv` 就是现成清单：
#   pymupdf / pypdfium2   PDF 文本抽取与页面渲染（发票、行程单）
#   pillow                图片准备
#   openai                分类/消歧的模型调用（走技能自己的 API 配置）
#   portalocker           材料登记表、确认清单的跨进程文件锁
#   requests              航旅服务查询
# 其余（httpx / pydantic / anyio / h11 …）由 openai 传递安装。
#
# 不装 lingua-py：技能里是 try/except 软依赖（装不上回落脚本统计兜底），
# 且 PyPI 上 0.1.0 已被 yank，镜像源拿不到——装它反而会构建失败。
#
# 构建：
#   docker build -f docker/sandbox-finance.Dockerfile -t pointer-sandbox-finance:latest .
#
# 使用：
#   POINTER_SERVER_ENVIRONMENT_PROVIDER=light
#   POINTER_SERVER_ENVIRONMENT_BACKEND=docker
#   POINTER_SERVER_ENVIRONMENT_TEMPLATE=pointer-sandbox-finance:latest
#
# 可选层（默认不装，按需打开）：
#   --build-arg WITH_LIBREOFFICE=1  老式 .doc/.xls 转 docx（+约 400MB）
#   --build-arg WITH_OCR=1          扫描件 OCR（paddleocr，+约 800MB）
#
FROM python:3.12-slim

ARG PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple
ARG APT_MIRROR=mirrors.tuna.tsinghua.edu.cn
ARG WITH_LIBREOFFICE=0
ARG WITH_OCR=0

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# ca-certificates：cwpt 是 HTTPS。其余不装——技能不调 curl/unzip/git/node。
# ca-certificates：cwpt 是 HTTPS。
# 解压：register 阶段要解 rar/7z 附件（requirements.txt 的"系统级命令依赖"）。
#   - `unrar` 在 Debian non-free，默认源里没有 → 用 `unrar-free`（提供 unrar 命令，
#     只支持 RAR4；RAR5 交给 7z）
#   - trixie 的 `p7zip-full` 是过渡包，实体是 `7zip`（提供 7z / 7zz）
# apt 走清华源：默认 deb.debian.org 在国内会 502。
RUN sed -i "s|deb.debian.org|${APT_MIRROR}|g" /etc/apt/sources.list.d/debian.sources 2>/dev/null \
 || sed -i "s|deb.debian.org|${APT_MIRROR}|g" /etc/apt/sources.list; \
    apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates \
      unrar-free \
      7zip \
 && rm -rf /var/lib/apt/lists/*

RUN if [ "$WITH_LIBREOFFICE" = "1" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends libreoffice-writer \
      && rm -rf /var/lib/apt/lists/*; \
    fi

# 依赖以技能的 `scripts/requirements.txt` 为准（那份是唯一契约），版本取宿主
# 验证过的组合。`.venv` 不能直接搬进镜像：`bin/python3` 是指向宿主解释器绝对
# 路径的符号链接（`-> /Library/Developer/CommandLineTools/usr/bin/python3`，
# pyvenv.cfg 里也绑着宿主 home），挂进容器就是断链；而且技能根是**只读**挂载，
# `reimburse setup` 在容器里建不了 venv。
#
# PyMuPDF 必须 <1.28：requirements 里注明 1.28 起包内自带 ocr/（自动 Tesseract），
# 文本抽取口径与生产不同；pymupdf4llm 又锁在 0.0.27，二者要一起满足。
RUN pip install --no-cache-dir -i "$PIP_INDEX_URL" \
      "requests==2.32.5" \
      "portalocker==3.2.0" \
      "pymupdf==1.26.5" \
      "pymupdf4llm==0.0.27" \
      "openai==2.48.0" \
      "pillow==11.3.0" \
      "pypdfium2==5.12.1"

RUN if [ "$WITH_OCR" = "1" ]; then \
      pip install --no-cache-dir -i "$PIP_INDEX_URL" \
        "paddlepaddle==3.2.0" "paddleocr==3.7.0" "paddlex==3.7.2"; \
    fi

WORKDIR /workspace
CMD ["sleep", "infinity"]

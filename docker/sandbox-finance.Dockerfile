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
ARG WITH_LIBREOFFICE=0
ARG WITH_OCR=0

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# ca-certificates：cwpt 是 HTTPS。其余不装——技能不调 curl/unzip/git/node。
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN if [ "$WITH_LIBREOFFICE" = "1" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends libreoffice-writer \
      && rm -rf /var/lib/apt/lists/*; \
    fi

# pymupdf 锁 1.x：2.x 会拖进 onnxruntime / numpy / networkx / pymupdf4llm
# 一整套（约 250MB），技能只用到 `fitz.open` 和 `fitz.TOOLS`，用不上。
RUN pip install --no-cache-dir -i "$PIP_INDEX_URL" \
      "pymupdf>=1.26,<2" \
      pypdfium2 \
      pillow \
      openai \
      portalocker \
      requests

RUN if [ "$WITH_OCR" = "1" ]; then \
      pip install --no-cache-dir -i "$PIP_INDEX_URL" paddlepaddle paddleocr; \
    fi

WORKDIR /workspace
CMD ["sleep", "infinity"]

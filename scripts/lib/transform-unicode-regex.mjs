/**
 * Chrome 63 不支持的两类**正则语法**（都是 Chrome 64 才有的），esbuild 不会降级，
 * 在 Chrome 63 上直接 SyntaxError，整个 chunk 不执行：
 *
 * 1. Unicode 属性转义 `\p{L}` / `\P{Nd}` / `\p{P}\p{S}`
 * 2. 命名捕获组 `(?<name>…)` 与反向引用 `\k<name>`
 *
 * 实际影响：`marked`（Markdown 渲染）两种都用，终端用的 `xterm` 也用了 `\p{Control}`。
 * 打不住的话，聊天里的 Markdown 整块失效——不是样式问题，是功能没了。
 *
 * 处理方式：
 * - 正则**字面量**：交给 Babel 的 `transform-unicode-property-regex` 与
 *   `transform-named-capturing-groups-regex`，展开成显式码点范围 / 编号分组。
 * - 字符串里的**片段**（marked 用 `new RegExp("\\p{P}\\p{S}", "u")` 拼正则）：
 *   用同一个 Babel 插件先算出等价字符类，再替换字符串内容。
 *
 * 处理完仍有残留就让**构建失败**——宁可构建红，也不出一个在目标浏览器上白屏的包。
 */

const UNICODE_ESCAPE = /\\[pP]\{/
const NAMED_GROUP = /\(\?<[A-Za-z_$]/
const NAMED_BACKREF = /\\k</

/** 用 Babel 算出 `\p{P}\p{S}` 的等价字符类（不含方括号）。 */
async function expandPunctuationClass(plugins) {
  const { transformAsync } = await import('@babel/core')
  const out = await transformAsync('const probe = /[\\p{P}\\p{S}]/u;', {
    babelrc: false,
    configFile: false,
    compact: true,
    comments: false,
    plugins
  })
  const m = /const probe\s*=\s*\/\[(.*)\]\//.exec(out?.code ?? '')
  if (!m) {
    throw new Error(
      '[unicode-regex] 无法从 Babel 输出中提取字符类，拒绝生成可能白屏的产物'
    )
  }
  return m[1]
}

export function transformUnicodePropertyRegex() {
  return {
    name: 'pointer-transform-unicode-property-regex',
    async renderChunk(code) {
      const needsUnicode = UNICODE_ESCAPE.test(code)
      const needsNamedGroup = NAMED_GROUP.test(code) || NAMED_BACKREF.test(code)
      if (!needsUnicode && !needsNamedGroup) return null

      const [{ transformAsync }, unicodePlugin, namedGroupPlugin] =
        await Promise.all([
          import('@babel/core'),
          import('@babel/plugin-transform-unicode-property-regex').then(
            (m) => m.default
          ),
          import('@babel/plugin-transform-named-capturing-groups-regex').then(
            (m) => m.default
          )
        ])
      const plugins = needsUnicode
        ? [unicodePlugin, namedGroupPlugin]
        : [namedGroupPlugin]

      let out = code
      const babelOut = await transformAsync(out, {
        babelrc: false,
        configFile: false,
        compact: true,
        comments: false,
        sourceMaps: false,
        plugins
      })
      if (babelOut && typeof babelOut.code === 'string') out = babelOut.code

      // 字面量处理完，剩下的只可能是字符串里拼正则的片段。
      if (UNICODE_ESCAPE.test(out)) {
        const cls = await expandPunctuationClass([unicodePlugin])
        out = out.replaceAll('\\\\p{P}\\\\p{S}', cls)
      }

      if (UNICODE_ESCAPE.test(out) || NAMED_GROUP.test(out)) {
        const hit = UNICODE_ESCAPE.test(out) ? '\\p{…}' : '(?<name>…)'
        throw new Error(
          `[unicode-regex] 仍有无法降级的 ${hit} 残留，Chrome 63 会解析失败`
        )
      }
      return { code: out, map: null }
    }
  }
}

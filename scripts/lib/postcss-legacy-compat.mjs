/**
 * Chrome 63/66 CSS compatibility layer.
 *
 * The app targets `chrome >= 63`, but several authoring conveniences only
 * landed much later. Two classes of failure matter:
 *
 *   1. A selector list with ONE unsupported selector invalidates the whole
 *      rule. `:focus-visible` (Chrome 86+) usually shares a list with `:hover`,
 *      so old browsers lose the hover styling too.
 *   2. Unknown declarations are dropped, so logical shorthands such as
 *      `inset-block` (Chrome 87+) silently lose their positioning.
 *
 * This plugin rewrites both into forms every supported browser understands.
 * Modern browsers get the same computed result, so nothing changes there.
 */
const LEGACY_FLAG = 'pointerLegacyCompat'

/** Split a selector list on top-level commas only. */
function splitSelectorList(selector) {
  const out = []
  let depth = 0
  let buf = ''
  for (const ch of selector) {
    if (ch === '(' || ch === '[') depth += 1
    else if (ch === ')' || ch === ']') depth = Math.max(0, depth - 1)
    if (ch === ',' && depth === 0) {
      if (buf.trim()) out.push(buf.trim())
      buf = ''
      continue
    }
    buf += ch
  }
  if (buf.trim()) out.push(buf.trim())
  return out
}

/**
 * Selector features with **no legacy equivalent** — they cannot be rewritten,
 * only isolated. A selector list is invalid as a whole when one entry uses an
 * unknown feature, so these have to be pulled out of the list or they take the
 * whole rule down with them.
 *
 * `:focus-visible` is Chrome 86+, `:has()` is 105+.
 */
const UNEMULATABLE = [':focus-visible', ':has(']

/** Upper bound on how many selectors one rule may expand into. */
const MAX_EXPANDED_SELECTORS = 32

/**
 * Locate the first `:is(` / `:where(` that is **not** nested inside another
 * functional pseudo. Nesting matters: expanding `:not(:is(a, b))` into
 * `:not(a)` / `:not(b)` would turn a conjunction into a disjunction.
 */
function findFunctionalPseudo(selector, names) {
  let depth = 0
  for (let i = 0; i < selector.length; i += 1) {
    const ch = selector[i]
    if (ch === '(' || ch === '[') {
      depth += 1
      continue
    }
    if (ch === ')' || ch === ']') {
      depth = Math.max(0, depth - 1)
      continue
    }
    if (depth !== 0) continue
    for (const name of names) {
      if (!selector.startsWith(name, i)) continue
      let close = i + name.length - 1
      let inner = 0
      for (; close < selector.length; close += 1) {
        if (selector[close] === '(') inner += 1
        else if (selector[close] === ')') {
          inner -= 1
          if (inner === 0) break
        }
      }
      if (close >= selector.length) return null
      return { start: i, end: close + 1, args: selector.slice(i + name.length, close) }
    }
  }
  return null
}

/**
 * `:is(a, b) x` -> `a x, b x`. `:where()` gets the same treatment.
 *
 * Specificity caveat: `:where()` is zero-specificity, so expanding it to plain
 * selectors raises the weight of these rules on every browser. That is a
 * deliberate trade — the alternative is Chrome 63 dropping the rule entirely.
 *
 * Returns `null` when the selector would blow past the budget; the caller then
 * leaves the rule alone instead of emitting something enormous.
 */
function expandIsWhere(selector) {
  const out = []
  const stack = [selector]
  while (stack.length) {
    const current = stack.pop()
    const found = findFunctionalPseudo(current, [':is(', ':where('])
    if (!found) {
      out.push(current)
      continue
    }
    const alternatives = splitSelectorList(found.args)
    if (!alternatives.length) {
      out.push(current)
      continue
    }
    const head = current.slice(0, found.start)
    const tail = current.slice(found.end)
    if (out.length + stack.length + alternatives.length > MAX_EXPANDED_SELECTORS) return null
    // Push in reverse so popping yields the source order.
    for (let i = alternatives.length - 1; i >= 0; i -= 1) {
      stack.push(`${head}${alternatives[i]}${tail}`)
    }
  }
  return out
}

/**
 * The Chrome-63-safe form of a selector, or `null` when it is already safe.
 *
 * Two steps, in this order:
 *   1. drop entries using a feature we cannot emulate (`:has()`, `:focus-visible`)
 *      — but only when other entries survive, otherwise there is nothing to save;
 *   2. expand `:is()` / `:where()` on the survivors.
 */
function legacySelector(selector) {
  const parts = splitSelectorList(selector)
  const clean = parts.filter((part) => !UNEMULATABLE.some((marker) => part.includes(marker)))
  if (!clean.length) return null
  const expanded = []
  for (const part of clean) {
    const variants = expandIsWhere(part)
    if (!variants) return null
    expanded.push(...variants)
  }
  const out = expanded.join(', ')
  return out === selector ? null : out
}

/** `inset: <1..4 values>` -> top/right/bottom/left, per the CSS shorthand rules. */
function insetBox(value) {
  const parts = value.trim().split(/\s+/)
  if (!parts.length || parts.length > 4) return null
  const [a, b = a, c = a, d = b] = parts
  return [
    ['top', a],
    ['right', b],
    ['bottom', c],
    ['left', d]
  ]
}

/**
 * Chrome < 84 has no flex `gap`. Utility classes written straight into a
 * template are covered by the `[class~='gap-N']` rules in globals.css, but
 * component classes receive their layout through `@apply` — the element carries
 * no `gap-*` token, so those rules never match (this is what left `New task`,
 * the project rows and the composer pickers cramped).
 *
 * Derive the fallback from the final CSS instead: any rule that sets both a
 * flex display and a `gap` gets a sibling rule that spaces children with
 * margins. Grid containers are skipped — `gap` already works there in 66.
 */
function emitFlexGapFallback(rule) {
  if (!rule.selector) return
  let display = ''
  let gap = ''
  let direction = ''
  rule.walkDecls((decl) => {
    if (decl.prop === 'display') display = decl.value.trim()
    else if (decl.prop === 'gap') gap = decl.value.trim()
    else if (decl.prop === 'flex-direction') direction = decl.value.trim()
  })
  if (!/^(inline-)?flex$/.test(display) || !gap) return
  const parts = gap.split(/\s+/).filter(Boolean)
  if (!parts.length) return
  const isColumn = direction.startsWith('column')
  // Row flex spaces children horizontally (column-gap); column flex vertically.
  const value = isColumn ? parts[0] : (parts[1] ?? parts[0])
  if (!value || /^0[a-z%]*$/.test(value)) return

  const base = []
  for (const part of splitSelectorList(rule.selector)) {
    // A `:has()` / `:focus-visible` entry cannot be emulated; a margin fallback
    // built on it would be invalid in the very browsers this targets.
    if (UNEMULATABLE.some((marker) => part.includes(marker))) continue
    const variants = expandIsWhere(part)
    if (!variants) return
    base.push(...variants)
  }
  if (!base.length) return
  const prop = isColumn ? 'margin-top' : 'margin-left'
  const iconProp = isColumn ? 'margin-bottom' : 'margin-right'

  // Three rules instead of one: an icon followed by a *bare text node* is not
  // reachable by `> * + *` (text nodes are not elements), so icons also get a
  // trailing margin. The sibling rule then skips icon-adjacent nodes so nothing
  // is spaced twice.
  const variants = [
    { selector: base.map((s) => `html.no-flex-gap ${s} > svg`).join(', '), prop: iconProp },
    {
      selector: base.map((s) => `html.no-flex-gap ${s} > :not(svg) + *:not(svg)`).join(', '),
      prop
    },
    { selector: base.map((s) => `html.no-flex-gap ${s} > * + svg`).join(', '), prop }
  ]

  let anchor = rule
  for (const variant of variants) {
    const fallback = rule.clone({ selector: variant.selector, nodes: [] })
    fallback.raws[LEGACY_FLAG] = true
    fallback.append({ prop: variant.prop, value })
    rule.parent.insertAfter(anchor, fallback)
    anchor = fallback
  }
}

export default function legacyCssCompat() {
  return {
    postcssPlugin: 'pointer-legacy-css-compat',

    Rule(rule) {
      if (rule.raws[LEGACY_FLAG]) return
      if (rule.selector) {
        const legacy = legacySelector(rule.selector)
        if (legacy) {
          const clone = rule.clone({ selector: legacy })
          clone.raws[LEGACY_FLAG] = true
          rule.parent.insertBefore(rule, clone)
        }
      }
      emitFlexGapFallback(rule)
    },

    Declaration(decl) {
      if (decl.raws[LEGACY_FLAG]) return
      let pairs = null
      switch (decl.prop) {
        case 'inset':
          // Skip values with functions: splitting `calc()` on spaces is wrong.
          if (decl.value.includes('(')) return
          pairs = insetBox(decl.value)
          break
        case 'inset-block':
          pairs = [['top', decl.value], ['bottom', decl.value]]
          break
        case 'inset-inline':
          pairs = [['left', decl.value], ['right', decl.value]]
          break
        case 'inset-block-start':
          pairs = [['top', decl.value]]
          break
        case 'inset-block-end':
          pairs = [['bottom', decl.value]]
          break
        case 'inset-inline-start':
          pairs = [['left', decl.value]]
          break
        case 'inset-inline-end':
          pairs = [['right', decl.value]]
          break
        default:
          return
      }
      if (!pairs) return
      for (const [prop, value] of pairs) {
        const clone = decl.clone({ prop, value })
        clone.raws[LEGACY_FLAG] = true
        decl.parent.insertBefore(decl, clone)
      }
    }
  }
}

legacyCssCompat.postcss = true

// Minimal evaluator for the rule-dsl §3.1 formula language, used ONLY by the mock API so the formula playground works
// offline. The authoritative implementation is the Rust `rule-engine` crate.
// Supports: optional `F(x,y) =` header, + - * / % ^ (right-assoc), unary minus, implicit multiplication (2x, 3(x+1),
// (a)(b)), functions and constants from the spec.

export class FormulaError extends Error {
  constructor(message: string, public position: number) {
    super(message)
  }
}

type Tok = { t: 'num', v: number, p: number } | { t: 'id', v: string, p: number } | { t: 'op', v: string, p: number }

function tokenize(src: string, offset: number): Tok[] {
  const out: Tok[] = []
  let i = 0
  while (i < src.length) {
    const c = src[i]!
    if (/\s/.test(c)) { i++; continue }
    const num = /^(\d+\.?\d*|\.\d+)(e[+-]?\d+)?/i.exec(src.slice(i))
    if (num) { out.push({ t: 'num', v: Number(num[0]), p: offset + i }); i += num[0].length; continue }
    const id = /^[A-Za-z_]\w*/.exec(src.slice(i))
    if (id) { out.push({ t: 'id', v: id[0], p: offset + i }); i += id[0].length; continue }
    if ('+-*/%^(),'.includes(c)) { out.push({ t: 'op', v: c, p: offset + i }); i++; continue }
    throw new FormulaError(`unexpected character '${c}'`, offset + i)
  }
  return out
}

const FUNCS: Record<string, [number, number, (...a: number[]) => number]> = {
  abs: [1, 1, Math.abs], sqrt: [1, 1, Math.sqrt], ln: [1, 1, Math.log], log10: [1, 1, Math.log10],
  log: [2, 2, (x, b) => Math.log(x) / Math.log(b)], exp: [1, 1, Math.exp], pow: [2, 2, Math.pow],
  min: [1, 99, Math.min], max: [1, 99, Math.max], floor: [1, 1, Math.floor], ceil: [1, 1, Math.ceil],
  round: [1, 2, (x, d = 0) => Math.round(x * 10 ** d) / 10 ** d], clamp: [3, 3, (x, lo, hi) => Math.min(hi, Math.max(lo, x))],
  sigmoid: [1, 1, x => 1 / (1 + Math.exp(-x))],
  gauss: [3, 3, (x, mu, s) => Math.exp(-0.5 * ((x - mu) / s) ** 2) / (s * Math.sqrt(2 * Math.PI))],
  normcdf: [3, 3, (x, mu, s) => 0.5 * (1 + erf((x - mu) / (s * Math.SQRT2)))],
  if: [3, 3, (c, a, b) => (c !== 0 ? a : b)],
}
const CONSTS: Record<string, number> = { pi: Math.PI, e: Math.E }

function erf(x: number): number {
  const t = 1 / (1 + 0.3275911 * Math.abs(x))
  const y = 1 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * Math.exp(-x * x)
  return x >= 0 ? y : -y
}

export function evaluateFormula(expr: string, variables: Record<string, unknown>): number {
  let body = expr
  let offset = 0
  const header = /^\s*([A-Za-z_]\w*)\s*\(([^)]*)\)\s*=/.exec(expr)
  let params: string[] | null = null
  if (header) {
    params = header[2]!.split(',').map(s => s.trim()).filter(Boolean)
    offset = header[0].length
    body = expr.slice(offset)
  }
  const toks = tokenize(body, offset)
  let pos = 0
  const peek = () => toks[pos]
  const next = () => toks[pos++]
  const end = expr.length

  const value = (name: string, p: number): number => {
    if (name in variables) {
      const v = variables[name]
      if (v === null || v === undefined) throw new FormulaError(`variable ${name} is null`, p)
      const n = typeof v === 'boolean' ? (v ? 1 : 0) : Number(v)
      if (Number.isNaN(n)) throw new FormulaError(`variable ${name} is not numeric`, p)
      return n
    }
    if (name in CONSTS) return CONSTS[name]!
    throw new FormulaError(`unbound variable ${name}`, p)
  }

  const startsPrimary = (t: Tok | undefined) => !!t && (t.t === 'num' || t.t === 'id' || (t.t === 'op' && t.v === '('))

  function expression(): number {
    let v = term()
    for (let t = peek(); t && t.t === 'op' && (t.v === '+' || t.v === '-'); t = peek()) {
      next()
      v = t.v === '+' ? v + term() : v - term()
    }
    return v
  }
  function term(): number {
    let v = unary()
    for (;;) {
      const t = peek()
      if (t && t.t === 'op' && (t.v === '*' || t.v === '/' || t.v === '%')) {
        next()
        const r = unary()
        if ((t.v === '/' || t.v === '%') && r === 0) throw new FormulaError('division by zero', t.p)
        v = t.v === '*' ? v * r : t.v === '/' ? v / r : v % r
      }
      else if (startsPrimary(t) && toks[pos - 1] && (toks[pos - 1]!.t === 'num' || (toks[pos - 1]!.t === 'op' && toks[pos - 1]!.v === ')'))) {
        v *= unary() // implicit multiplication
      }
      else return v
    }
  }
  function unary(): number {
    const t = peek()
    if (t && t.t === 'op' && t.v === '-') { next(); return -unary() }
    return power()
  }
  function power(): number {
    const base = primary()
    const t = peek()
    if (t && t.t === 'op' && t.v === '^') { next(); return base ** unary() }
    return base
  }
  function primary(): number {
    const t = next()
    if (!t) throw new FormulaError('unexpected end of formula', end)
    if (t.t === 'num') return t.v
    if (t.t === 'id') {
      const la = peek()
      if (la && la.t === 'op' && la.v === '(') {
        const fn = FUNCS[t.v]
        if (!fn) throw new FormulaError(`unknown function ${t.v}`, t.p)
        next()
        const args: number[] = []
        if (!(peek()?.t === 'op' && peek()!.v === ')')) {
          args.push(expression())
          while (peek()?.t === 'op' && peek()!.v === ',') { next(); args.push(expression()) }
        }
        const close = next()
        if (!close || close.t !== 'op' || close.v !== ')') throw new FormulaError('expected )', close?.p ?? end)
        if (args.length < fn[0] || args.length > fn[1]) throw new FormulaError(`${t.v} expects ${fn[0]}${fn[1] !== fn[0] ? `..${fn[1]}` : ''} arguments`, t.p)
        return fn[2](...args)
      }
      if (params && !params.includes(t.v) && !(t.v in CONSTS)) throw new FormulaError(`${t.v} is not a declared parameter`, t.p)
      return value(t.v, t.p)
    }
    if (t.v === '(') {
      const v = expression()
      const close = next()
      if (!close || close.t !== 'op' || close.v !== ')') throw new FormulaError('expected )', close?.p ?? end)
      return v
    }
    throw new FormulaError(`unexpected '${t.v}'`, t.p)
  }

  const result = expression()
  if (pos < toks.length) throw new FormulaError(`unexpected '${String(toks[pos]!.v)}'`, toks[pos]!.p)
  if (!Number.isFinite(result)) throw new FormulaError('non-finite result', end)
  return result
}

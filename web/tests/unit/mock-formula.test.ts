import { describe, expect, it } from 'vitest'
import { evaluateFormula, FormulaError } from '../../server/mock/formula'

describe('mock formula evaluator (rule-dsl §3.1)', () => {
  it('evaluates the spec example F(x,y,z) = 2x + 2^y / z^2', () => {
    expect(evaluateFormula('F(x,y,z) = 2x + 2^y / z^2', { x: 10, y: 3, z: 2 })).toBe(22)
  })

  it('supports implicit multiplication, right-assoc power and unary minus precedence', () => {
    expect(evaluateFormula('3(x+1)', { x: 1 })).toBe(6)
    expect(evaluateFormula('(a+b)(a-b)', { a: 3, b: 1 })).toBe(8)
    expect(evaluateFormula('2^3^2', {})).toBe(512)
    expect(evaluateFormula('-x^2', { x: 3 })).toBe(-9)
    expect(evaluateFormula('max(1, x, 3) + if(0, 10, 20)', { x: 7 })).toBe(27)
  })

  it('traps on division by zero and null arguments', () => {
    expect(() => evaluateFormula('x / y', { x: 1, y: 0 })).toThrowError(/division by zero/)
    expect(() => evaluateFormula('x + 1', { x: null })).toThrowError(/null/)
  })

  it('reports parse errors with a position', () => {
    try {
      evaluateFormula('F(x) = x + foo(1)', { x: 1 })
      expect.unreachable()
    }
    catch (err) {
      expect(err).toBeInstanceOf(FormulaError)
      expect((err as FormulaError).position).toBe(11)
    }
    expect(() => evaluateFormula('F(x) = x + y', { x: 1, y: 2 })).toThrowError(/not a declared parameter/)
  })
})

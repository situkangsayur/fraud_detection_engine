//! The formula language (rule-dsl §3.1): `F(x,y,z) = 2x + 2^y / z^2`.
//!
//! Pipeline: **lexer** (text → tokens with byte positions) → **recursive-descent parser** (tokens → [`Expr`]
//! AST) → **evaluator** (AST + variable bindings → `f64` or [`EvalError`]). A formula is parsed once and cached
//! ([`crate::cache::formula`]), then evaluated many times.
//!
//! Grammar (precedence low → high):
//! ```text
//! formula := [ header "=" ] expr          header := IDENT "(" IDENT { "," IDENT } ")"
//! expr    := term { ("+" | "-") term }
//! term    := unary { ("*" | "/" | "%") unary | <implicit> unary }
//! unary   := ("-" | "+") unary | power     -- so -x^2 = -(x^2)
//! power   := primary [ "^" unary ]          -- right-associative: 2^3^2 = 2^(3^2)
//! primary := NUMBER | IDENT | IDENT "(" args ")" | "(" expr ")"
//! ```
//! Implicit multiplication applies when a NUMBER or `)` is followed by an IDENT or `(`: `2x`, `3(x+1)`,
//! `(a+b)(a-b)`, `abs(x)y`. Adjacent identifiers are never split (`xy` is one identifier).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use statrs::distribution::{Continuous, ContinuousCDF, Normal};
use thiserror::Error;

/// A parse error with the byte position of the offending token.
#[derive(Debug, Clone, PartialEq, Eq, Error, serde::Serialize)]
#[error("{message} (at position {position})")]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl ParseError {
    fn new(message: impl Into<String>, position: usize) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }
}

/// Why a formula could not produce a finite number (becomes a `trapped` rule outcome).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvalError {
    #[error("unbound_variable: {0}")]
    UnboundVariable(String),
    #[error("division_by_zero")]
    DivisionByZero,
    #[error("non_finite_result: {0}")]
    NonFinite(String),
    #[error("domain_error: {0}")]
    Domain(String),
}

// ---------------------------------------------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------------------------------------------

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

impl BinOp {
    fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Pow => "^",
        }
    }
}

/// Built-in functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Abs,
    Sqrt,
    Ln,
    Log10,
    Log,
    Exp,
    Pow,
    Min,
    Max,
    Floor,
    Ceil,
    Round,
    Clamp,
    Sigmoid,
    Gauss,
    Normcdf,
    If,
}

impl Func {
    /// All functions with their names.
    pub const ALL: [Func; 17] = [
        Func::Abs,
        Func::Sqrt,
        Func::Ln,
        Func::Log10,
        Func::Log,
        Func::Exp,
        Func::Pow,
        Func::Min,
        Func::Max,
        Func::Floor,
        Func::Ceil,
        Func::Round,
        Func::Clamp,
        Func::Sigmoid,
        Func::Gauss,
        Func::Normcdf,
        Func::If,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Func::Abs => "abs",
            Func::Sqrt => "sqrt",
            Func::Ln => "ln",
            Func::Log10 => "log10",
            Func::Log => "log",
            Func::Exp => "exp",
            Func::Pow => "pow",
            Func::Min => "min",
            Func::Max => "max",
            Func::Floor => "floor",
            Func::Ceil => "ceil",
            Func::Round => "round",
            Func::Clamp => "clamp",
            Func::Sigmoid => "sigmoid",
            Func::Gauss => "gauss",
            Func::Normcdf => "normcdf",
            Func::If => "if",
        }
    }

    pub fn from_name(name: &str) -> Option<Func> {
        Func::ALL.iter().copied().find(|f| f.name() == name)
    }

    /// Allowed argument count `(min, max)`; `max = None` means variadic.
    pub fn arity(self) -> (usize, Option<usize>) {
        match self {
            Func::Abs
            | Func::Sqrt
            | Func::Ln
            | Func::Log10
            | Func::Exp
            | Func::Floor
            | Func::Ceil
            | Func::Sigmoid => (1, Some(1)),
            Func::Log | Func::Pow => (2, Some(2)),
            Func::Round => (1, Some(2)),
            Func::Min | Func::Max => (1, None),
            Func::Clamp | Func::Gauss | Func::Normcdf | Func::If => (3, Some(3)),
        }
    }
}

/// Built-in constants.
pub const CONSTANTS: [(&str, f64); 2] = [("pi", std::f64::consts::PI), ("e", std::f64::consts::E)];

fn constant(name: &str) -> Option<f64> {
    CONSTANTS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// Formula abstract syntax tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

impl fmt::Display for Expr {
    /// Fully parenthesised rendering; re-parsing it yields the same AST.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Num(n) => write!(f, "{n:?}"),
            Expr::Var(name) => f.write_str(name),
            Expr::Neg(inner) => write!(f, "(-{inner})"),
            Expr::Bin(op, a, b) => write!(f, "({a} {} {b})", op.symbol()),
            Expr::Call(func, args) => {
                write!(f, "{}(", func.name())?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
    Eq,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    pos: usize,
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Num(n) => format!("number {n}"),
        Tok::Ident(s) => format!("identifier '{s}'"),
        Tok::Plus => "'+'".into(),
        Tok::Minus => "'-'".into(),
        Tok::Star => "'*'".into(),
        Tok::Slash => "'/'".into(),
        Tok::Percent => "'%'".into(),
        Tok::Caret => "'^'".into(),
        Tok::LParen => "'('".into(),
        Tok::RParen => "')'".into(),
        Tok::Comma => "','".into(),
        Tok::Eq => "'='".into(),
    }
}

fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let tok = match c {
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'^' => Tok::Caret,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b',' => Tok::Comma,
            b'=' => Tok::Eq,
            b'0'..=b'9' | b'.' => {
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                // Exponent only when followed by digits: `2e-3` is a number, `2e` is 2*e, `2ex` is 2*ex.
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    let mut j = i + 1;
                    if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j].is_ascii_digit() {
                        while j < bytes.len() && bytes[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    }
                }
                let text = &src[start..i];
                let n: f64 = text
                    .parse()
                    .map_err(|_| ParseError::new(format!("invalid number '{text}'"), start))?;
                out.push(Token {
                    tok: Tok::Num(n),
                    pos: start,
                });
                continue;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Token {
                    tok: Tok::Ident(src[start..i].to_string()),
                    pos: start,
                });
                continue;
            }
            _ => {
                let ch = src[start..].chars().next().unwrap_or('?');
                return Err(ParseError::new(format!("unexpected character '{ch}'"), start));
            }
        };
        out.push(Token { tok, pos: start });
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------------------------------------------

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
    end: usize,
    var_positions: Vec<(String, usize)>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    fn position(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.end, |t| t.pos)
    }

    fn prev(&self) -> Option<&Tok> {
        self.pos
            .checked_sub(1)
            .and_then(|p| self.tokens.get(p))
            .map(|t| &t.tok)
    }

    fn advance(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    fn expect(&mut self, want: &Tok, what: &str) -> Result<(), ParseError> {
        match self.peek() {
            Some(t) if t == want => {
                self.pos += 1;
                Ok(())
            }
            Some(t) => Err(ParseError::new(
                format!("expected {what}, found {}", describe(t)),
                self.position(),
            )),
            None => Err(ParseError::new(
                format!("expected {what}, found end of formula"),
                self.position(),
            )),
        }
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.term()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                Some(Tok::Percent) => BinOp::Mod,
                Some(Tok::Ident(_)) | Some(Tok::LParen)
                    if matches!(self.prev(), Some(Tok::Num(_)) | Some(Tok::RParen)) =>
                {
                    // implicit multiplication: 2x, 3(x+1), (a)(b)
                    let rhs = self.unary()?;
                    lhs = Expr::Bin(BinOp::Mul, Box::new(lhs), Box::new(rhs));
                    continue;
                }
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.unary()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Plus) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let base = self.primary()?;
        if matches!(self.peek(), Some(Tok::Caret)) {
            self.pos += 1;
            let exponent = self.unary()?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let position = self.position();
        let Some(token) = self.advance().cloned() else {
            return Err(ParseError::new("unexpected end of formula", position));
        };
        match token.tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::LParen => {
                let inner = self.expr()?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(inner)
            }
            Tok::Ident(name) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    let func = Func::from_name(&name)
                        .ok_or_else(|| ParseError::new(format!("unknown function '{name}'"), token.pos))?;
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.expr()?);
                            if matches!(self.peek(), Some(Tok::Comma)) {
                                self.pos += 1;
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(&Tok::RParen, "')' or ','")?;
                    let (min, max) = func.arity();
                    let ok = args.len() >= min && max.is_none_or(|m| args.len() <= m);
                    if !ok {
                        let expected = match max {
                            Some(m) if m == min => format!("{min}"),
                            Some(m) => format!("{min} to {m}"),
                            None => format!("at least {min}"),
                        };
                        return Err(ParseError::new(
                            format!(
                                "function '{name}' expects {expected} argument(s), got {}",
                                args.len()
                            ),
                            token.pos,
                        ));
                    }
                    Ok(Expr::Call(func, args))
                } else {
                    self.var_positions.push((name.clone(), token.pos));
                    Ok(Expr::Var(name))
                }
            }
            other => Err(ParseError::new(
                format!("unexpected {}", describe(&other)),
                token.pos,
            )),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Formula
// ---------------------------------------------------------------------------------------------------------------

/// A parsed formula.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    source: String,
    name: Option<String>,
    params: Option<Vec<String>>,
    expr: Expr,
    var_positions: Vec<(String, usize)>,
}

impl Formula {
    /// Parses `F(x,y) = expr` or a bare `expr`.
    pub fn parse(source: &str) -> Result<Formula, ParseError> {
        let tokens = lex(source)?;
        let eq_positions: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| t.tok == Tok::Eq)
            .map(|(i, _)| i)
            .collect();
        let (name, params, body) = match eq_positions.as_slice() {
            [] => (None, None, &tokens[..]),
            [eq] => {
                let (name, params) = parse_header(&tokens[..*eq], tokens[*eq].pos)?;
                (Some(name), Some(params), &tokens[eq + 1..])
            }
            [_, second, ..] => {
                return Err(ParseError::new(
                    "only one '=' is allowed (after the header)",
                    tokens[*second].pos,
                ))
            }
        };
        if body.is_empty() {
            return Err(ParseError::new("empty formula", source.len()));
        }
        let mut parser = Parser {
            tokens: body,
            pos: 0,
            end: source.len(),
            var_positions: Vec::new(),
        };
        let expr = parser.expr()?;
        if let Some(extra) = body.get(parser.pos) {
            return Err(ParseError::new(
                format!("unexpected {}", describe(&extra.tok)),
                extra.pos,
            ));
        }
        let formula = Formula {
            source: source.to_string(),
            name,
            params,
            expr,
            var_positions: parser.var_positions,
        };
        if let Some(params) = &formula.params {
            for (var, pos) in &formula.var_positions {
                if !params.contains(var) && constant(var).is_none() {
                    return Err(ParseError::new(
                        format!("variable '{var}' is not declared in the header"),
                        *pos,
                    ));
                }
            }
        }
        Ok(formula)
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn expr(&self) -> &Expr {
        &self.expr
    }

    /// Header name (`F` in `F(x) = …`).
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Header parameters, if a header is present.
    pub fn params(&self) -> Option<&[String]> {
        self.params.as_deref()
    }

    /// Identifiers used as variables in the body (constants excluded unless declared as parameters).
    pub fn free_variables(&self) -> BTreeSet<String> {
        self.var_positions
            .iter()
            .filter(|(v, _)| constant(v).is_none() || self.params.as_ref().is_some_and(|p| p.contains(v)))
            .map(|(v, _)| v.clone())
            .collect()
    }

    /// Checks that the provided argument names bind the formula exactly (rule-dsl §3.1): with a header, the
    /// parameter list must equal the argument set; without, every free identifier must be bound.
    pub fn check_bindings(&self, arg_names: &BTreeSet<String>) -> Result<(), ParseError> {
        match &self.params {
            Some(params) => {
                let declared: BTreeSet<String> = params.iter().cloned().collect();
                if &declared != arg_names {
                    let missing: Vec<_> = declared.difference(arg_names).cloned().collect();
                    let extra: Vec<_> = arg_names.difference(&declared).cloned().collect();
                    return Err(ParseError::new(
                        format!(
                            "header parameters must equal args: missing args [{}], unexpected args [{}]",
                            missing.join(", "),
                            extra.join(", ")
                        ),
                        0,
                    ));
                }
                Ok(())
            }
            None => {
                for (var, pos) in &self.var_positions {
                    if !arg_names.contains(var) && constant(var).is_none() {
                        return Err(ParseError::new(
                            format!("variable '{var}' is not bound in args"),
                            *pos,
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    /// Evaluates with the given variable values. Bound variables shadow constants.
    pub fn eval(&self, vars: &BTreeMap<String, f64>) -> Result<f64, EvalError> {
        let value = eval_expr(&self.expr, vars)?;
        finite(value, "result")
    }
}

fn parse_header(tokens: &[Token], eq_pos: usize) -> Result<(String, Vec<String>), ParseError> {
    let err = |pos: usize| ParseError::new("invalid header: expected NAME(param, ...) before '='", pos);
    let mut iter = tokens.iter();
    let name = match iter.next() {
        Some(Token {
            tok: Tok::Ident(n), ..
        }) => n.clone(),
        Some(t) => return Err(err(t.pos)),
        None => return Err(err(eq_pos)),
    };
    match iter.next() {
        Some(Token { tok: Tok::LParen, .. }) => {}
        Some(t) => return Err(err(t.pos)),
        None => return Err(err(eq_pos)),
    }
    let mut params: Vec<String> = Vec::new();
    let mut expect_ident = true;
    loop {
        match iter.next() {
            Some(Token {
                tok: Tok::Ident(p),
                pos,
            }) if expect_ident => {
                if params.contains(p) {
                    return Err(ParseError::new(format!("duplicate parameter '{p}'"), *pos));
                }
                if Func::from_name(p).is_some() {
                    return Err(ParseError::new(
                        format!("parameter '{p}' shadows a function name"),
                        *pos,
                    ));
                }
                params.push(p.clone());
                expect_ident = false;
            }
            Some(Token { tok: Tok::Comma, .. }) if !expect_ident => expect_ident = true,
            Some(Token { tok: Tok::RParen, .. }) if !expect_ident || params.is_empty() => break,
            Some(t) => return Err(err(t.pos)),
            None => return Err(err(eq_pos)),
        }
    }
    if let Some(t) = iter.next() {
        return Err(err(t.pos));
    }
    Ok((name, params))
}

fn finite(value: f64, what: &str) -> Result<f64, EvalError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(EvalError::NonFinite(what.to_string()))
    }
}

fn eval_expr(expr: &Expr, vars: &BTreeMap<String, f64>) -> Result<f64, EvalError> {
    match expr {
        Expr::Num(n) => Ok(*n),
        Expr::Var(name) => vars
            .get(name)
            .copied()
            .or_else(|| constant(name))
            .ok_or_else(|| EvalError::UnboundVariable(name.clone())),
        Expr::Neg(inner) => Ok(-eval_expr(inner, vars)?),
        Expr::Bin(op, a, b) => {
            let x = eval_expr(a, vars)?;
            let y = eval_expr(b, vars)?;
            let r = match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => {
                    if y == 0.0 {
                        return Err(EvalError::DivisionByZero);
                    }
                    x / y
                }
                BinOp::Mod => {
                    if y == 0.0 {
                        return Err(EvalError::DivisionByZero);
                    }
                    x % y
                }
                BinOp::Pow => x.powf(y),
            };
            finite(r, op.symbol())
        }
        Expr::Call(func, args) => eval_call(*func, args, vars),
    }
}

fn eval_call(func: Func, args: &[Expr], vars: &BTreeMap<String, f64>) -> Result<f64, EvalError> {
    // Arguments are evaluated on demand so that `if(z, 1/z, 0)` does not trap when z = 0.
    let arg = |i: usize| -> Result<f64, EvalError> {
        let expr = args
            .get(i)
            .ok_or_else(|| EvalError::Domain(format!("{} is missing argument {}", func.name(), i + 1)))?;
        eval_expr(expr, vars)
    };
    let all = || {
        args.iter()
            .map(|a| eval_expr(a, vars))
            .collect::<Result<Vec<f64>, _>>()
    };
    let r = match func {
        Func::If => return if arg(0)? != 0.0 { arg(1) } else { arg(2) },
        Func::Abs => arg(0)?.abs(),
        Func::Sqrt => {
            let x = arg(0)?;
            if x < 0.0 {
                return Err(EvalError::Domain(format!("sqrt of negative number {x}")));
            }
            x.sqrt()
        }
        Func::Ln => {
            let x = arg(0)?;
            if x <= 0.0 {
                return Err(EvalError::Domain(format!("ln of non-positive number {x}")));
            }
            x.ln()
        }
        Func::Log10 => {
            let x = arg(0)?;
            if x <= 0.0 {
                return Err(EvalError::Domain(format!("log10 of non-positive number {x}")));
            }
            x.log10()
        }
        Func::Log => {
            let (x, base) = (arg(0)?, arg(1)?);
            if x <= 0.0 || base <= 0.0 || base == 1.0 {
                return Err(EvalError::Domain(format!("log({x}, {base}) is undefined")));
            }
            x.ln() / base.ln()
        }
        Func::Exp => arg(0)?.exp(),
        Func::Pow => arg(0)?.powf(arg(1)?),
        Func::Min => all()?.into_iter().fold(f64::INFINITY, f64::min),
        Func::Max => all()?.into_iter().fold(f64::NEG_INFINITY, f64::max),
        Func::Floor => arg(0)?.floor(),
        Func::Ceil => arg(0)?.ceil(),
        Func::Round => {
            let x = arg(0)?;
            let digits = if args.len() > 1 { arg(1)? } else { 0.0 };
            if digits.fract() != 0.0 || !(0.0..=15.0).contains(&digits) {
                return Err(EvalError::Domain(format!(
                    "round digits must be an integer 0..15, got {digits}"
                )));
            }
            let factor = 10f64.powi(digits as i32);
            (x * factor).round() / factor
        }
        Func::Clamp => {
            let (x, lo, hi) = (arg(0)?, arg(1)?, arg(2)?);
            if lo > hi {
                return Err(EvalError::Domain(format!(
                    "clamp lower bound {lo} > upper bound {hi}"
                )));
            }
            x.clamp(lo, hi)
        }
        Func::Sigmoid => 1.0 / (1.0 + (-arg(0)?).exp()),
        Func::Gauss => normal(arg(1)?, arg(2)?)?.pdf(arg(0)?),
        Func::Normcdf => normal(arg(1)?, arg(2)?)?.cdf(arg(0)?),
    };
    finite(r, func.name())
}

fn normal(mu: f64, sigma: f64) -> Result<Normal, EvalError> {
    if sigma <= 0.0 {
        return Err(EvalError::Domain(format!("sigma must be > 0, got {sigma}")));
    }
    Normal::new(mu, sigma).map_err(|e| EvalError::Domain(e.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn vars(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    fn eval(src: &str, pairs: &[(&str, f64)]) -> Result<f64, EvalError> {
        Formula::parse(src).unwrap().eval(&vars(pairs))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn spec_example() {
        assert_eq!(
            eval(
                "F(x,y,z) = 2x + 2^y / z^2",
                &[("x", 10.0), ("y", 3.0), ("z", 2.0)]
            ),
            Ok(22.0)
        );
    }

    #[test]
    fn precedence_table() {
        type Case<'a> = (&'a str, &'a [(&'a str, f64)], f64);
        let cases: &[Case<'_>] = &[
            ("1 + 2 * 3", &[], 7.0),
            ("(1 + 2) * 3", &[], 9.0),
            ("2^3^2", &[], 512.0),
            ("-x^2", &[("x", 3.0)], -9.0),
            ("(-x)^2", &[("x", 3.0)], 9.0),
            ("2^-1", &[], 0.5),
            ("10 - 4 - 3", &[], 3.0),
            ("100 / 10 / 5", &[], 2.0),
            ("7 % 4", &[], 3.0),
            ("2x", &[("x", 4.0)], 8.0),
            ("3(x+1)", &[("x", 1.0)], 6.0),
            ("(a+b)(a-b)", &[("a", 5.0), ("b", 3.0)], 16.0),
            ("2x^2", &[("x", 3.0)], 18.0),
            ("abs(x)y", &[("x", -2.0), ("y", 3.0)], 6.0),
            ("2e", &[], 2.0 * std::f64::consts::E),
            ("2e-3", &[], 0.002),
            ("1.5E2", &[], 150.0),
            ("+5", &[], 5.0),
            ("--5", &[], 5.0),
            ("2 * pi", &[], 2.0 * std::f64::consts::PI),
            ("xy", &[("xy", 7.0)], 7.0),
        ];
        for (src, v, want) in cases {
            let got = eval(src, v).unwrap_or_else(|e| panic!("{src}: {e}"));
            assert!(close(got, *want), "{src} = {got}, want {want}");
        }
    }

    #[test]
    fn functions() {
        let cases: &[(&str, f64)] = &[
            ("abs(-3)", 3.0),
            ("sqrt(16)", 4.0),
            ("ln(e)", 1.0),
            ("log10(1000)", 3.0),
            ("log(8, 2)", 3.0),
            ("exp(0)", 1.0),
            ("pow(2, 10)", 1024.0),
            ("min(3, 1, 2)", 1.0),
            ("max(3, 1, 2)", 3.0),
            ("floor(2.7)", 2.0),
            ("ceil(2.1)", 3.0),
            ("round(2.5)", 3.0),
            ("round(1.23456, 2)", 1.23),
            ("clamp(15, 0, 10)", 10.0),
            ("sigmoid(0)", 0.5),
            ("normcdf(0, 0, 1)", 0.5),
            ("if(1, 5, 1/0)", 5.0),
            ("if(0, 1/0, 7)", 7.0),
        ];
        for (src, want) in cases {
            let got = eval(src, &[]).unwrap_or_else(|e| panic!("{src}: {e}"));
            assert!(close(got, *want), "{src} = {got}, want {want}");
        }
        let g = eval("gauss(0, 0, 1)", &[]).unwrap();
        assert!(close(g, 1.0 / (2.0 * std::f64::consts::PI).sqrt()));
    }

    #[test]
    fn eval_traps() {
        assert_eq!(eval("1/x", &[("x", 0.0)]), Err(EvalError::DivisionByZero));
        assert_eq!(eval("5 % 0", &[]), Err(EvalError::DivisionByZero));
        assert!(matches!(eval("sqrt(-1)", &[]), Err(EvalError::Domain(_))));
        assert!(matches!(eval("ln(0)", &[]), Err(EvalError::Domain(_))));
        assert!(matches!(eval("10^400", &[]), Err(EvalError::NonFinite(_))));
        assert!(matches!(eval("exp(1000)", &[]), Err(EvalError::NonFinite(_))));
        assert!(matches!(eval("gauss(1, 0, 0)", &[]), Err(EvalError::Domain(_))));
        assert!(matches!(eval("clamp(1, 5, 0)", &[]), Err(EvalError::Domain(_))));
        assert_eq!(eval("x + 1", &[]), Err(EvalError::UnboundVariable("x".into())));
    }

    #[test]
    fn parse_errors_with_positions() {
        let cases: &[(&str, usize, &str)] = &[
            ("1 +", 3, "end of formula"),
            ("foo(1)", 0, "unknown function 'foo'"),
            ("sqrt(1, 2)", 0, "expects 1"),
            ("log(1)", 0, "expects 2"),
            ("min()", 0, "at least 1"),
            ("(1 + 2", 6, "expected ')'"),
            ("1 $ 2", 2, "unexpected character"),
            ("2 3", 2, "unexpected number"),
            ("F(x) = x + y", 11, "not declared"),
            ("F(x, x) = x", 5, "duplicate parameter"),
            ("F(x) = x = 1", 9, "only one '='"),
            ("1 + F(x) = x", 0, "invalid header"),
            ("F(x) =", 6, "empty formula"),
            ("x y", 2, "unexpected identifier"),
            ("(a)2", 3, "unexpected number"),
            ("x(1)", 0, "unknown function 'x'"),
        ];
        for (src, pos, msg) in cases {
            let err = Formula::parse(src)
                .err()
                .unwrap_or_else(|| panic!("{src} should fail"));
            assert!(err.message.contains(msg), "{src}: {err}");
            assert_eq!(err.position, *pos, "{src}: {err}");
        }
    }

    #[test]
    fn bindings() {
        let f = Formula::parse("F(x,y,z) = 2x + 2^y / z^2").unwrap();
        let args = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        assert!(f.check_bindings(&args(&["x", "y", "z"])).is_ok());
        assert!(f
            .check_bindings(&args(&["x", "y"]))
            .unwrap_err()
            .message
            .contains("missing args [z]"));
        assert!(f
            .check_bindings(&args(&["x", "y", "z", "w"]))
            .unwrap_err()
            .message
            .contains("unexpected args [w]"));
        let g = Formula::parse("a * 2 + pi").unwrap();
        assert!(g.check_bindings(&args(&["a"])).is_ok());
        assert_eq!(g.check_bindings(&args(&[])).unwrap_err().position, 0);
        assert_eq!(g.free_variables(), args(&["a"]));
        assert_eq!(f.name(), Some("F"));
        assert_eq!(f.params().map(<[String]>::len), Some(3));
    }

    #[test]
    fn parameter_named_e_shadows_constant() {
        assert_eq!(eval("F(e) = e + 1", &[("e", 1.0)]), Ok(2.0));
    }

    #[test]
    fn display_reparses_to_same_ast() {
        for src in [
            "F(x,y,z) = 2x + 2^y / z^2",
            "-x^2 + abs(y)(3)",
            "min(a, b, 1e-7) % 3",
            "if(x, 1, -2)",
        ] {
            let f = Formula::parse(src).unwrap();
            let rendered = f.expr().to_string();
            let again = Formula::parse(&rendered).unwrap();
            assert_eq!(f.expr(), again.expr(), "{src} -> {rendered}");
        }
    }
}

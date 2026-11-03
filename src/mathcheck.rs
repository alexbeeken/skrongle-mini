//! Answer checking for maths questions: `numeric` (a number, within a
//! tolerance, optionally with a unit or as a list of values in any order) and
//! `expression` (an algebraic expression, accepted when it is equivalent to
//! the expected one, e.g. `2(x+1)` for `2x + 2`).
//!
//! A small recursive-descent parser and evaluator, no dependencies. What a
//! learner can type:
//!
//! - numbers: `12`, `-3.5`, `1,250`, `2,5` (decimal comma), `2.5e3`, `3/4`,
//!   `1 1/2` (mixed number), `40%`;
//! - operators `+ - * / ^`, parentheses, `!` (factorial), and the usual
//!   look-alikes from phones and copy-paste: `−`, `×`, `·`, `÷`, `²`, `³`, `√`, `π`;
//! - implicit multiplication: `2x`, `3(x+1)`, `xy`, `(x+1)(x-1)`;
//! - functions `sqrt abs exp ln log sin cos tan asin acos atan` (`log` is
//!   base 10) and the constants `pi` and `e`;
//! - single-letter variables (any letter but `e`).
//!
//! Equivalence is decided numerically: both expressions are evaluated at a
//! fixed set of sample points and must agree at every point where both are
//! defined (at least five of them). That is how CAS-free graders (Moodle's
//! STACK fallback, WeBWorK) do it, and it is more than good enough for
//! homework-sized algebra.
//!
//! Input is bounded so a hostile course file or answer cannot overflow the
//! stack (#357): at most [`MAX_LEN`] characters, at most [`MAX_DEPTH`] levels
//! of nesting while parsing, and a parsed tree at most [`MAX_DEPTH`] deep, so
//! the recursive `eval`, `vars` and drop stay shallow.

/// Longest input `parse` accepts, in characters. Answers are short; this also
/// bounds the size of every tree the parser builds.
pub const MAX_LEN: usize = 200;
/// Deepest nesting (brackets, signs, powers, function arguments) and deepest
/// parsed tree `parse` accepts; `eval` gives up past it.
pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func {
    Sqrt,
    Abs,
    Exp,
    Ln,
    Log,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
}

const FUNCS: &[(&str, Func)] = &[
    ("sqrt", Func::Sqrt),
    ("asin", Func::Asin),
    ("acos", Func::Acos),
    ("atan", Func::Atan),
    ("abs", Func::Abs),
    ("exp", Func::Exp),
    ("sin", Func::Sin),
    ("cos", Func::Cos),
    ("tan", Func::Tan),
    ("log", Func::Log),
    ("ln", Func::Ln),
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    /// pi or e: a number that multiplies implicitly like a variable (2pi).
    Const(f64),
    Var(char),
    Func(Func),
    Op(char),
    LParen,
    RParen,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Var(char),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(Func, Box<Expr>),
    Fact(Box<Expr>),
}

/// Replace the look-alike characters people type or paste with ASCII.
fn canonical(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '−' | '–' | '—' => out.push('-'),
            '×' | '·' | '⋅' | '∙' => out.push('*'),
            '÷' | '∶' => out.push('/'),
            '²' => out.push_str("^2"),
            '³' => out.push_str("^3"),
            '√' => out.push_str("sqrt"),
            'π' => out.push_str("pi"),
            '[' | '{' => out.push('('),
            ']' | '}' => out.push(')'),
            _ => out.push(c),
        }
    }
    out
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = canonical(s).chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let mut num = String::new();
            while i < chars.len() {
                let d = chars[i];
                if d.is_ascii_digit() || d == '.' {
                    num.push(d);
                    i += 1;
                } else if d == ',' && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()) {
                    // "1,250" is a thousands separator (exactly three digits
                    // follow, then no digit); "2,5" is a decimal comma.
                    let run = chars[i + 1..].iter().take_while(|n| n.is_ascii_digit()).count();
                    if run == 3 {
                        i += 1;
                    } else if !num.contains('.') {
                        num.push('.');
                        i += 1;
                    } else {
                        break;
                    }
                } else if (d == 'e' || d == 'E')
                    && (chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())
                        || (matches!(chars.get(i + 1), Some('+') | Some('-')) && chars.get(i + 2).is_some_and(|n| n.is_ascii_digit())))
                {
                    num.push('e');
                    i += 1;
                    if matches!(chars[i], '+' | '-') {
                        num.push(chars[i]);
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            let v: f64 = num.parse().map_err(|_| format!("'{num}' is not a number"))?;
            toks.push(Tok::Num(v));
        } else if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect::<String>().to_lowercase();
            let mut j = 0;
            let w: Vec<char> = word.chars().collect();
            while j < w.len() {
                let rest: String = w[j..].iter().collect();
                if let Some((name, f)) = FUNCS.iter().find(|(name, _)| rest.starts_with(name)) {
                    toks.push(Tok::Func(*f));
                    j += name.chars().count();
                } else if rest.starts_with("pi") {
                    toks.push(Tok::Const(std::f64::consts::PI));
                    j += 2;
                } else if w[j] == 'e' {
                    toks.push(Tok::Const(std::f64::consts::E));
                    j += 1;
                } else if w[j].is_ascii_alphabetic() {
                    toks.push(Tok::Var(w[j]));
                    j += 1;
                } else {
                    return Err(format!("'{}' is not something the checker understands", w[j]));
                }
            }
        } else if matches!(c, '+' | '-' | '*' | '/' | '^' | '!' | '%') {
            toks.push(Tok::Op(c));
            i += 1;
        } else if c == '(' {
            toks.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            toks.push(Tok::RParen);
            i += 1;
        } else {
            return Err(format!("'{c}' is not something the checker understands"));
        }
    }
    Ok(toks)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// Current nesting depth; every recursive cycle of the grammar passes
    /// through `nested`.
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// Run one nested step, failing past `MAX_DEPTH` instead of recursing on.
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        if self.depth >= MAX_DEPTH {
            return Err("the answer is nested too deeply".into());
        }
        self.depth += 1;
        let r = f(self);
        self.depth -= 1;
        r
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut left = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let right = self.term()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(op @ ('*' | '/'))) => {
                    self.pos += 1;
                    let right = self.unary()?;
                    left = Expr::Bin(op, Box::new(left), Box::new(right));
                }
                // Implicit multiplication: 2x, x(y+1), (a)(b), 3 sqrt(2). Two
                // bare numbers in a row ("1 1/2") are not a product.
                Some(Tok::Var(_)) | Some(Tok::Const(_)) | Some(Tok::Func(_)) | Some(Tok::LParen) => {
                    let right = self.power()?;
                    left = Expr::Bin('*', Box::new(left), Box::new(right));
                }
                Some(Tok::Num(_)) if !matches!(self.toks.get(self.pos.wrapping_sub(1)), Some(Tok::Num(_))) => {
                    let right = self.power()?;
                    left = Expr::Bin('*', Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.nested(Self::unary)?)))
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.nested(Self::unary)
            }
            _ => self.power(),
        }
    }

    /// Right-associative, binding tighter than unary minus: -x^2 = -(x^2).
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.postfix()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let exp = self.nested(Self::unary)?;
            return Ok(Expr::Bin('^', Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Some(Tok::Op('!')) => {
                    self.pos += 1;
                    e = Expr::Fact(Box::new(e));
                }
                Some(Tok::Op('%')) => {
                    self.pos += 1;
                    e = Expr::Bin('/', Box::new(e), Box::new(Expr::Num(100.0)));
                }
                _ => return Ok(e),
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(v)) | Some(Tok::Const(v)) => Ok(Expr::Num(v)),
            Some(Tok::Var(c)) => Ok(Expr::Var(c)),
            Some(Tok::Func(f)) => {
                // sqrt(2), sqrt 2, sin x^2 = sin(x^2)
                let arg = if let Some(Tok::LParen) = self.peek() { self.nested(Self::primary)? } else { self.nested(Self::power)? };
                Ok(Expr::Call(f, Box::new(arg)))
            }
            Some(Tok::LParen) => {
                let e = self.nested(Self::expr)?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err("a bracket is not closed".into()),
                }
            }
            Some(Tok::RParen) => Err("a closing bracket has no opening one".into()),
            Some(Tok::Op(o)) => Err(format!("'{o}' is missing something on one side")),
            None => Err("the answer ends too early".into()),
        }
    }
}

/// Parse an expression. Errors are short and meant for the learner.
pub fn parse(s: &str) -> Result<Expr, String> {
    if s.chars().count() > MAX_LEN {
        return Err(format!("the answer is too long (at most {MAX_LEN} characters)"));
    }
    let toks = tokenize(s)?;
    if toks.is_empty() {
        return Err("there is nothing to check".into());
    }
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let e = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(match p.toks[p.pos] {
            Tok::RParen => "a closing bracket has no opening one".into(),
            Tok::Num(_) => "two numbers in a row: add an operator between them".into(),
            _ => "the answer could not be read to the end".into(),
        });
    }
    // Long chains (1+1+1+…) are built by loops, not recursion, so the tree
    // can be deeper than the nesting the parser saw.
    if e.depth() > MAX_DEPTH {
        return Err("the answer is nested too deeply".into());
    }
    Ok(e)
}

impl Expr {
    /// Depth of the tree (a leaf is 1). Only called on trees from inputs of
    /// at most `MAX_LEN` characters, so the recursion is bounded.
    fn depth(&self) -> usize {
        match self {
            Expr::Num(_) | Expr::Var(_) => 1,
            Expr::Neg(a) | Expr::Call(_, a) | Expr::Fact(a) => 1 + a.depth(),
            Expr::Bin(_, a, b) => 1 + a.depth().max(b.depth()),
        }
    }

    pub fn vars(&self, out: &mut Vec<char>) {
        match self {
            Expr::Num(_) => {}
            Expr::Var(c) => {
                if !out.contains(c) {
                    out.push(*c);
                }
            }
            Expr::Neg(a) | Expr::Call(_, a) | Expr::Fact(a) => a.vars(out),
            Expr::Bin(_, a, b) => {
                a.vars(out);
                b.vars(out);
            }
        }
    }

    /// Value at a point; NaN where undefined, or past `MAX_DEPTH` levels
    /// (which `parse` never produces; a guard for hand-built trees).
    pub fn eval(&self, env: &dyn Fn(char) -> f64) -> f64 {
        self.eval_at(env, 0)
    }

    fn eval_at(&self, env: &dyn Fn(char) -> f64, depth: usize) -> f64 {
        if depth >= MAX_DEPTH {
            return f64::NAN;
        }
        let d = depth + 1;
        match self {
            Expr::Num(v) => *v,
            Expr::Var(c) => env(*c),
            Expr::Neg(a) => -a.eval_at(env, d),
            Expr::Bin(op, a, b) => {
                let (x, y) = (a.eval_at(env, d), b.eval_at(env, d));
                match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    '/' => x / y,
                    '^' => x.powf(y),
                    _ => f64::NAN,
                }
            }
            Expr::Call(f, a) => {
                let x = a.eval_at(env, d);
                match f {
                    Func::Sqrt => x.sqrt(),
                    Func::Abs => x.abs(),
                    Func::Exp => x.exp(),
                    Func::Ln => x.ln(),
                    Func::Log => x.log10(),
                    Func::Sin => x.sin(),
                    Func::Cos => x.cos(),
                    Func::Tan => x.tan(),
                    Func::Asin => x.asin(),
                    Func::Acos => x.acos(),
                    Func::Atan => x.atan(),
                }
            }
            Expr::Fact(a) => {
                let x = a.eval_at(env, d);
                if x < 0.0 || x.fract() != 0.0 || x > 170.0 {
                    f64::NAN
                } else {
                    (1..=x as u64).fold(1.0, |acc, k| acc * k as f64)
                }
            }
        }
    }
}

/// Default closeness: equal up to floating-point noise.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Drop a leading "x =" / "y=" (an answer written as an equation).
fn strip_lhs(s: &str) -> &str {
    let t = s.trim();
    if let Some((lhs, rhs)) = t.split_once('=') {
        let lhs = lhs.trim();
        if lhs.chars().count() <= 3 && lhs.chars().all(|c| c.is_alphabetic() || c == '(' || c == ')') {
            return rhs.trim();
        }
    }
    t
}

/// The value of a typed number: a constant expression (`3/4`, `2^10`,
/// `sqrt(2)`, `40%`) or a mixed number (`1 1/2`, `-2 3/4`).
pub fn number(s: &str) -> Result<f64, String> {
    let t = s.trim();
    // Mixed number: whole, space, fraction.
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() == 2 && parts[1].contains('/') {
        let whole = parts[0];
        let frac: Vec<&str> = parts[1].split('/').collect();
        let digits = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());
        let w = whole.strip_prefix('-').unwrap_or(whole);
        if digits(w) && frac.len() == 2 && digits(frac[0]) && digits(frac[1]) {
            let (n, d): (f64, f64) = (frac[0].parse().unwrap_or(0.0), frac[1].parse().unwrap_or(1.0));
            if d == 0.0 {
                return Err("a fraction cannot have 0 under the line".into());
            }
            let v = w.parse::<f64>().unwrap_or(0.0) + n / d;
            return Ok(if whole.starts_with('-') { -v } else { v });
        }
    }
    let e = parse(t)?;
    let mut vars = Vec::new();
    e.vars(&mut vars);
    if !vars.is_empty() {
        return Err("type a number (no letters)".into());
    }
    let v = e.eval(&|_| f64::NAN);
    if !v.is_finite() {
        return Err("that does not work out to a number".into());
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MathVerdict {
    pub correct: bool,
    pub note: Option<String>,
}

impl MathVerdict {
    fn right() -> MathVerdict {
        MathVerdict { correct: true, note: None }
    }
    fn wrong(note: Option<String>) -> MathVerdict {
        MathVerdict { correct: false, note }
    }
}

/// Remove the unit from the end of a typed answer, if it is there.
fn strip_unit<'a>(s: &'a str, unit: &str) -> &'a str {
    let t = s.trim();
    let u = unit.trim();
    if u.is_empty() {
        return t;
    }
    let lower = t.to_lowercase();
    if lower.ends_with(&u.to_lowercase()) {
        let cut = t.len() - u.len();
        if t.is_char_boundary(cut) {
            return t[..cut].trim_end();
        }
    }
    t
}

/// Split a list of values: `2; -3`, `2 and -3`, `x = 2 or x = -3`, and
/// (when nothing else splits it) `2, -3`.
fn split_list(s: &str, want: usize) -> Vec<String> {
    let lower = s.to_lowercase();
    let mut parts: Vec<String> = lower
        .split(';')
        .flat_map(|p| p.split(" and "))
        .flat_map(|p| p.split(" or "))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() == 1 && want > 1 {
        parts = parts[0].split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    }
    parts
}

/// Check a `numeric` answer. `expected` is the question's correct answer
/// (values separated by `;` for an any-order list), `tolerance` an absolute
/// margin, `unit` an optional unit the learner may type after the number.
pub fn check_numeric(expected: &str, given: &str, tolerance: Option<f64>, unit: &str) -> MathVerdict {
    let want_parts: Vec<&str> = expected.split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
    let want: Vec<f64> = match want_parts.iter().map(|p| number(strip_unit(strip_lhs(p), unit))).collect::<Result<Vec<_>, _>>() {
        Ok(v) => v,
        Err(_) => return MathVerdict::wrong(None),
    };
    let got_parts: Vec<String> =
        if want.len() > 1 { split_list(given, want.len()) } else { vec![given.trim().to_string()] };
    let mut got = Vec::new();
    for p in &got_parts {
        match number(strip_unit(strip_lhs(p), unit)) {
            Ok(v) => got.push(v),
            Err(e) => return MathVerdict::wrong(Some(format!("Could not read “{}” as a number: {e}.", p.trim()))),
        }
    }
    if got.len() != want.len() {
        let n = want.len();
        return MathVerdict::wrong(Some(if n > 1 {
            format!("The answer has {n} values; separate them with “;” or “and”.")
        } else {
            "The answer is a single number.".to_string()
        }));
    }
    let ok = |a: f64, b: f64| match tolerance {
        Some(t) => (a - b).abs() <= t + 1e-9 * a.abs().max(1.0),
        None => close(a, b),
    };
    // Match each expected value to a distinct given one (order does not matter).
    let mut used = vec![false; got.len()];
    let mut all = true;
    for w in &want {
        match (0..got.len()).find(|&i| !used[i] && ok(*w, got[i])) {
            Some(i) => used[i] = true,
            None => {
                all = false;
                break;
            }
        }
    }
    if all {
        return MathVerdict::right();
    }
    // A near miss on a single value is almost always rounding.
    if want.len() == 1 {
        let (w, g) = (want[0], got[0]);
        if w != 0.0 && ((w - g) / w).abs() <= 0.01 {
            return MathVerdict::wrong(Some("Very close: check your rounding.".into()));
        }
        if close(w, -g) && w != 0.0 {
            return MathVerdict::wrong(Some("Check the sign.".into()));
        }
    }
    MathVerdict::wrong(None)
}

/// Does typed text read as a numeric answer (one number or a list of them,
/// with or without the unit)? The tutor uses it to tell an attempt from chat.
pub fn reads_as_numeric(text: &str, unit: &str) -> bool {
    let parts = split_list(text, 2);
    !parts.is_empty() && parts.iter().all(|p| number(strip_unit(strip_lhs(p), unit)).is_ok())
}

/// Fixed pseudo-random sample points, so grading is deterministic.
fn samples(n: usize, lo: f64, hi: f64, salt: u64) -> Vec<f64> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    (0..n)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let r = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
            lo + r * (hi - lo)
        })
        .collect()
}

/// Are two expressions equivalent? `None` when they could not be compared
/// (too few points where both are defined).
fn equivalent(a: &Expr, b: &Expr) -> Option<bool> {
    let mut vars = Vec::new();
    a.vars(&mut vars);
    b.vars(&mut vars);
    // Mixed-sign points first; positive-only for sqrt/ln-heavy expressions.
    for (lo, hi) in [(-2.7, 2.9), (0.1, 3.1)] {
        let cols: Vec<Vec<f64>> = vars.iter().enumerate().map(|(i, _)| samples(16, lo, hi, i as u64 + 1)).collect();
        let mut valid = 0;
        for k in 0..16 {
            let env = |c: char| vars.iter().position(|v| *v == c).map(|i| cols[i][k]).unwrap_or(f64::NAN);
            let (x, y) = (a.eval(&env), b.eval(&env));
            if !x.is_finite() || !y.is_finite() {
                if x.is_finite() != y.is_finite() && x.is_finite() {
                    // Defined for the expected answer but not the given one.
                    return Some(false);
                }
                continue;
            }
            if (x - y).abs() > 1e-6 * x.abs().max(y.abs()).max(1.0) {
                return Some(false);
            }
            valid += 1;
        }
        if valid >= 5 {
            return Some(true);
        }
    }
    None
}

/// Check an `expression` answer: equivalent to the expected expression.
pub fn check_expression(expected: &str, given: &str) -> MathVerdict {
    let Ok(want) = parse(strip_lhs(expected)) else { return MathVerdict::wrong(None) };
    let got = match parse(strip_lhs(given)) {
        Ok(e) => e,
        Err(e) => return MathVerdict::wrong(Some(format!("Could not read that expression: {e}."))),
    };
    match equivalent(&want, &got) {
        Some(true) => MathVerdict::right(),
        Some(false) => {
            let (mut wv, mut gv) = (Vec::new(), Vec::new());
            want.vars(&mut wv);
            got.vars(&mut gv);
            let stray: Vec<char> = gv.iter().filter(|c| !wv.contains(c)).copied().collect();
            if !stray.is_empty() && !wv.is_empty() {
                let list: Vec<String> = wv.iter().map(|c| c.to_string()).collect();
                return MathVerdict::wrong(Some(format!("Use the variable{} {}.", if list.len() > 1 { "s" } else { "" }, list.join(", "))));
            }
            MathVerdict::wrong(None)
        }
        None => MathVerdict::wrong(Some("That expression could not be compared; try writing it another way.".into())),
    }
}

/// Validation for course files: the expected answer must be readable.
pub fn validate_numeric(answer: &str, unit: &str) -> Result<(), String> {
    for p in answer.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        number(strip_unit(strip_lhs(p), unit)).map_err(|e| format!("'{p}' is not a number the checker can read ({e})"))?;
    }
    if answer.split(';').all(|p| p.trim().is_empty()) {
        return Err("the answer is empty".into());
    }
    Ok(())
}

pub fn validate_expression(answer: &str) -> Result<(), String> {
    let e = parse(strip_lhs(answer)).map_err(|e| format!("'{answer}' is not an expression the checker can read ({e})"))?;
    // It must be defined somewhere, or nothing can ever be compared to it.
    match equivalent(&e, &e) {
        Some(_) => Ok(()),
        None => Err(format!("'{answer}' is undefined at almost every point, so answers cannot be checked against it")),
    }
}

/// The first answer of a numeric or expression question (its
/// `correct_answer`, then its `accepted_answers`) that the checker cannot
/// use, with why. The course rules refuse a file for it; the lesson editor
/// asks before a save (`/api/courses/{id}/answer-key`). Other types pass.
pub fn validate_answers(q: &crate::content::QuizItem) -> Result<(), (String, String)> {
    use crate::content::QuizType;
    for a in std::iter::once(&q.correct_answer).chain(&q.accepted_answers) {
        let checked = match q.kind {
            QuizType::Numeric => validate_numeric(a, &q.unit),
            QuizType::Expression => validate_expression(a),
            _ => Ok(()),
        };
        checked.map_err(|e| (a.clone(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_every_spelling() {
        for (s, v) in [
            ("12", 12.0),
            ("-3.5", -3.5),
            ("1,250", 1250.0),
            ("2,5", 2.5),
            ("2.5e3", 2500.0),
            ("3/4", 0.75),
            ("1 1/2", 1.5),
            ("-2 3/4", -2.75),
            ("40%", 0.4),
            ("2^10", 1024.0),
            ("−4", -4.0),
            ("5!", 120.0),
            ("√16", 4.0),
            ("2π", 2.0 * std::f64::consts::PI),
        ] {
            assert!((number(s).unwrap() - v).abs() < 1e-9, "{s}");
        }
        assert!(number("x+1").is_err());
        assert!(number("1/0").is_err());
        assert!(number("(2").is_err());
    }

    #[test]
    fn numeric_answers() {
        assert!(check_numeric("0.75", "3/4", None, "").correct);
        assert!(check_numeric("3/4", "0.75", None, "").correct);
        assert!(check_numeric("x = 4", "4", None, "").correct);
        assert!(check_numeric("4", "x=4", None, "").correct);
        assert!(check_numeric("12", "12 cm", None, "cm").correct);
        assert!(check_numeric("12", "12cm", None, "cm").correct);
        assert!(!check_numeric("12", "12 m", None, "cm").correct);
        assert!(check_numeric("3.14", "3.1416", Some(0.01), "").correct);
        assert!(!check_numeric("3.14", "3.2", Some(0.01), "").correct);
        let v = check_numeric("3.142", "3.14", None, "");
        assert!(!v.correct && v.note.unwrap().contains("rounding"));
        let v = check_numeric("5", "-5", None, "");
        assert!(v.note.unwrap().contains("sign"));
        // Lists, in any order and any separator.
        assert!(check_numeric("2; -3", "-3; 2", None, "").correct);
        assert!(check_numeric("2; -3", "x = 2 or x = -3", None, "").correct);
        assert!(check_numeric("2; -3", "-3, 2", None, "").correct);
        assert!(check_numeric("2; -3", "2 and -3", None, "").correct);
        assert!(!check_numeric("2; -3", "2", None, "").correct);
        assert!(!check_numeric("2; 2", "2; 3", None, "").correct);
        assert!(check_numeric("1250", "1,250", None, "").correct);
        let v = check_numeric("4", "four", None, "");
        assert!(!v.correct && v.note.is_some());
    }

    #[test]
    fn expressions_are_equivalent_or_not() {
        for (a, b) in [
            ("2x + 2", "2(x+1)"),
            ("x^2 - 1", "(x-1)(x+1)"),
            ("x^2 + 2x + 1", "(x + 1)^2"),
            ("6x^2", "2x * 3x"),
            ("1/2 x", "x/2"),
            ("sqrt(x^2)", "abs(x)"),
            ("exp(2x)", "exp(x)^2"),
            ("log(100)", "2"),
            ("y = 3x - 4", "-4 + 3x"),
            ("a^2 + 2ab + b^2", "(a+b)^2"),
            ("x²", "x*x"),
            ("2πr", "2 pi r"),
            ("sin(x)^2 + cos(x)^2", "1"),
        ] {
            assert!(check_expression(a, b).correct, "{a} vs {b}");
        }
        for (a, b) in [("2x + 2", "2x + 1"), ("x^2", "2x"), ("(x+1)^2", "x^2 + 1"), ("-x^2", "(-x)^2")] {
            assert!(!check_expression(a, b).correct, "{a} vs {b}");
        }
        let v = check_expression("2x", "2y");
        assert!(v.note.unwrap().contains("variable x"));
        assert!(check_expression("2x", "2x)").note.is_some());
    }

    #[test]
    fn validation() {
        assert!(validate_numeric("2; -3", "").is_ok());
        assert!(validate_numeric("12 cm", "cm").is_ok());
        assert!(validate_numeric("twelve", "").is_err());
        assert!(validate_numeric(" ; ", "").is_err());
        assert!(validate_expression("3x^2 + 1").is_ok());
        assert!(validate_expression("3x +").is_err());
        assert!(validate_expression("ln(-x^2 - 1)").is_err());
    }

    /// #357: deeply nested or very long input is an error, not a stack
    /// overflow. Runs on the default test-thread stack (2 MiB).
    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let deep = format!("{}x{}", "(".repeat(5000), ")".repeat(5000));
        assert!(validate_expression(&deep).unwrap_err().contains("too long"));
        assert!(parse(&deep).is_err());
        assert!(check_expression("x", &deep).note.is_some());
        assert!(number(&format!("{}1{}", "(".repeat(5000), ")".repeat(5000))).is_err());
        // Within the length cap, nesting past MAX_DEPTH is refused too.
        for deep in [
            format!("{}x{}", "(".repeat(90), ")".repeat(90)),
            "-".repeat(150) + "x",
            vec!["2"; 90].join("^"),
            "ln".repeat(70) + "x",
            vec!["1"; 90].join("+"),
        ] {
            assert!(deep.len() <= MAX_LEN, "{}", deep.len());
            assert!(parse(&deep).unwrap_err().contains("nested too deeply"), "{deep}");
        }
        // Real answers are far from the limits.
        assert!(validate_expression(&format!("{}x+1{}", "(".repeat(20), ")".repeat(20))).is_ok());
        assert!(validate_expression("x^5 + x^4 + x^3 + x^2 + x + 1").is_ok());
        // eval's own guard, for a tree built by hand.
        let mut e = Expr::Var('x');
        for _ in 0..(MAX_DEPTH + 5) {
            e = Expr::Neg(Box::new(e));
        }
        assert!(e.eval(&|_| 1.0).is_nan());
    }
}

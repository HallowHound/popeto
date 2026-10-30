//! Tiny JavaScript-subset expression engine used by Shimeji configs
//! (`${...}` / `#{...}` scripts such as `mascot.anchor.x < 100 && Math.random() < 0.5`).

use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum Val {
    Num(f64),
    Bool(bool),
    Str(Rc<str>),
    Point(i32, i32),
    Obj(Rc<[String]>),
    Undef,
}

impl Val {
    pub fn num(&self) -> f64 {
        match self {
            Val::Num(n) => *n,
            Val::Bool(b) => *b as i32 as f64,
            Val::Str(s) => s.trim().parse().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }
    /// Java-style `intValue()`: truncation, NaN -> 0, saturating.
    pub fn int(&self) -> i32 {
        let n = self.num();
        if n.is_nan() { 0 } else { n as i32 }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Val::Num(n) => *n != 0.0 && !n.is_nan(),
            Val::Bool(b) => *b,
            Val::Str(s) => !s.is_empty(),
            Val::Point(..) | Val::Obj(_) => true,
            Val::Undef => false,
        }
    }
    pub fn as_str(&self) -> String {
        match self {
            Val::Str(s) => s.to_string(),
            Val::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", *n as i64) } else { format!("{n}") }
            }
            Val::Bool(b) => b.to_string(),
            Val::Point(x, y) => format!("{x},{y}"),
            Val::Obj(p) => p.join("."),
            Val::Undef => "undefined".into(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Op { Add, Sub, Mul, Div, Mod, Lt, Le, Gt, Ge, Eq, Ne }

#[derive(Debug)]
pub enum Expr {
    Num(f64),
    Str(Rc<str>),
    Path(Vec<String>),
    Call(Vec<String>, Vec<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Tern(Box<Expr>, Box<Expr>, Box<Expr>),
}

pub trait Scope {
    fn get(&self, path: &[String]) -> Val;
    fn call(&self, path: &[String], args: &[Val]) -> Val;
}

// ---------------------------------------------------------------- tokenizer

#[derive(Debug, Clone, PartialEq)]
enum Tok { Num(f64), Id(String), Str(String), P(&'static str) }

const PUNCT: [&str; 23] = [
    "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "(", ")", ".", ",", "?", ":", "!", "<", ">", "+", "-",
    "*", "/", "%", "=",
];

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() { i += 1; continue; }
        if c.is_ascii_digit() || (c == '.' && i + 1 < b.len() && (b[i + 1] as char).is_ascii_digit()) {
            let s = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'.') {
                if (b[i] == b'e' || b[i] == b'E') && i + 1 < b.len() && (b[i + 1] == b'-' || b[i + 1] == b'+') { i += 1; }
                i += 1;
            }
            let t = &src[s..i];
            out.push(Tok::Num(t.parse().map_err(|_| format!("bad number {t}"))?));
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let s = i;
            while i < b.len() {
                let ch = src[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '_' || ch == '$' { i += ch.len_utf8(); } else { break; }
            }
            out.push(Tok::Id(src[s..i].to_string()));
            continue;
        }
        if c == '"' || c == '\'' {
            let q = b[i];
            i += 1;
            let s = i;
            while i < b.len() && b[i] != q { i += 1; }
            out.push(Tok::Str(src[s..i].to_string()));
            i += 1;
            continue;
        }
        let rest = &src[i..];
        match PUNCT.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => { out.push(Tok::P(p)); i += p.len(); }
            None => return Err(format!("unexpected '{}'", rest.chars().next().unwrap())),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- parser

struct Parser { t: Vec<Tok>, i: usize }

impl Parser {
    fn peek(&self) -> Option<&Tok> { self.t.get(self.i) }
    fn eat(&mut self, p: &str) -> bool {
        if let Some(Tok::P(x)) = self.peek() { if *x == p { self.i += 1; return true; } }
        false
    }
    fn expect(&mut self, p: &str) -> Result<(), String> {
        if self.eat(p) { Ok(()) } else { Err(format!("expected '{p}'")) }
    }
    fn tern(&mut self) -> Result<Expr, String> {
        let c = self.or()?;
        if self.eat("?") {
            let a = self.tern()?;
            self.expect(":")?;
            let b = self.tern()?;
            return Ok(Expr::Tern(c.into(), a.into(), b.into()));
        }
        Ok(c)
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.eat("||") { l = Expr::Or(l.into(), self.and()?.into()); }
        Ok(l)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.eq()?;
        while self.eat("&&") { l = Expr::And(l.into(), self.eq()?.into()); }
        Ok(l)
    }
    fn eq(&mut self) -> Result<Expr, String> {
        let mut l = self.rel()?;
        loop {
            let op = if self.eat("===") || self.eat("==") { Op::Eq } else if self.eat("!==") || self.eat("!=") { Op::Ne } else { break };
            l = Expr::Bin(op, l.into(), self.rel()?.into());
        }
        Ok(l)
    }
    fn rel(&mut self) -> Result<Expr, String> {
        let mut l = self.add()?;
        loop {
            let op = if self.eat("<=") { Op::Le } else if self.eat(">=") { Op::Ge } else if self.eat("<") { Op::Lt } else if self.eat(">") { Op::Gt } else { break };
            l = Expr::Bin(op, l.into(), self.add()?.into());
        }
        Ok(l)
    }
    fn add(&mut self) -> Result<Expr, String> {
        let mut l = self.mul()?;
        loop {
            let op = if self.eat("+") { Op::Add } else if self.eat("-") { Op::Sub } else { break };
            l = Expr::Bin(op, l.into(), self.mul()?.into());
        }
        Ok(l)
    }
    fn mul(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        loop {
            let op = if self.eat("*") { Op::Mul } else if self.eat("/") { Op::Div } else if self.eat("%") { Op::Mod } else { break };
            l = Expr::Bin(op, l.into(), self.unary()?.into());
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("!") { return Ok(Expr::Not(self.unary()?.into())); }
        if self.eat("-") { return Ok(Expr::Neg(self.unary()?.into())); }
        if self.eat("+") { return Ok(Expr::Bin(Op::Mul, Expr::Num(1.0).into(), self.unary()?.into())); }
        self.postfix()
    }
    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            if self.eat(".") {
                let Some(Tok::Id(name)) = self.peek().cloned() else { return Err("expected name after '.'".into()) };
                self.i += 1;
                match &mut e {
                    Expr::Path(p) => p.push(name),
                    _ => return Err("member access on non-path".into()),
                }
            } else if self.eat("(") {
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.tern()?);
                        if self.eat(")") { break; }
                        self.expect(",")?;
                    }
                }
                e = match e {
                    Expr::Path(p) => Expr::Call(p, args),
                    _ => return Err("call on non-path".into()),
                };
            } else {
                break;
            }
        }
        Ok(e)
    }
    fn primary(&mut self) -> Result<Expr, String> {
        let t = self.peek().cloned().ok_or("unexpected end")?;
        self.i += 1;
        Ok(match t {
            Tok::Num(n) => Expr::Num(n),
            Tok::Str(s) => Expr::Str(s.into()),
            Tok::Id(s) => Expr::Path(vec![s]),
            Tok::P("(") => { let e = self.tern()?; self.expect(")")?; e }
            Tok::P(p) => return Err(format!("unexpected '{p}'")),
        })
    }
}

pub fn parse(src: &str) -> Result<Expr, String> {
    let mut p = Parser { t: lex(src)?, i: 0 };
    let e = p.tern()?;
    if p.i != p.t.len() { return Err(format!("trailing tokens in '{src}'")); }
    Ok(e)
}

// ---------------------------------------------------------------- evaluator

thread_local! {
    static RNG: std::cell::Cell<u64> = std::cell::Cell::new({
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        t | 1
    });
}

pub fn random() -> f64 {
    RNG.with(|r| {
        let mut x = r.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        r.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

fn math(name: &str, a: &[Val]) -> Val {
    let n = |i: usize| a.get(i).map(|v| v.num()).unwrap_or(f64::NAN);
    Val::Num(match name {
        "random" => random(),
        "abs" => n(0).abs(),
        "floor" => n(0).floor(),
        "ceil" => n(0).ceil(),
        "round" => (n(0) + 0.5).floor(),
        "sqrt" => n(0).sqrt(),
        "pow" => n(0).powf(n(1)),
        "sin" => n(0).sin(),
        "cos" => n(0).cos(),
        "tan" => n(0).tan(),
        "atan" => n(0).atan(),
        "atan2" => n(0).atan2(n(1)),
        "exp" => n(0).exp(),
        "log" => n(0).ln(),
        "min" => a.iter().map(|v| v.num()).fold(f64::INFINITY, |m, v| if v.is_nan() || m.is_nan() { f64::NAN } else { m.min(v) }),
        "max" => a.iter().map(|v| v.num()).fold(f64::NEG_INFINITY, |m, v| if v.is_nan() || m.is_nan() { f64::NAN } else { m.max(v) }),
        _ => f64::NAN,
    })
}

fn equal(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Str(x), Val::Str(y)) => x == y,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Undef, Val::Undef) => true,
        (Val::Point(a, b), Val::Point(c, d)) => a == c && b == d,
        _ => a.num() == b.num(),
    }
}

pub fn eval(e: &Expr, s: &dyn Scope) -> Val {
    match e {
        Expr::Num(n) => Val::Num(*n),
        Expr::Str(t) => Val::Str(t.clone()),
        Expr::Path(p) => {
            if p.len() == 1 {
                match p[0].as_str() {
                    "true" => return Val::Bool(true),
                    "false" => return Val::Bool(false),
                    "NaN" | "undefined" | "null" => return if p[0] == "NaN" { Val::Num(f64::NAN) } else { Val::Undef },
                    "Infinity" => return Val::Num(f64::INFINITY),
                    _ => {}
                }
            }
            if p[0] == "Math" && p.len() == 2 {
                return match p[1].as_str() {
                    "PI" => Val::Num(std::f64::consts::PI),
                    "E" => Val::Num(std::f64::consts::E),
                    _ => Val::Undef,
                };
            }
            s.get(p)
        }
        Expr::Call(p, args) => {
            let a: Vec<Val> = args.iter().map(|x| eval(x, s)).collect();
            if p.len() == 2 && p[0] == "Math" { return math(&p[1], &a); }
            s.call(p, &a)
        }
        Expr::Not(x) => Val::Bool(!eval(x, s).truthy()),
        Expr::Neg(x) => Val::Num(-eval(x, s).num()),
        Expr::And(a, b) => { let l = eval(a, s); if !l.truthy() { l } else { eval(b, s) } }
        Expr::Or(a, b) => { let l = eval(a, s); if l.truthy() { l } else { eval(b, s) } }
        Expr::Tern(c, a, b) => if eval(c, s).truthy() { eval(a, s) } else { eval(b, s) },
        Expr::Bin(op, a, b) => {
            let l = eval(a, s);
            let r = eval(b, s);
            match op {
                Op::Add => match (&l, &r) {
                    (Val::Str(_), _) | (_, Val::Str(_)) => Val::Str(format!("{}{}", l.as_str(), r.as_str()).into()),
                    _ => Val::Num(l.num() + r.num()),
                },
                Op::Sub => Val::Num(l.num() - r.num()),
                Op::Mul => Val::Num(l.num() * r.num()),
                Op::Div => Val::Num(l.num() / r.num()),
                Op::Mod => Val::Num(l.num() % r.num()),
                Op::Lt => Val::Bool(l.num() < r.num()),
                Op::Le => Val::Bool(l.num() <= r.num()),
                Op::Gt => Val::Bool(l.num() > r.num()),
                Op::Ge => Val::Bool(l.num() >= r.num()),
                Op::Eq => Val::Bool(equal(&l, &r)),
                Op::Ne => Val::Bool(!equal(&l, &r)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct S;
    impl Scope for S {
        fn get(&self, p: &[String]) -> Val {
            match p.join(".").as_str() {
                "mascot.anchor.x" => Val::Num(10.0),
                "mascot.lookRight" => Val::Bool(true),
                _ => Val::Undef,
            }
        }
        fn call(&self, _: &[String], _: &[Val]) -> Val { Val::Bool(true) }
    }
    #[test]
    fn basics() {
        let e = parse("mascot.lookRight ? mascot.anchor.x + 5 : -1").unwrap();
        assert_eq!(eval(&e, &S).num(), 15.0);
        let e = parse("Math.min(3, 2*4) < 4 && !false").unwrap();
        assert!(eval(&e, &S).truthy());
        let e = parse("100 - Math.random*100").unwrap();
        assert!(eval(&e, &S).num().is_nan());
        let e = parse("mascot.environment.floor.isOn(mascot.anchor) || 0").unwrap();
        assert!(eval(&e, &S).truthy());
    }
}

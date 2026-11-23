//! The decompiler's expressions and statements, and how they print as C-like source.

use std::collections::HashSet;
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Var {
    Param(u32),
    Local(u32),
    /// One of the two slots between the parameters and the locals, which the
    /// VM fills with the return address and the caller's frame.
    Frame(u32),
    Static(u32),
    Global(u32),
    /// A value the stack carries from one basic block into the next.
    Temp(u32),
}

impl std::fmt::Display for Var {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Var::Param(n) => write!(f, "param{n}"),
            Var::Local(n) => write!(f, "local{n}"),
            Var::Frame(n) => write!(f, "frame{n}"),
            Var::Static(n) => write!(f, "static_{n}"),
            Var::Global(n) => write!(f, "global_{n}"),
            Var::Temp(n) => write!(f, "stack{n}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Vector,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    LogicalAnd,
    LogicalOr,
}

impl BinOp {
    fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::And => "&",
            BinOp::Or => "|",
            BinOp::Xor => "^",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::LogicalAnd => "&&",
            BinOp::LogicalOr => "||",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            BinOp::LogicalOr => 1,
            BinOp::LogicalAnd => 2,
            BinOp::Or => 3,
            BinOp::Xor => 4,
            BinOp::And => 5,
            BinOp::Eq | BinOp::Ne => 6,
            BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le => 7,
            BinOp::Add | BinOp::Sub => 9,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 10,
        }
    }

    /// Whether `x = x op y` may print as `x op= y`.
    fn compounds(self) -> bool {
        matches!(
            self,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::And
                | BinOp::Or
                | BinOp::Xor
        )
    }

    /// The comparison that holds exactly when this one does not, on integers.
    fn negated(self) -> Option<BinOp> {
        Some(match self {
            BinOp::Eq => BinOp::Ne,
            BinOp::Ne => BinOp::Eq,
            BinOp::Gt => BinOp::Le,
            BinOp::Ge => BinOp::Lt,
            BinOp::Lt => BinOp::Ge,
            BinOp::Le => BinOp::Gt,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f32),
    Str(String),
    /// A constant a native's parameter type spells, such as `true` or a hash in hex.
    Literal(String),
    Var(Var),
    AddrOf(Box<Expr>),
    Deref(Box<Expr>),
    /// Element `index` of an array whose elements are `size` slots wide.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        size: u32,
    },
    /// The slot `offset` slots into a struct.
    Field {
        base: Box<Expr>,
        offset: i64,
    },
    DynField {
        base: Box<Expr>,
        offset: Box<Expr>,
    },
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Binary {
        op: BinOp,
        kind: Kind,
        l: Box<Expr>,
        r: Box<Expr>,
    },
    Cast {
        to: &'static str,
        e: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        /// The type a natives list states the result is.
        returns: Option<String>,
    },
    /// A value several slots wide, gathered from single slots.
    Tuple(Vec<Expr>),
    /// Slot `i` of a value `of` slots wide.
    Component {
        e: Box<Expr>,
        i: u32,
        of: u32,
    },
    /// What a pop past the bottom of the whole frame reads; counted as a stack mismatch.
    Missing,
}

impl Expr {
    pub fn bin(op: BinOp, kind: Kind, l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op,
            kind,
            l: Box::new(l),
            r: Box::new(r),
        }
    }

    pub fn call(name: impl Into<String>, args: Vec<Expr>) -> Expr {
        Expr::Call {
            name: name.into(),
            args,
            returns: None,
        }
    }

    pub fn deref(p: Expr) -> Expr {
        match p {
            Expr::AddrOf(x) => *x,
            p => Expr::Deref(Box::new(p)),
        }
    }

    pub fn addr_of(x: Expr) -> Expr {
        match x {
            Expr::Deref(p) => *p,
            x => Expr::AddrOf(Box::new(x)),
        }
    }

    /// The condition that holds exactly when `self` does not.
    pub fn negate(self) -> Expr {
        match self {
            Expr::Not(e) => *e,
            Expr::Binary {
                op,
                kind: Kind::Int,
                l,
                r,
            } if op.negated().is_some() => Expr::Binary {
                op: op.negated().unwrap_or(op),
                kind: Kind::Int,
                l,
                r,
            },
            Expr::Binary {
                op: op @ (BinOp::Eq | BinOp::Ne),
                kind,
                l,
                r,
            } => Expr::Binary {
                op: op.negated().unwrap_or(op),
                kind,
                l,
                r,
            },
            e => Expr::Not(Box::new(e)),
        }
    }

    /// Whether evaluating this calls a function or a native.
    pub fn has_call(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| found |= matches!(e, Expr::Call { .. }));
        found
    }

    pub fn mentions(&self, v: Var) -> bool {
        let mut found = false;
        self.walk(&mut |e| found |= *e == Expr::Var(v));
        found
    }

    pub fn walk(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Expr::AddrOf(e)
            | Expr::Deref(e)
            | Expr::Not(e)
            | Expr::Neg(e)
            | Expr::Cast { e, .. }
            | Expr::Component { e, .. } => e.walk(f),
            Expr::Field { base, .. } => base.walk(f),
            Expr::Index { base, index, .. } => {
                base.walk(f);
                index.walk(f);
            }
            Expr::DynField { base, offset } => {
                base.walk(f);
                offset.walk(f);
            }
            Expr::Binary { l, r, .. } => {
                l.walk(f);
                r.walk(f);
            }
            Expr::Call { args, .. } | Expr::Tuple(args) => {
                for a in args {
                    a.walk(f);
                }
            }
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Str(_)
            | Expr::Literal(_)
            | Expr::Var(_)
            | Expr::Missing => {}
        }
    }

    fn precedence(&self) -> u8 {
        match self {
            Expr::Binary { op, .. } => op.precedence(),
            Expr::Not(_) | Expr::Neg(_) | Expr::Cast { .. } | Expr::AddrOf(_) | Expr::Deref(_) => {
                11
            }
            Expr::Int(v) if *v < 0 => 11,
            Expr::Float(v) if *v < 0.0 => 11,
            Expr::Index { .. }
            | Expr::Field { .. }
            | Expr::DynField { .. }
            | Expr::Component { .. }
            | Expr::Call { .. } => 12,
            _ => 13,
        }
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, min: u8) {
        let parens = self.precedence() < min;
        if parens {
            out.push('(');
        }
        match self {
            Expr::Int(v) => {
                let _ = write!(out, "{v}");
            }
            Expr::Float(v) => {
                let _ = write!(out, "{v:?}f");
            }
            Expr::Str(s) => {
                let _ = write!(out, "{s:?}");
            }
            Expr::Literal(s) => out.push_str(s),
            Expr::Var(v) => {
                let _ = write!(out, "{v}");
            }
            Expr::AddrOf(e) => {
                out.push('&');
                e.write(out, 11);
            }
            Expr::Deref(e) => {
                out.push('*');
                e.write(out, 11);
            }
            Expr::Index { base, index, size } => {
                base.write(out, 12);
                out.push('[');
                index.write(out, 0);
                if *size != 1 {
                    let _ = write!(out, " /*{size}*/");
                }
                out.push(']');
            }
            Expr::Field { base, offset } => {
                write_member(out, base);
                if *offset < 0 {
                    let _ = write!(out, "f_n{}", offset.unsigned_abs());
                } else {
                    let _ = write!(out, "f_{offset}");
                }
            }
            Expr::DynField { base, offset } => {
                write_member(out, base);
                out.push_str("f_[");
                offset.write(out, 0);
                out.push(']');
            }
            Expr::Not(e) => {
                out.push('!');
                e.write(out, 11);
            }
            Expr::Neg(e) => {
                out.push('-');
                e.write(out, 11);
            }
            Expr::Binary { op, l, r, .. } => {
                let p = op.precedence();
                l.write(out, p);
                let _ = write!(out, " {} ", op.symbol());
                r.write(out, p + 1);
            }
            Expr::Cast { to, e } => {
                let _ = write!(out, "({to})");
                e.write(out, 11);
            }
            Expr::Call { name, args, .. } => {
                out.push_str(name);
                out.push('(');
                write_list(out, args);
                out.push(')');
            }
            Expr::Tuple(items) => {
                out.push('{');
                write_list(out, items);
                out.push('}');
            }
            Expr::Component { e, i, of } => {
                e.write(out, 12);
                match (of, i) {
                    (3, 0) => out.push_str(".x"),
                    (3, 1) => out.push_str(".y"),
                    (3, 2) => out.push_str(".z"),
                    _ => {
                        let _ = write!(out, ".s{i}");
                    }
                }
            }
            Expr::Missing => out.push_str("__missing"),
        }
        if parens {
            out.push(')');
        }
    }
}

/// `base.` or, through a pointer, `p->`.
fn write_member(out: &mut String, base: &Expr) {
    match base {
        Expr::Deref(p) => {
            p.write(out, 12);
            out.push_str("->");
        }
        base => {
            base.write(out, 12);
            out.push('.');
        }
    }
}

fn write_list(out: &mut String, items: &[Expr]) {
    for (n, item) in items.iter().enumerate() {
        if n > 0 {
            out.push_str(", ");
        }
        item.write(out, 0);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Assign(Expr, Expr),
    Expr(Expr),
    Return(Option<Expr>),
    If {
        cond: Expr,
        then: Vec<Stmt>,
        els: Vec<Stmt>,
    },
    /// `cond` is `None` for `while (true)`.
    While {
        cond: Option<Expr>,
        body: Vec<Stmt>,
    },
    DoWhile {
        body: Vec<Stmt>,
        cond: Expr,
    },
    /// `None` leaves that part of the head empty.
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Box<Stmt>,
        body: Vec<Stmt>,
    },
    Switch {
        value: Expr,
        cases: Vec<Case>,
    },
    Break,
    Continue,
    /// A jump to the label at a code offset.
    Goto(u32),
    /// The start of the block at a code offset; printed only when a goto names it.
    Label(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    /// `None` is `default`; no label at all is code ahead of the first case.
    pub labels: Vec<Option<i64>>,
    pub body: Vec<Stmt>,
}

pub fn label_name(at: u32) -> String {
    format!("label_{at:06X}")
}

/// Writes statements, printing only the labels a goto names.
pub struct Printer<'a> {
    pub out: String,
    pub labels: &'a HashSet<u32>,
}

impl Printer<'_> {
    fn line(&mut self, indent: usize, text: &str) {
        for _ in 0..indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// `stmts` without the labels no goto names, which print nothing.
    fn visible<'s>(&self, stmts: &'s [Stmt]) -> Vec<&'s Stmt> {
        stmts
            .iter()
            .filter(|s| !matches!(s, Stmt::Label(at) if !self.labels.contains(at)))
            .collect()
    }

    pub fn block(&mut self, stmts: &[Stmt], indent: usize) {
        for s in stmts {
            self.stmt(s, indent);
        }
    }

    fn stmt(&mut self, s: &Stmt, indent: usize) {
        match s {
            Stmt::Assign(..) | Stmt::Expr(_) => {
                let text = format!("{};", simple(s));
                self.line(indent, &text);
            }
            Stmt::Return(None) => self.line(indent, "return;"),
            Stmt::Return(Some(e)) => self.line(indent, &format!("return {};", e.text())),
            Stmt::If { cond, then, els } => {
                self.line(indent, &format!("if ({}) {{", cond.text()));
                self.block(then, indent + 1);
                let mut els = els;
                loop {
                    match self.visible(els).as_slice() {
                        [] => break,
                        [Stmt::If {
                            cond,
                            then,
                            els: next,
                        }] => {
                            self.line(indent, &format!("}} else if ({}) {{", cond.text()));
                            self.block(then, indent + 1);
                            els = next;
                        }
                        _ => {
                            self.line(indent, "} else {");
                            self.block(els, indent + 1);
                            break;
                        }
                    }
                }
                self.line(indent, "}");
            }
            Stmt::While { cond, body } => {
                let cond = cond.as_ref().map_or_else(|| "true".to_owned(), Expr::text);
                self.line(indent, &format!("while ({cond}) {{"));
                self.block(body, indent + 1);
                self.line(indent, "}");
            }
            Stmt::DoWhile { body, cond } => {
                self.line(indent, "do {");
                self.block(body, indent + 1);
                self.line(indent, &format!("}} while ({});", cond.text()));
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let init = init.as_deref().map(simple).unwrap_or_default();
                let cond = cond.as_ref().map(Expr::text).unwrap_or_default();
                let head = format!("for ({init}; {cond}; {}) {{", simple(step));
                self.line(indent, &head);
                self.block(body, indent + 1);
                self.line(indent, "}");
            }
            Stmt::Switch { value, cases } => {
                self.line(indent, &format!("switch ({}) {{", value.text()));
                for case in cases {
                    for label in &case.labels {
                        match label {
                            Some(v) => self.line(indent + 1, &format!("case {v}:")),
                            None => self.line(indent + 1, "default:"),
                        }
                    }
                    self.block(&case.body, indent + 2);
                }
                self.line(indent, "}");
            }
            Stmt::Break => self.line(indent, "break;"),
            Stmt::Continue => self.line(indent, "continue;"),
            Stmt::Goto(at) => self.line(indent, &format!("goto {};", label_name(*at))),
            Stmt::Label(at) => {
                if self.labels.contains(at) {
                    self.line(indent.saturating_sub(1), &format!("{}:", label_name(*at)));
                }
            }
        }
    }
}

/// An assignment or an expression statement, without its semicolon.
fn simple(s: &Stmt) -> String {
    match s {
        Stmt::Assign(l, r) => match r {
            Expr::Binary {
                op, l: rl, r: rr, ..
            } if op.compounds() && **rl == *l => {
                format!("{} {}= {}", l.text(), op.symbol(), rr.text())
            }
            r => format!("{} = {}", l.text(), r.text()),
        },
        Stmt::Expr(e) => e.text(),
        _ => String::new(),
    }
}

/// How many `continue`s in `stmts` belong to the loop whose body they are.
pub fn own_continues(stmts: &[Stmt]) -> usize {
    stmts
        .iter()
        .map(|s| match s {
            Stmt::Continue => 1,
            Stmt::If { then, els, .. } => own_continues(then) + own_continues(els),
            Stmt::Switch { cases, .. } => cases.iter().map(|c| own_continues(&c.body)).sum(),
            _ => 0,
        })
        .sum()
}

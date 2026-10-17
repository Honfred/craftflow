//! The compiled formula, and everything needed to evaluate it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::env::{Body, Environment};
use crate::error::{CompileError, EvalError, closest};
use crate::parser::{self, Ast};

/// How deep calls may nest before the engine gives up, which is what stops a
/// pack function that calls itself from blowing the stack.
pub const MAX_CALL_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

/// A formula that has been parsed and had its function names resolved.
///
/// Compiling once and evaluating many times is the point: the solver
/// re-evaluates machine formulas on every recalculation.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Const(f64),
    Var(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// The index is into the environment the formula was compiled against.
    Call(usize, Vec<Expr>),
}

/// Where a formula reads its variables from.
pub trait Variables {
    fn get(&self, name: &str) -> Option<f64>;
}

impl Variables for BTreeMap<String, f64> {
    fn get(&self, name: &str) -> Option<f64> {
        Self::get(self, name).copied()
    }
}

impl Variables for HashMap<String, f64> {
    fn get(&self, name: &str) -> Option<f64> {
        Self::get(self, name).copied()
    }
}

impl<F: Fn(&str) -> Option<f64>> Variables for F {
    fn get(&self, name: &str) -> Option<f64> {
        self(name)
    }
}

/// Several sources consulted in order, so a machine can offer its parameters,
/// its constants and the recipe's fields without merging them into one map.
pub struct Scope<'a> {
    sources: Vec<&'a dyn Variables>,
}

impl<'a> Scope<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, source: &'a dyn Variables) -> Self {
        self.sources.push(source);
        self
    }
}

impl Default for Scope<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Variables for Scope<'_> {
    fn get(&self, name: &str) -> Option<f64> {
        self.sources.iter().find_map(|source| source.get(name))
    }
}

/// Nothing at all, for formulas that are pure arithmetic.
pub struct NoVariables;

impl Variables for NoVariables {
    fn get(&self, _name: &str) -> Option<f64> {
        None
    }
}

/// Parses `source` and resolves its function calls against `env`.
///
/// Variables are *not* resolved here: which ones exist depends on the machine
/// and recipe a formula is used with. Call [`Expr::variables`] to find out
/// what a formula needs and check that separately.
pub fn compile(source: &str, env: &Environment) -> Result<Expr, CompileError> {
    let ast = parser::parse(source)?;
    resolve(&ast, env, None)
}

/// Like [`compile`], but also rejects variables outside `allowed`.
///
/// Used for pack function bodies, which may only see their own parameters.
pub fn compile_with_variables(
    source: &str,
    env: &Environment,
    allowed: &BTreeSet<String>,
) -> Result<Expr, CompileError> {
    let ast = parser::parse(source)?;
    resolve(&ast, env, Some(allowed))
}

fn resolve(
    ast: &Ast,
    env: &Environment,
    allowed: Option<&BTreeSet<String>>,
) -> Result<Expr, CompileError> {
    Ok(match ast {
        Ast::Number(value) => Expr::Const(*value),

        Ast::Var { name, span } => {
            if let Some(allowed) = allowed
                && !allowed.contains(name)
            {
                return Err(CompileError::UnknownVariable {
                    span: *span,
                    name: name.clone(),
                    suggestion: closest(name, allowed.iter().map(String::as_str)),
                });
            }
            Expr::Var(name.clone())
        }

        Ast::Unary { op, rhs } => Expr::Unary(*op, Box::new(resolve(rhs, env, allowed)?)),

        Ast::Binary { op, lhs, rhs } => Expr::Binary(
            *op,
            Box::new(resolve(lhs, env, allowed)?),
            Box::new(resolve(rhs, env, allowed)?),
        ),

        Ast::Ternary {
            cond,
            then,
            otherwise,
        } => Expr::Ternary(
            Box::new(resolve(cond, env, allowed)?),
            Box::new(resolve(then, env, allowed)?),
            Box::new(resolve(otherwise, env, allowed)?),
        ),

        Ast::Call { name, args, span } => {
            let Some(index) = env.index_of(name) else {
                return Err(CompileError::UnknownFunction {
                    span: *span,
                    name: name.clone(),
                    suggestion: closest(name, env.names()),
                });
            };

            let arity = env.arity(index);
            if !arity.accepts(args.len()) {
                return Err(CompileError::WrongArity {
                    span: *span,
                    name: name.clone(),
                    expected: arity,
                    got: args.len(),
                });
            }

            let args = args
                .iter()
                .map(|arg| resolve(arg, env, allowed))
                .collect::<Result<Vec<_>, _>>()?;
            Expr::Call(index, args)
        }
    })
}

/// Zero is false, anything else is true. NaN counts as false.
fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

const fn number(condition: bool) -> f64 {
    if condition { 1.0 } else { 0.0 }
}

impl Expr {
    /// Every variable this formula reads, so callers can check them against
    /// what a machine actually offers.
    #[must_use]
    pub fn variables(&self) -> BTreeSet<&str> {
        let mut out = BTreeSet::new();
        self.collect_variables(&mut out);
        out
    }

    fn collect_variables<'a>(&'a self, out: &mut BTreeSet<&'a str>) {
        match self {
            Self::Const(_) => {}
            Self::Var(name) => {
                out.insert(name.as_str());
            }
            Self::Unary(_, inner) => inner.collect_variables(out),
            Self::Binary(_, lhs, rhs) => {
                lhs.collect_variables(out);
                rhs.collect_variables(out);
            }
            Self::Ternary(cond, then, otherwise) => {
                cond.collect_variables(out);
                then.collect_variables(out);
                otherwise.collect_variables(out);
            }
            Self::Call(_, args) => {
                for arg in args {
                    arg.collect_variables(out);
                }
            }
        }
    }

    /// Evaluates the formula.
    ///
    /// `env` must be the same environment the formula was compiled against:
    /// calls are stored as indices into it.
    pub fn eval(&self, env: &Environment, vars: &dyn Variables) -> Result<f64, EvalError> {
        self.eval_at(env, vars, 0)
    }

    fn eval_at(
        &self,
        env: &Environment,
        vars: &dyn Variables,
        depth: usize,
    ) -> Result<f64, EvalError> {
        if depth > MAX_CALL_DEPTH {
            return Err(EvalError::TooDeep {
                limit: MAX_CALL_DEPTH,
            });
        }

        match self {
            Self::Const(value) => Ok(*value),

            Self::Var(name) => vars
                .get(name)
                .ok_or_else(|| EvalError::UnknownVariable { name: name.clone() }),

            Self::Unary(op, inner) => {
                let value = inner.eval_at(env, vars, depth)?;
                Ok(match op {
                    UnaryOp::Neg => -value,
                    UnaryOp::Not => number(!truthy(value)),
                })
            }

            Self::Binary(op, lhs, rhs) => {
                // `&&` and `||` short-circuit, so the right side is only
                // evaluated when it can change the answer.
                match op {
                    BinaryOp::And => {
                        return Ok(if truthy(lhs.eval_at(env, vars, depth)?) {
                            number(truthy(rhs.eval_at(env, vars, depth)?))
                        } else {
                            0.0
                        });
                    }
                    BinaryOp::Or => {
                        return Ok(if truthy(lhs.eval_at(env, vars, depth)?) {
                            1.0
                        } else {
                            number(truthy(rhs.eval_at(env, vars, depth)?))
                        });
                    }
                    _ => {}
                }

                let a = lhs.eval_at(env, vars, depth)?;
                let b = rhs.eval_at(env, vars, depth)?;
                Ok(match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => {
                        if b == 0.0 {
                            return Err(EvalError::DivisionByZero);
                        }
                        a / b
                    }
                    BinaryOp::Rem => {
                        if b == 0.0 {
                            return Err(EvalError::DivisionByZero);
                        }
                        a % b
                    }
                    BinaryOp::Pow => a.powf(b),
                    BinaryOp::Lt => number(a < b),
                    BinaryOp::Le => number(a <= b),
                    BinaryOp::Gt => number(a > b),
                    BinaryOp::Ge => number(a >= b),
                    BinaryOp::Eq => number(crate::approx_eq(a, b)),
                    BinaryOp::Ne => number(!crate::approx_eq(a, b)),
                    BinaryOp::And | BinaryOp::Or => unreachable!("handled above"),
                })
            }

            Self::Ternary(cond, then, otherwise) => {
                if truthy(cond.eval_at(env, vars, depth)?) {
                    then.eval_at(env, vars, depth)
                } else {
                    otherwise.eval_at(env, vars, depth)
                }
            }

            Self::Call(index, args) => {
                let values = args
                    .iter()
                    .map(|arg| arg.eval_at(env, vars, depth))
                    .collect::<Result<Vec<_>, _>>()?;

                let function = env.function(*index);
                match &function.body {
                    Body::Native(f) => f(&values),
                    Body::Expr { params, body } => {
                        // A pack function sees only its own parameters: it is
                        // a pure helper, not a closure over the machine.
                        let locals: BTreeMap<String, f64> =
                            params.iter().cloned().zip(values).collect();
                        body.eval_at(env, &locals, depth + 1)
                    }
                    Body::Pending => Err(EvalError::Native {
                        function: function.name.clone(),
                        message: "function body was never compiled".to_owned(),
                    }),
                }
            }
        }
    }
}

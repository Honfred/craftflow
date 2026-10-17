//! Formula engine: parsing, compilation and evaluation of pack expressions.
//!
//! Machines describe their speed and draw as formulas over their parameters,
//! their constants and the recipe's fields (DESIGN.md §4.3):
//!
//! ```
//! use expr::{Environment, NoVariables, compile};
//!
//! let env = Environment::with_builtins();
//! let formula = compile("max(1, 2) * 10", &env)?;
//! assert_eq!(formula.eval(&env, &NoVariables)?, 20.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! A formula is compiled once and evaluated many times, since the solver
//! re-runs machine formulas on every recalculation.
//!
//! # Grammar
//!
//! - numbers: `2`, `2.5`, `.5`, `1e-3`
//! - variables: `speed_upgrades`, `recipe.duration` (a dot is part of the
//!   name, not a field access)
//! - arithmetic: `+ - * / % ^`, with `^` right-associative
//! - comparison: `< <= > >= == !=`, yielding `1` or `0`
//! - logic: `&& || !`, short-circuiting, where any non-zero value is true
//! - conditional: `cond ? a : b`
//! - calls: `speed_factor(n)`
//!
//! Everything is `f64`; there is no separate boolean type.

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why. Tests are exempt.
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod env;
pub mod error;
pub mod expr;
mod lexer;
mod parser;

pub use env::{Arity, Environment, FunctionDef, NativeFn};
pub use error::{CompileError, EvalError, Span};
pub use expr::{
    BinaryOp, Expr, MAX_CALL_DEPTH, NoVariables, Scope, UnaryOp, Variables, compile,
    compile_with_variables,
};

/// Relative float comparison, used by the `==` and `!=` operators.
///
/// Deliberately the same rule as `model::approx_eq`; this crate sits at the
/// bottom of the dependency graph (DESIGN.md §3) and depends on nothing, so
/// the few lines are repeated rather than adding an edge.
pub(crate) fn approx_eq(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() <= 1e-9 * scale
}

//! The set of functions a formula may call.
//!
//! Two kinds live here: native functions, which the host registers as Rust
//! closures (this is how a Luau plugin's `register_function` arrives), and
//! expression functions, which packs declare in `functions.json` as named
//! expressions. Both are pure, as DESIGN.md §8.2 requires.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::error::{CompileError, EvalError, Span};
use crate::expr::{Expr, compile_with_variables};

/// How many arguments a function takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    Exact(usize),
    AtLeast(usize),
}

impl Arity {
    #[must_use]
    pub const fn accepts(self, count: usize) -> bool {
        match self {
            Self::Exact(n) => count == n,
            Self::AtLeast(n) => count >= n,
        }
    }
}

impl fmt::Display for Arity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (word, n) = match self {
            Self::Exact(n) => ("exactly", *n),
            Self::AtLeast(n) => ("at least", *n),
        };
        let plural = if n == 1 { "argument" } else { "arguments" };
        write!(f, "{word} {n} {plural}")
    }
}

/// A native function: pure, and free to fail with a message.
pub type NativeFn = Arc<dyn Fn(&[f64]) -> Result<f64, EvalError> + Send + Sync>;

pub(crate) enum Body {
    Native(NativeFn),
    Expr {
        params: Vec<String>,
        body: Expr,
    },
    /// Declared but not yet compiled, so that pack functions may refer to one
    /// another regardless of the order they appear in.
    Pending,
}

impl Clone for Body {
    fn clone(&self) -> Self {
        match self {
            Self::Native(f) => Self::Native(Arc::clone(f)),
            Self::Expr { params, body } => Self::Expr {
                params: params.clone(),
                body: body.clone(),
            },
            Self::Pending => Self::Pending,
        }
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(_) => f.write_str("Native(..)"),
            Self::Expr { params, .. } => write!(f, "Expr({})", params.join(", ")),
            Self::Pending => f.write_str("Pending"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Function {
    pub name: String,
    pub arity: Arity,
    pub body: Body,
}

/// A declaration of a pack function, before its body is compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: String,
}

impl FunctionDef {
    pub fn new(
        name: impl Into<String>,
        params: impl IntoIterator<Item = impl Into<String>>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            params: params.into_iter().map(Into::into).collect(),
            body: body.into(),
        }
    }
}

/// Functions available to the formulas compiled against it.
///
/// Compiled formulas store calls as indices into this environment, so a
/// formula must be evaluated against the same one it was compiled with.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    functions: Vec<Function>,
    index: HashMap<String, usize>,
}

impl Environment {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The usual arithmetic helpers, available to every pack.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut env = Self::new();
        env.add_builtins();
        env
    }

    fn add_builtins(&mut self) {
        // Registration only fails on a duplicate name, and this runs on an
        // environment that has no functions yet.
        let mut add = |name: &str, arity: Arity, f: NativeFn| {
            let index = self.functions.len();
            self.functions.push(Function {
                name: name.to_owned(),
                arity,
                body: Body::Native(f),
            });
            self.index.insert(name.to_owned(), index);
        };

        add("abs", Arity::Exact(1), Arc::new(|a: &[f64]| Ok(a[0].abs())));
        add(
            "floor",
            Arity::Exact(1),
            Arc::new(|a: &[f64]| Ok(a[0].floor())),
        );
        add(
            "ceil",
            Arity::Exact(1),
            Arc::new(|a: &[f64]| Ok(a[0].ceil())),
        );
        add(
            "round",
            Arity::Exact(1),
            Arc::new(|a: &[f64]| Ok(a[0].round())),
        );
        add(
            "sqrt",
            Arity::Exact(1),
            Arc::new(|a: &[f64]| Ok(a[0].sqrt())),
        );
        add("exp", Arity::Exact(1), Arc::new(|a: &[f64]| Ok(a[0].exp())));
        add("ln", Arity::Exact(1), Arc::new(|a: &[f64]| Ok(a[0].ln())));
        add(
            "log10",
            Arity::Exact(1),
            Arc::new(|a: &[f64]| Ok(a[0].log10())),
        );
        add(
            "pow",
            Arity::Exact(2),
            Arc::new(|a: &[f64]| Ok(a[0].powf(a[1]))),
        );
        add(
            "min",
            Arity::AtLeast(2),
            Arc::new(|a: &[f64]| Ok(a.iter().copied().fold(f64::INFINITY, f64::min))),
        );
        add(
            "max",
            Arity::AtLeast(2),
            Arc::new(|a: &[f64]| Ok(a.iter().copied().fold(f64::NEG_INFINITY, f64::max))),
        );
        add(
            "clamp",
            Arity::Exact(3),
            Arc::new(|a: &[f64]| Ok(a[0].clamp(a[1].min(a[2]), a[2].max(a[1])))),
        );
    }

    /// Registers a Rust closure, which is how plugin functions get in.
    ///
    /// The closure is only ever called with an argument count its `arity`
    /// accepts, which is checked at compile time.
    pub fn register_native(
        &mut self,
        name: impl Into<String>,
        arity: Arity,
        f: NativeFn,
    ) -> Result<(), CompileError> {
        let name = name.into();
        self.reserve(&name)?;
        self.push(Function {
            name,
            arity,
            body: Body::Native(f),
        });
        Ok(())
    }

    /// Compiles a batch of pack functions.
    ///
    /// Signatures are registered first and bodies compiled afterwards, so the
    /// definitions may appear in any order and may refer to one another.
    pub fn define_functions(
        &mut self,
        defs: impl IntoIterator<Item = FunctionDef>,
    ) -> Result<(), CompileError> {
        let defs: Vec<FunctionDef> = defs.into_iter().collect();

        let mut slots = Vec::with_capacity(defs.len());
        for def in &defs {
            self.reserve(&def.name)?;
            slots.push(self.push(Function {
                name: def.name.clone(),
                arity: Arity::Exact(def.params.len()),
                body: Body::Pending,
            }));
        }

        // Compiled against `&*self`, which already knows every signature.
        let mut bodies = Vec::with_capacity(defs.len());
        for def in &defs {
            let allowed: BTreeSet<String> = def.params.iter().cloned().collect();
            bodies.push(compile_with_variables(&def.body, self, &allowed)?);
        }

        for ((slot, def), body) in slots.into_iter().zip(&defs).zip(bodies) {
            if let Some(function) = self.functions.get_mut(slot) {
                function.body = Body::Expr {
                    params: def.params.clone(),
                    body,
                };
            }
        }

        Ok(())
    }

    /// Convenience for a single pack function.
    pub fn define_function(
        &mut self,
        name: impl Into<String>,
        params: impl IntoIterator<Item = impl Into<String>>,
        body: impl Into<String>,
    ) -> Result<(), CompileError> {
        self.define_functions([FunctionDef::new(name, params, body)])
    }

    fn reserve(&self, name: &str) -> Result<(), CompileError> {
        if self.index.contains_key(name) {
            return Err(CompileError::DuplicateFunction {
                span: Span::new(0, name.len()),
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    fn push(&mut self, function: Function) -> usize {
        let index = self.functions.len();
        self.index.insert(function.name.clone(), index);
        self.functions.push(function);
        index
    }

    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Every registered name, for error suggestions and for listing.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.functions.iter().map(|f| f.name.as_str())
    }

    /// Arity of the function at `index`.
    ///
    /// Indices only ever come from [`Self::index_of`], so the fallback is
    /// unreachable in practice.
    #[must_use]
    pub fn arity(&self, index: usize) -> Arity {
        self.functions
            .get(index)
            .map_or(Arity::Exact(0), |f| f.arity)
    }

    pub(crate) fn function(&self, index: usize) -> &Function {
        self.functions.get(index).unwrap_or(&MISSING)
    }
}

/// Stand-in for an index that does not resolve, which cannot happen for a
/// formula evaluated against the environment it was compiled with.
static MISSING: Function = Function {
    name: String::new(),
    arity: Arity::Exact(0),
    body: Body::Pending,
};

//! End-to-end tests for the formula engine.

use std::collections::BTreeMap;
use std::sync::Arc;

use expr::{
    Arity, CompileError, Environment, EvalError, FunctionDef, NoVariables, Scope, Variables,
    compile,
};

fn eval(source: &str) -> f64 {
    let env = Environment::with_builtins();
    let formula = compile(source, &env).expect("compiles");
    formula.eval(&env, &NoVariables).expect("evaluates")
}

fn vars(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs
        .iter()
        .map(|&(name, value)| (name.to_owned(), value))
        .collect()
}

#[test]
fn arithmetic_precedence() {
    assert_eq!(eval("2 + 3 * 4"), 14.0);
    assert_eq!(eval("(2 + 3) * 4"), 20.0);
    assert_eq!(eval("10 - 4 - 3"), 3.0);
    assert_eq!(eval("10 / 2 / 5"), 1.0);
    assert_eq!(eval("7 % 4"), 3.0);
}

#[test]
fn power_is_right_associative_and_binds_tighter_than_unary_minus() {
    assert_eq!(eval("2 ^ 3 ^ 2"), 512.0);
    assert_eq!(eval("-2 ^ 2"), -4.0);
    assert_eq!(eval("(-2) ^ 2"), 4.0);
    assert_eq!(eval("2 ^ -1"), 0.5);
}

#[test]
fn comparisons_and_logic_yield_one_or_zero() {
    assert_eq!(eval("3 < 4"), 1.0);
    assert_eq!(eval("3 >= 4"), 0.0);
    assert_eq!(eval("1 && 0"), 0.0);
    assert_eq!(eval("0 || 2"), 1.0);
    assert_eq!(eval("!0"), 1.0);
    assert_eq!(eval("!5"), 0.0);
}

#[test]
fn equality_tolerates_float_noise() {
    assert_eq!(eval("0.1 + 0.2 == 0.3"), 1.0);
    assert_eq!(eval("1 != 1.001"), 1.0);
}

#[test]
fn ternary_is_lowest_precedence_and_right_associative() {
    assert_eq!(eval("1 > 0 ? 10 : 20"), 10.0);
    assert_eq!(eval("1 > 2 ? 10 : 20"), 20.0);
    // The condition swallows the whole `||` expression.
    assert_eq!(eval("0 || 1 ? 10 : 20"), 10.0);
    assert_eq!(eval("0 ? 1 : 0 ? 2 : 3"), 3.0);
}

#[test]
fn logic_short_circuits_past_a_failing_branch() {
    // Dividing by zero is an error, so this only passes if `&&` stops early.
    assert_eq!(eval("0 && (1 / 0)"), 0.0);
    assert_eq!(eval("1 || (1 / 0)"), 1.0);
}

#[test]
fn builtins_work() {
    assert_eq!(eval("min(3, 1, 2)"), 1.0);
    assert_eq!(eval("max(3, 1, 2)"), 3.0);
    assert_eq!(eval("abs(-4)"), 4.0);
    assert_eq!(eval("floor(2.7)"), 2.0);
    assert_eq!(eval("ceil(2.1)"), 3.0);
    assert_eq!(eval("round(2.5)"), 3.0);
    assert_eq!(eval("sqrt(9)"), 3.0);
    assert_eq!(eval("pow(2, 10)"), 1024.0);
    assert_eq!(eval("clamp(15, 0, 8)"), 8.0);
}

#[test]
fn variables_come_from_the_caller() {
    let env = Environment::with_builtins();
    let formula = compile("speed_upgrades * 2 + base_energy", &env).expect("compiles");
    let values = vars(&[("speed_upgrades", 4.0), ("base_energy", 50.0)]);
    assert_eq!(formula.eval(&env, &values).expect("evaluates"), 58.0);
}

#[test]
fn dotted_names_are_plain_variables() {
    let env = Environment::with_builtins();
    let formula = compile("recipe.duration / 2", &env).expect("compiles");
    let values = vars(&[("recipe.duration", 200.0)]);
    assert_eq!(formula.eval(&env, &values).expect("evaluates"), 100.0);
}

#[test]
fn a_scope_chains_several_sources() {
    let env = Environment::with_builtins();
    let formula = compile("base_energy * speed_upgrades", &env).expect("compiles");

    let params = vars(&[("speed_upgrades", 3.0)]);
    let constants = vars(&[("base_energy", 50.0)]);
    let scope = Scope::new().with(&params).with(&constants);

    assert_eq!(formula.eval(&env, &scope).expect("evaluates"), 150.0);
}

#[test]
fn variables_can_be_listed_before_evaluating() {
    let env = Environment::with_builtins();
    let formula = compile("max(a, b) + a * recipe.duration", &env).expect("compiles");
    let names: Vec<&str> = formula.variables().into_iter().collect();
    assert_eq!(names, ["a", "b", "recipe.duration"]);
}

#[test]
fn a_missing_variable_is_an_evaluation_error() {
    let env = Environment::with_builtins();
    let formula = compile("a + b", &env).expect("compiles");
    let values = vars(&[("a", 1.0)]);
    assert_eq!(
        formula.eval(&env, &values),
        Err(EvalError::UnknownVariable {
            name: "b".to_owned()
        })
    );
}

#[test]
fn pack_functions_are_named_expressions() {
    let mut env = Environment::with_builtins();
    env.define_function("speed_factor", ["n"], "1 / (1 + n * 0.5)")
        .expect("defines");

    let formula =
        compile("recipe.duration * speed_factor(speed_upgrades)", &env).expect("compiles");
    let values = vars(&[("recipe.duration", 200.0), ("speed_upgrades", 2.0)]);
    assert_eq!(formula.eval(&env, &values).expect("evaluates"), 100.0);
}

#[test]
fn pack_functions_may_be_defined_in_any_order() {
    let mut env = Environment::with_builtins();
    env.define_functions([
        FunctionDef::new("outer", ["n"], "inner(n) * 10"),
        FunctionDef::new("inner", ["n"], "n + 1"),
    ])
    .expect("defines");

    let formula = compile("outer(2)", &env).expect("compiles");
    assert_eq!(formula.eval(&env, &NoVariables).expect("evaluates"), 30.0);
}

#[test]
fn a_pack_function_sees_only_its_parameters() {
    let mut env = Environment::with_builtins();
    let err = env
        .define_function("bad", ["n"], "n * base_energy")
        .expect_err("rejects");

    assert!(
        matches!(&err, CompileError::UnknownVariable { name, .. } if name == "base_energy"),
        "got {err:?}"
    );
}

#[test]
fn native_functions_cover_the_plugin_case() {
    let mut env = Environment::with_builtins();
    env.register_native(
        "energy_factor",
        Arity::Exact(2),
        Arc::new(|args: &[f64]| Ok(args[0] * 2.0 - args[1])),
    )
    .expect("registers");

    let formula = compile(
        "base_energy * energy_factor(speed_upgrades, energy_upgrades)",
        &env,
    )
    .expect("compiles");
    let values = vars(&[
        ("base_energy", 50.0),
        ("speed_upgrades", 3.0),
        ("energy_upgrades", 1.0),
    ]);
    assert_eq!(formula.eval(&env, &values).expect("evaluates"), 250.0);
}

#[test]
fn a_native_function_may_fail() {
    let mut env = Environment::with_builtins();
    env.register_native(
        "boom",
        Arity::Exact(0),
        Arc::new(|_: &[f64]| {
            Err(EvalError::Native {
                function: "boom".to_owned(),
                message: "no".to_owned(),
            })
        }),
    )
    .expect("registers");

    let formula = compile("boom()", &env).expect("compiles");
    assert!(matches!(
        formula.eval(&env, &NoVariables),
        Err(EvalError::Native { .. })
    ));
}

#[test]
fn unknown_functions_suggest_the_closest_name() {
    let mut env = Environment::with_builtins();
    env.define_function("speed_factor", ["n"], "n")
        .expect("defines");

    let err = compile("speed_factr(1)", &env).expect_err("rejects");
    let CompileError::UnknownFunction {
        name, suggestion, ..
    } = &err
    else {
        panic!("got {err:?}");
    };
    assert_eq!(name, "speed_factr");
    assert_eq!(suggestion.as_deref(), Some("speed_factor"));
}

#[test]
fn wrong_argument_counts_are_caught_at_compile_time() {
    let mut env = Environment::with_builtins();
    env.define_function("speed_factor", ["n"], "n")
        .expect("defines");

    let err = compile("speed_factor(1, 2)", &env).expect_err("rejects");
    assert!(
        matches!(&err, CompileError::WrongArity { name, got: 2, .. } if name == "speed_factor"),
        "got {err:?}"
    );

    let err = compile("min(1)", &env).expect_err("rejects");
    assert!(matches!(err, CompileError::WrongArity { got: 1, .. }));
}

#[test]
fn division_by_zero_is_reported() {
    let env = Environment::with_builtins();
    let formula = compile("1 / 0", &env).expect("compiles");
    assert_eq!(
        formula.eval(&env, &NoVariables),
        Err(EvalError::DivisionByZero)
    );

    let formula = compile("1 % 0", &env).expect("compiles");
    assert_eq!(
        formula.eval(&env, &NoVariables),
        Err(EvalError::DivisionByZero)
    );
}

#[test]
fn runaway_recursion_stops() {
    let mut env = Environment::with_builtins();
    env.define_function("forever", ["n"], "forever(n)")
        .expect("defines");

    let formula = compile("forever(1)", &env).expect("compiles");
    assert!(matches!(
        formula.eval(&env, &NoVariables),
        Err(EvalError::TooDeep { .. })
    ));
}

#[test]
fn duplicate_definitions_are_refused() {
    let mut env = Environment::with_builtins();
    env.define_function("f", ["n"], "n").expect("defines");
    assert!(matches!(
        env.define_function("f", ["n"], "n + 1"),
        Err(CompileError::DuplicateFunction { .. })
    ));
    // A builtin cannot be shadowed either.
    assert!(matches!(
        env.define_function("min", ["a", "b"], "a"),
        Err(CompileError::DuplicateFunction { .. })
    ));
}

#[test]
fn syntax_errors_point_at_the_problem() {
    let env = Environment::with_builtins();

    let err = compile("2 +", &env).expect_err("rejects");
    assert!(
        matches!(err, CompileError::UnexpectedEnd { .. }),
        "got {err:?}"
    );

    let err = compile("(2 + 3", &env).expect_err("rejects");
    assert!(
        matches!(err, CompileError::UnexpectedEnd { .. }),
        "got {err:?}"
    );

    let err = compile("2 3", &env).expect_err("rejects");
    assert!(
        matches!(err, CompileError::UnexpectedToken { .. }),
        "got {err:?}"
    );

    let err = compile("", &env).expect_err("rejects");
    assert!(matches!(err, CompileError::Empty { .. }), "got {err:?}");

    let err = compile("1 ? 2", &env).expect_err("rejects");
    assert!(
        matches!(err, CompileError::UnexpectedEnd { .. }),
        "got {err:?}"
    );
}

#[test]
fn the_error_report_underlines_the_offending_text() {
    let mut env = Environment::with_builtins();
    env.define_function("speed_factor", ["n"], "n")
        .expect("defines");

    let source = "recipe.duration * speed_factr(n)";
    let err = compile(source, &env).expect_err("rejects");
    let report = err.report(source);

    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(lines[0], source);
    // The carets sit under `speed_factr(n)`, which starts at column 19.
    assert_eq!(lines[1].len() - lines[1].trim_start().len(), 18);
    assert!(lines[1].trim().starts_with("^^^"));
    assert!(
        lines[2].contains("column 19") && lines[2].contains("did you mean `speed_factor`"),
        "got {}",
        lines[2]
    );
}

#[test]
fn the_design_machine_formulas_compile_and_run() {
    // DESIGN.md §4.3, with plausible stand-ins for the Mekanism curves.
    let mut env = Environment::with_builtins();
    env.define_functions([
        FunctionDef::new("speed_factor", ["n"], "1 / (1 + n * 0.5)"),
        FunctionDef::new(
            "energy_factor",
            ["speed", "energy"],
            "pow(2, speed) / pow(2, energy)",
        ),
    ])
    .expect("defines");

    let duration =
        compile("recipe.duration * speed_factor(speed_upgrades)", &env).expect("compiles");
    let energy = compile(
        "base_energy * energy_factor(speed_upgrades, energy_upgrades)",
        &env,
    )
    .expect("compiles");

    let params = vars(&[("speed_upgrades", 2.0), ("energy_upgrades", 1.0)]);
    let constants = vars(&[("base_energy", 50.0)]);
    let recipe = vars(&[("recipe.duration", 200.0)]);
    let scope = Scope::new().with(&params).with(&constants).with(&recipe);

    assert_eq!(duration.eval(&env, &scope).expect("evaluates"), 100.0);
    assert_eq!(energy.eval(&env, &scope).expect("evaluates"), 100.0);
}

#[test]
fn a_closure_can_supply_variables() {
    let env = Environment::with_builtins();
    let formula = compile("a + b", &env).expect("compiles");
    let lookup = |name: &str| match name {
        "a" => Some(1.5),
        "b" => Some(2.5),
        _ => None,
    };
    assert_eq!(
        formula
            .eval(&env, &lookup as &dyn Variables)
            .expect("evaluates"),
        4.0
    );
}

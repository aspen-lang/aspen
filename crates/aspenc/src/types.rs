//! Bidirectional typing with source evidence kept separate from semantic types.

use std::{fmt, rc::Rc};

use crate::{Expr, Loc, Pattern, Program, Span};

/// The sole currently expressible structural actor type is the empty structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorType {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Never,
    Actor(ActorType),
    Any,
}

impl Type {
    pub const UNIT: Self = Self::Actor(ActorType {});

    pub fn is_subtype_of(self, expected: Self) -> bool {
        self == expected || self == Self::Never || expected == Self::Any
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Never => "never",
            Self::Actor(_) => "{}",
            Self::Any => "any",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    pub ty: Type,
    /// The pattern node or other source site imposing this requirement.
    pub origin: Span,
}

/// A result's origin is explicit: expression and pattern trees need not align.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeEvidence {
    pub ty: Type,
    pub expression: Span,
    pub result_from: Option<Rc<TypeEvidence>>,
}

impl TypeEvidence {
    pub fn producer(&self) -> &Self {
        match &self.result_from {
            Some(source) => source.producer(),
            None => self,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub pattern: Span,
    pub value: Rc<TypeEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedExpression {
    pub evidence: Rc<TypeEvidence>,
    pub kind: TypedExprKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedExprKind {
    Actor,
    Variable {
        binding: Binding,
    },
    Let {
        value: Box<TypedExpression>,
        /// These bindings are in scope only in `body`, not `value`.
        bindings: Vec<Binding>,
        body: Box<TypedExpression>,
    },
}

/// Component traversal is empty today. Future structural comparisons append
/// component steps while retaining the corresponding source evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeMismatch {
    pub expected: Expectation,
    pub actual: Rc<TypeEvidence>,
    pub comparison_path: Vec<String>,
}

impl fmt::Display for TypeMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected {}, found {}", self.expected.ty, self.actual.ty)
    }
}

impl std::error::Error for TypeMismatch {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeError {
    Mismatch(TypeMismatch),
    UnboundVariable { name: String, span: Span },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch(mismatch) => mismatch.fmt(f),
            Self::UnboundVariable { name, .. } => write!(f, "unbound variable {name:?}"),
        }
    }
}

impl std::error::Error for TypeError {}

pub type TypeResult<T> = Result<T, TypeError>;

/// Innermost bindings take precedence. Extension never mutates an outer scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    bindings: Vec<Binding>,
}

impl Environment {
    pub fn lookup(&self, name: &str) -> Option<&Binding> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name)
    }

    pub fn extended(&self, bindings: impl IntoIterator<Item = Binding>) -> Self {
        let mut scope = self.clone();
        scope.bindings.extend(bindings);
        scope
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternPlan {
    pub expectation: Expectation,
    variable: Option<String>,
}

pub fn prepare_pattern(pattern: &Loc<Pattern>) -> PatternPlan {
    PatternPlan {
        expectation: Expectation {
            ty: Type::Any,
            origin: pattern.span,
        },
        variable: match &pattern.value {
            Pattern::Discard => None,
            Pattern::Variable(name) => Some(name.clone()),
        },
    }
}

impl PatternPlan {
    /// Recheck the contract so callers cannot bind an incompatible typed value.
    pub fn apply(&self, value: &TypedExpression) -> TypeResult<Vec<Binding>> {
        assert_type(value, &self.expectation)?;
        Ok(self
            .variable
            .iter()
            .map(|name| Binding {
                name: name.clone(),
                pattern: self.expectation.origin,
                value: Rc::clone(&value.evidence),
            })
            .collect())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedBinding {
    pub value: TypedExpression,
    pub bindings: Vec<Binding>,
}

pub fn check_binding(
    environment: &Environment,
    pattern: &Loc<Pattern>,
    expression: &Loc<Expr>,
) -> TypeResult<CheckedBinding> {
    let plan = prepare_pattern(pattern);
    let value = check_in(environment, expression, &plan.expectation)?;
    let bindings = plan.apply(&value)?;
    Ok(CheckedBinding { value, bindings })
}

pub fn synthesize(expression: &Loc<Expr>) -> TypeResult<TypedExpression> {
    synthesize_in(&Environment::default(), expression)
}

/// Return the actual, narrower type and its evidence, not the expected type.
pub fn check(expression: &Loc<Expr>, expected: &Expectation) -> TypeResult<TypedExpression> {
    check_in(&Environment::default(), expression, expected)
}

pub fn synthesize_in(
    environment: &Environment,
    expression: &Loc<Expr>,
) -> TypeResult<TypedExpression> {
    type_expression(environment, expression, None)
}

pub fn check_in(
    environment: &Environment,
    expression: &Loc<Expr>,
    expected: &Expectation,
) -> TypeResult<TypedExpression> {
    type_expression(environment, expression, Some(expected))
}

fn assert_type(value: &TypedExpression, expected: &Expectation) -> TypeResult<()> {
    if value.evidence.ty.is_subtype_of(expected.ty) {
        Ok(())
    } else {
        Err(TypeError::Mismatch(TypeMismatch {
            expected: expected.clone(),
            actual: Rc::clone(&value.evidence),
            comparison_path: Vec::new(),
        }))
    }
}

fn type_expression(
    environment: &Environment,
    expression: &Loc<Expr>,
    expected: Option<&Expectation>,
) -> TypeResult<TypedExpression> {
    let (ty, result_from, kind) = match &expression.value {
        Expr::Actor(_) => (Type::UNIT, None, TypedExprKind::Actor),
        Expr::Variable(name) => {
            let binding = environment
                .lookup(name)
                .ok_or_else(|| TypeError::UnboundVariable {
                    name: name.clone(),
                    span: expression.span,
                })?;
            (
                binding.value.ty,
                Some(Rc::clone(&binding.value)),
                TypedExprKind::Variable {
                    binding: binding.clone(),
                },
            )
        }
        Expr::Let(binding) => {
            let CheckedBinding { value, bindings } =
                check_binding(environment, &binding.pattern, &binding.value)?;
            // A strict let's unreachable body is still synthesized for diagnostics,
            // but it must not determine the result type or inherit its expectation.
            let diverges = value.evidence.ty == Type::Never;
            let body_scope = environment.extended(bindings.iter().cloned());
            let body = type_expression(
                &body_scope,
                &binding.body,
                if diverges { None } else { expected },
            )?;
            let result = Rc::clone(if diverges {
                &value.evidence
            } else {
                &body.evidence
            });
            (
                result.ty,
                Some(result),
                TypedExprKind::Let {
                    value: Box::new(value),
                    bindings,
                    body: Box::new(body),
                },
            )
        }
    };
    let typed = TypedExpression {
        evidence: Rc::new(TypeEvidence {
            ty,
            expression: expression.span,
            result_from,
        }),
        kind,
    };
    if let Some(expected) = expected {
        assert_type(&typed, expected)?;
    }
    Ok(typed)
}

/// Type-check a successfully parsed program; parse diagnostics remain separate.
pub fn check_program(program: &Program) -> TypeResult<Vec<TypedExpression>> {
    program.expressions.iter().map(synthesize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lexer, Pos, parse};

    fn expression(source: &str) -> Loc<Expr> {
        let mut diagnostics = Vec::new();
        let mut program = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        program.expressions.remove(0)
    }

    fn site() -> Span {
        Span {
            start: Pos { line: 8, col: 2 },
            end: Pos { line: 8, col: 3 },
        }
    }

    #[test]
    fn references_preserve_use_declaration_and_value_sites() {
        let typed = synthesize(&expression("let x = {}. x")).unwrap();
        let TypedExprKind::Let { body, .. } = &typed.kind else {
            panic!()
        };
        let TypedExprKind::Variable { binding } = &body.kind else {
            panic!()
        };
        assert_eq!(body.evidence.ty, Type::UNIT);
        assert_eq!(body.evidence.expression.start.col, 13);
        assert_eq!(binding.pattern.start.col, 5);
        assert_eq!(body.evidence.producer().expression.start.col, 9);
        let TypeError::Mismatch(error) = check(
            &expression("let x = {}. x"),
            &Expectation {
                ty: Type::Never,
                origin: site(),
            },
        )
        .unwrap_err() else {
            panic!()
        };
        assert_eq!(error.actual.expression.start.col, 13);
        assert_eq!(error.actual.producer().expression.start.col, 9);
        assert_eq!(error.expected.origin, site());
    }

    #[test]
    fn unbound_names_and_nonrecursive_scope() {
        for source in [
            "x",
            "let x = x. x",
            "let _ = {}. x",
            "let _ = let x = {}. x. x",
        ] {
            let TypeError::UnboundVariable { name, span } =
                synthesize(&expression(source)).unwrap_err()
            else {
                panic!("{source}")
            };
            assert_eq!(name, "x");
            assert_eq!(span.end.col, span.start.col + 1);
        }
    }

    #[test]
    fn shadowing_initializer_sees_outer_binding() {
        let typed = synthesize(&expression("let x = {}. let x = x. x")).unwrap();
        let TypedExprKind::Let {
            body,
            bindings: outer,
            ..
        } = typed.kind
        else {
            panic!()
        };
        let TypedExprKind::Let {
            value,
            body,
            bindings: inner,
        } = body.kind
        else {
            panic!()
        };
        let TypedExprKind::Variable {
            binding: read_outer,
        } = value.kind
        else {
            panic!()
        };
        let TypedExprKind::Variable {
            binding: read_inner,
        } = body.kind
        else {
            panic!()
        };
        assert_eq!(read_outer.pattern, outer[0].pattern);
        assert_eq!(read_inner.pattern, inner[0].pattern);
        assert_ne!(read_outer.pattern, read_inner.pattern);
    }

    #[test]
    fn scoped_environment_and_never_initializer() {
        let outer = Environment::default().extended([Binding {
            name: "x".into(),
            pattern: site(),
            value: Rc::new(TypeEvidence {
                ty: Type::Never,
                expression: site(),
                result_from: None,
            }),
        }]);
        let typed = check_in(
            &outer,
            &expression("let x = x. {}"),
            &Expectation {
                ty: Type::Never,
                origin: site(),
            },
        )
        .unwrap();
        assert_eq!(typed.evidence.ty, Type::Never);
        assert_eq!(outer.lookup("x").unwrap().value.ty, Type::Never);
        assert!(matches!(
            synthesize_in(&outer, &expression("let _ = x. missing")),
            Err(TypeError::UnboundVariable { .. })
        ));
        let typed = synthesize(&expression("let x = {}. let _ = let x = {}. x. x")).unwrap();
        let TypedExprKind::Let { bindings, body, .. } = typed.kind else {
            panic!()
        };
        let TypedExprKind::Let { body, .. } = body.kind else {
            panic!()
        };
        let TypedExprKind::Variable { binding } = body.kind else {
            panic!()
        };
        assert_eq!(binding.pattern, bindings[0].pattern);
    }

    #[test]
    fn entire_lattice() {
        let types = [Type::Never, Type::UNIT, Type::Any];
        for (i, actual) in types.iter().enumerate() {
            for (j, expected) in types.iter().enumerate() {
                assert_eq!(actual.is_subtype_of(*expected), i <= j);
            }
        }
        assert_eq!(Type::UNIT.to_string(), "{}");
    }

    #[test]
    fn checking_preserves_precision() {
        for ty in [Type::Any, Type::UNIT] {
            let typed = check(&expression("{}"), &Expectation { ty, origin: site() }).unwrap();
            assert_eq!(typed.evidence.ty, Type::UNIT);
        }
    }

    #[test]
    fn variables_bind_precise_types_and_preserve_both_origins() {
        let ast = expression("let value = let _ = {}. {}. {}");
        let typed = synthesize(&ast).unwrap();
        let TypedExprKind::Let {
            value,
            bindings,
            body,
        } = &typed.kind
        else {
            panic!()
        };
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].name, "value");
        assert_eq!(bindings[0].pattern.start.col, 5);
        assert_eq!(bindings[0].pattern.end.col, 10);
        assert_eq!(bindings[0].value.ty, Type::UNIT);
        assert!(Rc::ptr_eq(&bindings[0].value, &value.evidence));
        assert_eq!(bindings[0].value.producer().expression.start.col, 25);
        assert_eq!(typed.evidence.producer(), body.evidence.producer());
        let TypedExprKind::Let { bindings, .. } = &value.kind else {
            panic!()
        };
        assert!(bindings.is_empty());
    }

    #[test]
    fn checking_let_reports_body_and_expected_origin() {
        let ast = expression("let x = {}. let y = {}. {}");
        let expected = Expectation {
            ty: Type::Never,
            origin: site(),
        };
        let TypeError::Mismatch(mismatch) = check(&ast, &expected).unwrap_err() else {
            panic!()
        };
        assert_eq!(mismatch.expected, expected);
        assert_eq!(mismatch.actual.ty, Type::UNIT);
        assert_eq!(mismatch.actual.producer().expression.start.col, 25);
        assert!(mismatch.comparison_path.is_empty());
        assert_eq!(mismatch.to_string(), "expected never, found {}");
    }

    #[test]
    fn pattern_contract_mismatches_retain_pattern_and_value() {
        let pattern = Loc {
            value: Pattern::Variable("x".into()),
            span: site(),
        };
        let mut plan = prepare_pattern(&pattern);
        assert_eq!(plan.expectation.ty, Type::Any);
        plan.expectation.ty = Type::Never;
        let typed = synthesize(&expression("let _ = {}. {} ")).unwrap();
        let TypeError::Mismatch(mismatch) = plan.apply(&typed).unwrap_err() else {
            panic!()
        };
        assert_eq!(mismatch.expected.origin, pattern.span);
        assert_eq!(mismatch.actual.producer().expression.start.col, 13);
    }

    #[test]
    fn bindings_remain_local_to_their_let() {
        let typed = synthesize(&expression("let x = let x = {}. {}. let x = {}. {}")).unwrap();
        let TypedExprKind::Let {
            bindings,
            value,
            body,
        } = typed.kind
        else {
            panic!()
        };
        assert_eq!(bindings.len(), 1);
        for nested in [value, body] {
            let TypedExprKind::Let { bindings, .. } = nested.kind else {
                panic!()
            };
            assert_eq!(bindings.len(), 1);
            assert_eq!(bindings[0].name, "x");
        }
    }
}

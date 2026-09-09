//! Bidirectional typing with source evidence kept separate from semantic types.

use std::{fmt, rc::Rc};

use crate::{Expr, Loc, Pattern, Program, Span};

mod semantic;
pub use semantic::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    pub ty: Type,
    /// The pattern node or other source site imposing this requirement.
    pub origin: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceOrigin {
    Expression,
    ReceiverPattern,
}

/// A result's origin is explicit: expression and pattern trees need not align.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeEvidence {
    pub ty: Type,
    /// The originating source span; receiver values originate at their pattern.
    pub expression: Span,
    pub origin: EvidenceOrigin,
    pub result_from: Option<Rc<TypeEvidence>>,
    /// Actor components live at the producer, including through lexical aliases.
    pub methods: Vec<MethodEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodEvidence {
    pub parameters: Vec<LocatedParameter>,
    pub span: Span,
    pub input: Expectation,
    pub output: Rc<TypeEvidence>,
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
    Actor {
        methods: Vec<TypedMethod>,
    },
    Variable {
        binding: Binding,
    },
    Let {
        value: Box<TypedExpression>,
        instantiations: Vec<ParameterInstantiation>,
        /// These bindings are in scope only in `body`, not `value`.
        bindings: Vec<Binding>,
        body: Box<TypedExpression>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedMethod {
    pub parameters: Vec<LocatedParameter>,
    pub span: Span,
    pub pattern: Span,
    pub expectation: Expectation,
    pub bindings: Vec<Binding>,
    pub body: Box<TypedExpression>,
}

/// A failed structural comparison retains the full evidence tree. The path
/// identifies an unsatisfied expected method and one representative candidate.
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

/// Parameter declarations keep their source origins outside semantic types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedParameter {
    pub parameter: TypeParameter,
    pub origin: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingTemplate {
    Discard,
    Variable {
        name: String,
        parameter: TypeVariable,
        origin: Span,
    },
}

/// Recursive patterns will compose child plans and concatenate their parameters.
/// Quantifier scope belongs to the consumer, never to an individual child plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternPlan {
    pub parameters: Vec<LocatedParameter>,
    pub expectation: Expectation,
    pub template: BindingTemplate,
}

pub fn prepare_pattern(pattern: &Loc<Pattern>) -> PatternPlan {
    match &pattern.value {
        Pattern::Discard => PatternPlan {
            parameters: Vec::new(),
            expectation: Expectation {
                ty: Type::Any,
                origin: pattern.span,
            },
            template: BindingTemplate::Discard,
        },
        Pattern::Variable(name) => {
            let variable = TypeVariable::fresh(Type::Any);
            PatternPlan {
                parameters: vec![LocatedParameter {
                    parameter: TypeParameter {
                        variable: variable.clone(),
                    },
                    origin: pattern.span,
                }],
                expectation: Expectation {
                    ty: Type::Variable(variable.clone()),
                    origin: pattern.span,
                },
                template: BindingTemplate::Variable {
                    name: name.clone(),
                    parameter: variable,
                    origin: pattern.span,
                },
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterInstantiation {
    pub parameter: LocatedParameter,
    pub actual: Rc<TypeEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternInstantiation {
    pub arguments: Vec<ParameterInstantiation>,
    pub bindings: Vec<Binding>,
}

impl PatternPlan {
    /// Let matching checks a bound, then chooses the actual type, not any
    /// arbitrary solution of `actual <: parameter <: bound`.
    pub fn instantiate(&self, value: &TypedExpression) -> TypeResult<PatternInstantiation> {
        match &self.template {
            BindingTemplate::Discard => {
                assert_type(value, &self.expectation)?;
                Ok(PatternInstantiation {
                    arguments: Vec::new(),
                    bindings: Vec::new(),
                })
            }
            BindingTemplate::Variable {
                name,
                parameter,
                origin,
            } => {
                let declaration = self
                    .parameters
                    .iter()
                    .find(|p| p.parameter.variable == *parameter)
                    .expect("binding template parameter must be declared in its plan");
                let expected = Expectation {
                    ty: (*parameter.upper_bound).clone(),
                    origin: *origin,
                };
                assert_type(value, &expected)?;
                Ok(PatternInstantiation {
                    arguments: vec![ParameterInstantiation {
                        parameter: declaration.clone(),
                        actual: Rc::clone(&value.evidence),
                    }],
                    bindings: vec![Binding {
                        name: name.clone(),
                        pattern: *origin,
                        value: Rc::clone(&value.evidence),
                    }],
                })
            }
        }
    }

    pub fn apply(&self, value: &TypedExpression) -> TypeResult<Vec<Binding>> {
        Ok(self.instantiate(value)?.bindings)
    }

    /// Receiver parameters remain rigid while the body is checked. The caller
    /// quantifies the collected parameters over the entire method signature.
    pub fn receiver_bindings(&self) -> Vec<Binding> {
        match &self.template {
            BindingTemplate::Discard => Vec::new(),
            BindingTemplate::Variable {
                name,
                parameter,
                origin,
            } => vec![Binding {
                name: name.clone(),
                pattern: *origin,
                value: Rc::new(TypeEvidence {
                    ty: Type::Variable(parameter.clone()),
                    expression: *origin,
                    origin: EvidenceOrigin::ReceiverPattern,
                    result_from: None,
                    methods: Vec::new(),
                }),
            }],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedBinding {
    pub value: TypedExpression,
    pub bindings: Vec<Binding>,
    pub instantiations: Vec<ParameterInstantiation>,
}

pub fn check_binding(
    environment: &Environment,
    pattern: &Loc<Pattern>,
    expression: &Loc<Expr>,
) -> TypeResult<CheckedBinding> {
    let plan = prepare_pattern(pattern);
    let value = synthesize_in(environment, expression)?;
    let instance = plan.instantiate(&value)?;
    Ok(CheckedBinding {
        value,
        bindings: instance.bindings,
        instantiations: instance.arguments,
    })
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

fn mismatch_path(actual: &Type, expected: &Type) -> Vec<String> {
    let (Type::Actor(actual), Type::Actor(expected)) = (actual, expected) else {
        return Vec::new();
    };
    for (index, required) in expected.methods.iter().enumerate() {
        if actual
            .methods
            .iter()
            .any(|provided| provided.is_subtype_of(required))
        {
            continue;
        }
        let mut path = vec![format!("expected.methods[{index}]")];
        if let Some(provided) = actual.methods.first() {
            path.push("actual.methods[0]".into());
            if !provided.parameters.is_empty() || !required.parameters.is_empty() {
                path.push("quantified signature".into());
                return path;
            }
            if !required.input.is_subtype_of(&provided.input) {
                path.push("input (contravariant)".into());
                path.extend(mismatch_path(&required.input, &provided.input));
            } else {
                path.push("output (covariant)".into());
                path.extend(mismatch_path(&provided.output, &required.output));
            }
        }
        return path;
    }
    Vec::new()
}

fn assert_type(value: &TypedExpression, expected: &Expectation) -> TypeResult<()> {
    if value.evidence.ty.is_subtype_of(&expected.ty) {
        Ok(())
    } else {
        Err(TypeError::Mismatch(TypeMismatch {
            expected: expected.clone(),
            actual: Rc::clone(&value.evidence),
            comparison_path: mismatch_path(&value.evidence.ty, &expected.ty),
        }))
    }
}

fn type_expression(
    environment: &Environment,
    expression: &Loc<Expr>,
    expected: Option<&Expectation>,
) -> TypeResult<TypedExpression> {
    let (ty, result_from, kind) = match &expression.value {
        Expr::Actor(actor) => {
            let mut methods = Vec::with_capacity(actor.methods.len());
            let mut method_types = Vec::with_capacity(actor.methods.len());
            for method in &actor.methods {
                let plan = prepare_pattern(&method.pattern);
                let bindings = plan.receiver_bindings();
                let scope = environment.extended(bindings.iter().cloned());
                let body = synthesize_in(&scope, &method.body)?;
                method_types.push(MethodType {
                    parameters: plan
                        .parameters
                        .iter()
                        .map(|p| p.parameter.clone())
                        .collect(),
                    input: plan.expectation.ty.clone(),
                    output: body.evidence.ty.clone(),
                });
                methods.push(TypedMethod {
                    parameters: plan.parameters,
                    span: method.span,
                    pattern: method.pattern.span,
                    expectation: plan.expectation,
                    bindings,
                    body: Box::new(body),
                });
            }
            (
                Type::Actor(ActorType {
                    methods: method_types,
                }),
                None,
                TypedExprKind::Actor { methods },
            )
        }
        Expr::Variable(name) => {
            let binding = environment
                .lookup(name)
                .ok_or_else(|| TypeError::UnboundVariable {
                    name: name.clone(),
                    span: expression.span,
                })?;
            (
                binding.value.ty.clone(),
                Some(Rc::clone(&binding.value)),
                TypedExprKind::Variable {
                    binding: binding.clone(),
                },
            )
        }
        Expr::Let(binding) => {
            let CheckedBinding {
                value,
                bindings,
                instantiations,
            } = check_binding(environment, &binding.pattern, &binding.value)?;
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
                result.ty.clone(),
                Some(result),
                TypedExprKind::Let {
                    value: Box::new(value),
                    instantiations,
                    bindings,
                    body: Box::new(body),
                },
            )
        }
    };
    let methods = match &kind {
        TypedExprKind::Actor { methods } => methods
            .iter()
            .map(|method| MethodEvidence {
                parameters: method.parameters.clone(),
                span: method.span,
                input: method.expectation.clone(),
                output: Rc::clone(&method.body.evidence),
            })
            .collect(),
        _ => Vec::new(),
    };
    let typed = TypedExpression {
        evidence: Rc::new(TypeEvidence {
            ty,
            expression: expression.span,
            origin: EvidenceOrigin::Expression,
            result_from,
            methods,
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

    fn actor(methods: Vec<(Type, Type)>) -> Type {
        Type::Actor(ActorType {
            methods: methods
                .into_iter()
                .map(|(input, output)| MethodType {
                    parameters: Vec::new(),
                    input,
                    output,
                })
                .collect(),
        })
    }

    #[test]
    fn pattern_parameters_instantiate_precisely_in_lets() {
        let ast = expression("let x = {}. x");
        let typed = synthesize(&ast).unwrap();
        let TypedExprKind::Let {
            bindings,
            instantiations,
            ..
        } = &typed.kind
        else {
            panic!()
        };
        assert_eq!(instantiations.len(), 1);
        assert_eq!(
            *instantiations[0].parameter.parameter.variable.upper_bound,
            Type::Any
        );
        assert_eq!(instantiations[0].parameter.origin, bindings[0].pattern);
        assert_eq!(instantiations[0].actual.ty, Type::UNIT);
        assert!(Rc::ptr_eq(&instantiations[0].actual, &bindings[0].value));
        assert_eq!(typed.evidence.ty, Type::UNIT);
        let Expr::Let(binding) = &ast.value else {
            panic!()
        };
        let first = prepare_pattern(&binding.pattern);
        let second = prepare_pattern(&binding.pattern);
        assert_ne!(
            first.parameters[0].parameter.variable,
            second.parameters[0].parameter.variable
        );
        assert_eq!(first.parameters.len(), 1);
        assert!(matches!(first.expectation.ty, Type::Variable(_)));
    }

    #[test]
    fn generic_receivers_preserve_dependency_and_scope() {
        for (source, expected) in [
            ("{ def x => x }", "{ <A <: any> A -> A }"),
            ("{ def _ => {} }", "{ any -> {} }"),
            ("{ def x => let y = x. y }", "{ <A <: any> A -> A }"),
            (
                "{ def x => { def y => x } }",
                "{ <A <: any> A -> { <B <: any> B -> A } }",
            ),
        ] {
            assert_eq!(
                synthesize(&expression(source))
                    .unwrap()
                    .evidence
                    .ty
                    .to_string(),
                expected
            );
        }
        let identity = synthesize(&expression("{ def x => x }")).unwrap();
        assert!(
            identity
                .evidence
                .ty
                .is_subtype_of(&actor(vec![(Type::UNIT, Type::UNIT)]))
        );
        assert!(!actor(vec![(Type::Any, Type::Any)]).is_subtype_of(&identity.evidence.ty));
        let TypedExprKind::Actor { methods } = &identity.kind else {
            panic!()
        };
        let binding = &methods[0].bindings[0];
        assert_eq!(methods[0].parameters[0].origin, binding.pattern);
        let scope = Environment::default().extended([binding.clone()]);
        assert!(
            check_in(
                &scope,
                &expression("x"),
                &Expectation {
                    ty: Type::Any,
                    origin: site()
                }
            )
            .is_ok()
        );
        assert!(
            check_in(
                &scope,
                &expression("x"),
                &Expectation {
                    ty: Type::UNIT,
                    origin: site()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn structural_width_variance_and_overlaps() {
        let broad = actor(vec![(Type::Any, Type::UNIT)]);
        let narrow = actor(vec![(Type::UNIT, Type::Any)]);
        assert!(broad.is_subtype_of(&narrow));
        assert!(!narrow.is_subtype_of(&broad));
        assert!(broad.is_subtype_of(&Type::UNIT));
        assert!(!Type::UNIT.is_subtype_of(&broad));
        assert!(!Type::Any.is_subtype_of(&Type::UNIT));
        assert!(Type::Never.is_subtype_of(&broad));
        let overlaps = actor(vec![(Type::Any, Type::Any), (Type::Any, Type::UNIT)]);
        assert_eq!(overlaps.to_string(), "{ any -> any. any -> {} }");
        assert!(overlaps.is_subtype_of(&broad));
        assert!(broad.is_subtype_of(&overlaps));
        let nested_actual = actor(vec![(narrow.clone(), broad.clone())]);
        let nested_expected = actor(vec![(broad, narrow)]);
        assert!(nested_actual.is_subtype_of(&nested_expected));
        assert!(!nested_expected.is_subtype_of(&nested_actual));
    }

    #[test]
    fn receivers_capture_and_shadow_without_leaking() {
        let typed = synthesize(&expression("let x = {}. { def x => x. def _ => x }")).unwrap();
        let TypedExprKind::Let { body, bindings, .. } = &typed.kind else {
            panic!()
        };
        let TypedExprKind::Actor { methods } = &body.kind else {
            panic!()
        };
        assert_eq!(methods.len(), 2);
        assert!(matches!(methods[0].expectation.ty, Type::Variable(_)));
        assert_eq!(methods[0].bindings[0].value.ty, methods[0].expectation.ty);
        assert_eq!(methods[0].body.evidence.ty, methods[0].expectation.ty);
        assert!(methods[1].bindings.is_empty());
        assert_eq!(methods[1].body.evidence.ty, Type::UNIT);
        let TypedExprKind::Variable { binding } = &methods[1].body.kind else {
            panic!()
        };
        assert_eq!(binding.pattern, bindings[0].pattern);
        assert_eq!(body.evidence.methods[0].input.origin, methods[0].pattern);
        assert!(Rc::ptr_eq(
            &body.evidence.methods[0].output,
            &methods[0].body.evidence
        ));
        assert!(methods[0].span.start.col < methods[1].span.start.col);
        for source in [
            "{ def x => x. def _ => x }",
            "let _ = { def x => x }. x",
            "{ def _ => x }",
        ] {
            assert!(
                matches!(
                    synthesize(&expression(source)),
                    Err(TypeError::UnboundVariable { .. })
                ),
                "{source}"
            );
        }
        let typed = synthesize(&expression("{ def x => { def _ => x } }")).unwrap();
        assert_eq!(
            typed.evidence.ty.to_string(),
            "{ <A <: any> A -> { any -> A } }"
        );
    }

    #[test]
    fn structural_failure_keeps_components_through_aliases() {
        let expected = Expectation {
            ty: actor(vec![(Type::Any, Type::UNIT)]),
            origin: site(),
        };
        let TypeError::Mismatch(error) =
            check(&expression("let a = { def x => x }. a"), &expected).unwrap_err()
        else {
            panic!()
        };
        assert_eq!(
            error.comparison_path,
            [
                "expected.methods[0]",
                "actual.methods[0]",
                "quantified signature"
            ]
        );
        assert_eq!(error.expected.origin, site());
        let component = &error.actual.producer().methods[0];
        assert!(matches!(component.output.ty, Type::Variable(_)));
        assert_eq!(
            component.output.producer().origin,
            EvidenceOrigin::ReceiverPattern
        );
        assert_eq!(
            component.output.producer().expression,
            component.input.origin
        );
        let TypeError::Mismatch(error) = check(&expression("{}"), &expected).unwrap_err() else {
            panic!()
        };
        assert_eq!(error.comparison_path, ["expected.methods[0]"]);
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
            ..
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
                origin: EvidenceOrigin::Expression,
                result_from: None,
                methods: Vec::new(),
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
                assert_eq!(actual.is_subtype_of(expected), i <= j);
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
            ..
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
            value: Pattern::Discard,
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
            ..
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

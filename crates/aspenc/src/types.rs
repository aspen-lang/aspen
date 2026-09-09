//! Bidirectional typing with source evidence kept separate from semantic types.

use std::{fmt, rc::Rc};

use crate::{Expr, Loc, Pattern, Program, Selector, Span, TypeExpr};

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
    pub selector: Option<Selector<Rc<TypeEvidence>>>,
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
    Selector(Selector<TypedExpression>),
    Send {
        callee: Box<TypedExpression>,
        message: Box<TypedExpression>,
        method: Option<usize>,
    },
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
    UnboundVariable {
        name: String,
        span: Span,
    },
    OverlappingReceivers {
        first: Span,
        second: Span,
    },
    DuplicateBinding {
        name: String,
        first: Span,
        second: Span,
    },
    NotActor {
        callee: Rc<TypeEvidence>,
    },
    NoReceiver {
        callee: Rc<TypeEvidence>,
        message: Rc<TypeEvidence>,
    },
    UnknownType {
        name: String,
        span: Span,
    },
    InvalidAnnotation {
        span: Span,
        message: String,
    },
    InvalidActorType {
        span: Span,
    },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch(mismatch) => mismatch.fmt(f),
            Self::UnknownType { name, .. } => write!(f, "unknown type {name:?}"),
            Self::InvalidAnnotation { message, .. } => f.write_str(message),
            Self::OverlappingReceivers { .. } => {
                f.write_str("receiver input types are not disjoint")
            }
            Self::DuplicateBinding { name, .. } => write!(f, "duplicate pattern binding {name:?}"),
            Self::NotActor { .. } => f.write_str("message receiver is not an actor"),
            Self::NoReceiver { .. } => f.write_str("no receiver accepts this message type"),
            Self::InvalidActorType { .. } => {
                f.write_str("actor type has overlapping receiver inputs")
            }
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
    pub types: TypeEnvironment,
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

/// Named types have a separate namespace; resolving a name never creates a parameter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeEnvironment {
    bindings: Vec<(String, Type)>,
}

impl TypeEnvironment {
    pub fn extended(&self, name: impl Into<String>, ty: Type) -> Self {
        let mut scope = self.clone();
        scope.bindings.push((name.into(), ty));
        scope
    }

    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.bindings
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, ty)| ty)
    }
}

pub fn resolve_type(environment: &TypeEnvironment, syntax: &Loc<TypeExpr>) -> TypeResult<Type> {
    let ty = match &syntax.value {
        TypeExpr::Any => Type::Any,
        TypeExpr::Never => Type::Never,
        TypeExpr::Variable(name) => {
            environment
                .lookup(name)
                .cloned()
                .ok_or_else(|| TypeError::UnknownType {
                    name: name.clone(),
                    span: syntax.span,
                })?
        }
        TypeExpr::Selector(selector) => Type::Selector(try_selector_map(selector, |child| {
            resolve_type(environment, child)
        })?),
        TypeExpr::Actor(methods) => Type::Actor(ActorType {
            methods: methods
                .iter()
                .map(|method| {
                    Ok(MethodType {
                        parameters: Vec::new(),
                        input: resolve_type(environment, &method.input)?,
                        output: resolve_type(environment, &method.output)?,
                    })
                })
                .collect::<TypeResult<_>>()?,
        }),
    };
    if !ty.is_well_formed() {
        return Err(TypeError::InvalidActorType { span: syntax.span });
    }
    Ok(ty)
}

fn try_selector_map<T, U>(
    selector: &Selector<T>,
    mut f: impl FnMut(&T) -> TypeResult<U>,
) -> TypeResult<Selector<U>> {
    Ok(match selector {
        Selector::Atomic(name) => Selector::Atomic(name.clone()),
        Selector::Operator { operator, value } => Selector::Operator {
            operator: operator.clone(),
            value: Box::new(f(value)?),
        },
        Selector::Keyword(parts) => Selector::Keyword(
            parts
                .iter()
                .map(|(name, value)| Ok((name.clone(), f(value)?)))
                .collect::<TypeResult<_>>()?,
        ),
    })
}

/// Parameter declarations keep their source origins outside semantic types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedParameter {
    pub parameter: TypeParameter,
    pub origin: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingTemplate {
    Selector(Selector<PatternPlan>),
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

pub fn prepare_pattern(pattern: &Loc<Pattern>) -> TypeResult<PatternPlan> {
    prepare_pattern_in(&TypeEnvironment::default(), pattern)
}

pub fn prepare_pattern_in(
    environment: &TypeEnvironment,
    pattern: &Loc<Pattern>,
) -> TypeResult<PatternPlan> {
    prepare_with_bound(
        environment,
        pattern,
        &Expectation {
            ty: Type::Any,
            origin: pattern.span,
        },
    )
}

fn prepare_with_bound(
    environment: &TypeEnvironment,
    pattern: &Loc<Pattern>,
    bound: &Expectation,
) -> TypeResult<PatternPlan> {
    Ok(match &pattern.value {
        Pattern::Annotated { ty, pattern: inner } => {
            let resolved = resolve_type(environment, ty)?;
            // A nested explicit annotation is allowed only when it strengthens
            // the enclosing requirement; no implicit intersection types exist.
            if !resolved.is_subtype_of(&bound.ty) {
                return Err(TypeError::InvalidAnnotation {
                    span: ty.span,
                    message: "annotation does not refine the enclosing type constraint".into(),
                });
            }
            let mut plan = prepare_with_bound(
                environment,
                inner,
                &Expectation {
                    ty: resolved,
                    origin: ty.span,
                },
            )?;
            locate_annotation_components(&mut plan, ty, ty.span);
            plan
        }
        Pattern::Selector(selector) => {
            let upper = &bound.ty;
            let children = if *upper == Type::Any {
                try_selector_map(selector, |child| prepare_pattern_in(environment, child))?
            } else if let Type::Selector(expected) = upper {
                if !selector.same_shape(expected) {
                    return Err(TypeError::InvalidAnnotation {
                        span: bound.origin,
                        message: "annotation and selector pattern have different shapes".into(),
                    });
                }
                let types = expected.values();
                let mut index = 0;
                try_selector_map(selector, |child| {
                    let result = prepare_with_bound(
                        environment,
                        child,
                        &Expectation {
                            ty: types[index].clone(),
                            origin: bound.origin,
                        },
                    );
                    index += 1;
                    result
                })?
            } else {
                return Err(TypeError::InvalidAnnotation {
                    span: bound.origin,
                    message: "annotation does not accept this selector pattern".into(),
                });
            };
            let parameters = children
                .values()
                .into_iter()
                .flat_map(|p| p.parameters.clone())
                .collect();
            PatternPlan {
                parameters,
                expectation: Expectation {
                    ty: Type::Selector(children.map(|p| p.expectation.ty.clone())),
                    origin: pattern.span,
                },
                template: BindingTemplate::Selector(children),
            }
        }
        Pattern::Discard => PatternPlan {
            parameters: Vec::new(),
            expectation: bound.clone(),
            template: BindingTemplate::Discard,
        },
        Pattern::Variable(name) => {
            let variable = TypeVariable::fresh(bound.ty.clone());
            PatternPlan {
                parameters: vec![LocatedParameter {
                    parameter: TypeParameter {
                        variable: variable.clone(),
                    },
                    origin: bound.origin,
                }],
                expectation: Expectation {
                    ty: Type::Variable(variable.clone()),
                    origin: bound.origin,
                },
                template: BindingTemplate::Variable {
                    name: name.clone(),
                    parameter: variable,
                    origin: pattern.span,
                },
            }
        }
    })
}

// Distribute written component origins along with their constraints. Explicit
// annotations inside the pattern retain their own, more local origins.
fn locate_annotation_components(plan: &mut PatternPlan, syntax: &Loc<TypeExpr>, inherited: Span) {
    if plan.expectation.origin == inherited {
        plan.expectation.origin = syntax.span;
    }
    for parameter in &mut plan.parameters {
        if parameter.origin == inherited {
            parameter.origin = syntax.span;
        }
    }
    if let (BindingTemplate::Selector(children), TypeExpr::Selector(types)) =
        (&mut plan.template, &syntax.value)
    {
        match (children, types) {
            (Selector::Operator { value: child, .. }, Selector::Operator { value: ty, .. }) => {
                locate_annotation_components(child, ty, inherited)
            }
            (Selector::Keyword(children), Selector::Keyword(types)) => {
                for ((_, child), (_, ty)) in children.iter_mut().zip(types) {
                    locate_annotation_components(child, ty, inherited);
                }
            }
            _ => {}
        }
        if let BindingTemplate::Selector(children) = &plan.template {
            plan.parameters = children
                .values()
                .into_iter()
                .flat_map(|child| child.parameters.clone())
                .collect();
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
        self.instantiate_evidence(&value.evidence)
    }

    fn instantiate_evidence(&self, value: &Rc<TypeEvidence>) -> TypeResult<PatternInstantiation> {
        match &self.template {
            BindingTemplate::Selector(children) => {
                let upper = self
                    .parameters
                    .iter()
                    .map(|p| {
                        (
                            p.parameter.variable.clone(),
                            (*p.parameter.variable.upper_bound).clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let expected = self.expectation.ty.substitute(&upper);
                let mut actual_type = &value.ty;
                while let Type::Variable(variable) = actual_type {
                    actual_type = &variable.upper_bound;
                }
                let actual = match actual_type {
                    Type::Selector(actual) if children.same_shape(actual) => Some(actual),
                    Type::Never => None,
                    _ => {
                        assert_evidence(
                            value,
                            &Expectation {
                                ty: expected,
                                origin: self.expectation.origin,
                            },
                        )?;
                        None
                    }
                };
                let producer = value.producer();
                let evidence = producer
                    .selector
                    .as_ref()
                    .map(|s| s.values())
                    .unwrap_or_default();
                let types = actual.map(|s| s.values()).unwrap_or_default();
                let mut result = PatternInstantiation {
                    arguments: Vec::new(),
                    bindings: Vec::new(),
                };
                for (index, child) in children.values().iter().enumerate() {
                    let component =
                        evidence
                            .get(index)
                            .map(|v| Rc::clone(v))
                            .unwrap_or_else(|| {
                                Rc::new(TypeEvidence {
                                    ty: types
                                        .get(index)
                                        .map(|t| (*t).clone())
                                        .unwrap_or(Type::Never),
                                    expression: value.expression,
                                    origin: value.origin,
                                    result_from: None,
                                    methods: Vec::new(),
                                    selector: None,
                                })
                            });
                    let instance = child.instantiate_evidence(&component)?;
                    result.arguments.extend(instance.arguments);
                    result.bindings.extend(instance.bindings);
                }
                Ok(result)
            }
            BindingTemplate::Discard => {
                assert_evidence(value, &self.expectation)?;
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
                    origin: self.expectation.origin,
                };
                assert_evidence(value, &expected)?;
                Ok(PatternInstantiation {
                    arguments: vec![ParameterInstantiation {
                        parameter: declaration.clone(),
                        actual: Rc::clone(value),
                    }],
                    bindings: vec![Binding {
                        name: name.clone(),
                        pattern: *origin,
                        value: Rc::clone(value),
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
            BindingTemplate::Selector(children) => children
                .values()
                .into_iter()
                .flat_map(|p| p.receiver_bindings())
                .collect(),
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
                    selector: None,
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
    let plan = prepare_pattern_in(&environment.types, pattern)?;
    validate_bindings(&plan)?;
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
    if !expected.ty.is_well_formed() {
        return Err(TypeError::InvalidActorType {
            span: expected.origin,
        });
    }
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
    assert_evidence(&value.evidence, expected)
}

fn assert_evidence(value: &Rc<TypeEvidence>, expected: &Expectation) -> TypeResult<()> {
    if value.ty.is_subtype_of(&expected.ty) {
        Ok(())
    } else {
        Err(TypeError::Mismatch(TypeMismatch {
            expected: expected.clone(),
            actual: Rc::clone(value),
            comparison_path: mismatch_path(&value.ty, &expected.ty),
        }))
    }
}

fn collect_argument_evidence(
    template: &Type,
    actual: &Rc<TypeEvidence>,
    arguments: &mut Vec<(TypeVariable, Rc<TypeEvidence>)>,
) {
    match template {
        Type::Variable(variable) => arguments.push((variable.clone(), Rc::clone(actual))),
        Type::Selector(selector) => {
            if let Some(components) = &actual.producer().selector
                && selector.same_shape(components)
            {
                for (template, value) in selector.values().into_iter().zip(components.values()) {
                    collect_argument_evidence(template, value, arguments);
                }
            }
        }
        _ => {}
    }
}

fn substitute_evidence(
    evidence: &TypeEvidence,
    instances: &[(TypeVariable, Type)],
    arguments: &[(TypeVariable, Rc<TypeEvidence>)],
) -> Rc<TypeEvidence> {
    if let Type::Variable(variable) = &evidence.ty
        && let Some((_, actual)) = arguments.iter().find(|(key, _)| key == variable)
    {
        return Rc::new(TypeEvidence {
            ty: actual.ty.clone(),
            expression: evidence.expression,
            origin: evidence.origin,
            result_from: Some(Rc::clone(actual)),
            methods: Vec::new(),
            selector: None,
        });
    }
    Rc::new(TypeEvidence {
        ty: evidence.ty.substitute(instances),
        expression: evidence.expression,
        origin: evidence.origin,
        result_from: evidence
            .result_from
            .as_ref()
            .map(|source| substitute_evidence(source, instances, arguments)),
        selector: evidence
            .selector
            .as_ref()
            .map(|s| s.map(|v| substitute_evidence(v, instances, arguments))),
        methods: evidence
            .methods
            .iter()
            .map(|method| MethodEvidence {
                parameters: method.parameters.clone(),
                span: method.span,
                input: Expectation {
                    ty: method.input.ty.substitute(instances),
                    origin: method.input.origin,
                },
                output: substitute_evidence(&method.output, instances, arguments),
            })
            .collect(),
    })
}

fn validate_bindings(plan: &PatternPlan) -> TypeResult<()> {
    let bindings = plan.receiver_bindings();
    for (i, binding) in bindings.iter().enumerate() {
        if let Some(previous) = bindings[..i].iter().find(|p| p.name == binding.name) {
            return Err(TypeError::DuplicateBinding {
                name: binding.name.clone(),
                first: previous.pattern,
                second: binding.pattern,
            });
        }
    }
    Ok(())
}

fn type_expression(
    environment: &Environment,
    expression: &Loc<Expr>,
    expected: Option<&Expectation>,
) -> TypeResult<TypedExpression> {
    let (ty, result_from, kind) = match &expression.value {
        Expr::Selector(selector) => {
            let mut error = None;
            let typed = selector.map(|value| match synthesize_in(environment, value) {
                Ok(value) => Some(value),
                Err(e) => {
                    error = Some(e);
                    None
                }
            });
            if let Some(error) = error {
                return Err(error);
            }
            let typed = typed.map(|v| v.clone().unwrap());
            let ty = if typed.values().iter().any(|v| v.evidence.ty == Type::Never) {
                Type::Never
            } else {
                Type::Selector(typed.map(|v| v.evidence.ty.clone()))
            };
            (ty, None, TypedExprKind::Selector(typed))
        }
        Expr::Send { callee, message } => {
            let callee = synthesize_in(environment, callee)?;
            let message = synthesize_in(environment, message)?;
            let mut ty = &callee.evidence.ty;
            while let Type::Variable(variable) = ty {
                ty = &variable.upper_bound;
            }
            let (output, method) = if *ty == Type::Never || message.evidence.ty == Type::Never {
                (Type::Never, None)
            } else {
                let Type::Actor(actor) = ty else {
                    return Err(TypeError::NotActor {
                        callee: Rc::clone(&callee.evidence),
                    });
                };
                if !actor.has_disjoint_inputs() {
                    return Err(TypeError::InvalidActorType {
                        span: callee.evidence.expression,
                    });
                }
                let Some((index, reply)) =
                    actor.methods.iter().enumerate().find_map(|(i, method)| {
                        method
                            .instantiate(&message.evidence.ty)
                            .map(|reply| (i, reply))
                    })
                else {
                    return Err(TypeError::NoReceiver {
                        callee: Rc::clone(&callee.evidence),
                        message: Rc::clone(&message.evidence),
                    });
                };
                (reply, Some(index))
            };
            let result_from = method.and_then(|index| {
                let Type::Actor(actor) = ty else {
                    return None;
                };
                let instances = actor.methods[index].infer_instances(&message.evidence.ty)?;
                let mut arguments = Vec::new();
                collect_argument_evidence(
                    &actor.methods[index].input,
                    &message.evidence,
                    &mut arguments,
                );
                callee
                    .evidence
                    .producer()
                    .methods
                    .get(index)
                    .map(|method| substitute_evidence(&method.output, &instances, &arguments))
            });
            (
                output,
                result_from,
                TypedExprKind::Send {
                    callee: Box::new(callee),
                    message: Box::new(message),
                    method,
                },
            )
        }
        Expr::Actor(actor) => {
            let mut methods = Vec::with_capacity(actor.methods.len());
            let mut method_types = Vec::with_capacity(actor.methods.len());
            for method in &actor.methods {
                let plan = prepare_pattern_in(&environment.types, &method.pattern)?;
                validate_bindings(&plan)?;
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
            for i in 0..method_types.len() {
                for j in 0..i {
                    if !method_types[i]
                        .accepted_input()
                        .is_disjoint_from(&method_types[j].accepted_input())
                    {
                        return Err(TypeError::OverlappingReceivers {
                            first: methods[j].pattern,
                            second: methods[i].pattern,
                        });
                    }
                }
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
            if !binding.value.ty.is_well_formed() {
                return Err(TypeError::InvalidActorType {
                    span: binding.pattern,
                });
            }
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
    let selector = match &kind {
        TypedExprKind::Selector(s) => Some(s.map(|v| Rc::clone(&v.evidence))),
        _ => None,
    };
    let typed = TypedExpression {
        evidence: Rc::new(TypeEvidence {
            ty,
            expression: expression.span,
            origin: EvidenceOrigin::Expression,
            result_from,
            methods,
            selector,
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
    fn annotated_bindings_preserve_precision_and_bound_receivers() {
        let typed = synthesize(&expression("let any x = {}. x")).unwrap();
        assert_eq!(typed.evidence.ty, Type::UNIT);
        let typed = synthesize(&expression("{ def ({} x) => x }")).unwrap();
        let Type::Actor(actor) = &typed.evidence.ty else {
            panic!()
        };
        let parameter = &actor.methods[0].parameters[0].variable;
        assert_eq!(*parameter.upper_bound, Type::UNIT);
        assert_eq!(actor.methods[0].output, Type::Variable(parameter.clone()));
        assert_eq!(
            synthesize(&expression("{ def ({} x) => x } ({})"))
                .unwrap()
                .evidence
                .ty,
            Type::UNIT
        );
        assert!(matches!(
            synthesize(&expression("{ def ({} x) => x } bad")),
            Err(TypeError::NoReceiver { .. })
        ));
    }

    #[test]
    fn annotation_errors_locate_written_type_and_actual_component() {
        let source = "let #x: y z: {} abc = #x: {} z: #bad. abc";
        let TypeError::Mismatch(error) = synthesize(&expression(source)).unwrap_err() else {
            panic!()
        };
        assert_eq!(error.expected.ty, Type::UNIT);
        assert_eq!(
            error.expected.origin.start.col as usize,
            source.find("{} abc").unwrap() + 1
        );
        assert_eq!(
            error.actual.expression.start.col as usize,
            source.find("#bad").unwrap() + 1
        );
        let source = "let (#x: {} z: any) (#x: a z: b) = #x: #bad z: {}. b";
        let TypeError::Mismatch(error) = synthesize(&expression(source)).unwrap_err() else {
            panic!()
        };
        assert_eq!(
            error.expected.origin.start.col as usize,
            source.find("{}").unwrap() + 1
        );
        assert_eq!(
            error.actual.expression.start.col as usize,
            source.find("#bad").unwrap() + 1
        );
        assert!(matches!(
            synthesize(&expression("let never _ = {}. {}")),
            Err(TypeError::Mismatch(_))
        ));
    }

    #[test]
    fn named_types_resolve_without_implicit_declarations() {
        assert!(
            matches!(synthesize(&expression("let Missing x = {}. x")), Err(TypeError::UnknownType { name, .. }) if name == "Missing")
        );
        let parameter = TypeVariable::fresh(Type::UNIT);
        let mut environment = Environment::default();
        environment.types = environment
            .types
            .extended("T", Type::Variable(parameter.clone()));
        environment = environment.extended([Binding {
            name: "input".into(),
            pattern: site(),
            value: Rc::new(TypeEvidence {
                ty: Type::Variable(parameter.clone()),
                expression: site(),
                origin: EvidenceOrigin::ReceiverPattern,
                result_from: None,
                methods: Vec::new(),
                selector: None,
            }),
        }]);
        let typed = synthesize_in(&environment, &expression("let T x = input. x")).unwrap();
        assert_eq!(typed.evidence.ty, Type::Variable(parameter));
        assert!(matches!(
            synthesize_in(&environment, &expression("let T x = {}. x")),
            Err(TypeError::Mismatch(_))
        ));
        let typed = synthesize_in(&environment, &expression("{ def (T x) => x }")).unwrap();
        let Type::Actor(actor) = typed.evidence.ty.clone() else {
            panic!()
        };
        assert_eq!(
            *actor.methods[0].parameters[0].variable.upper_bound,
            environment.types.lookup("T").unwrap().clone()
        );
    }

    #[test]
    fn grouped_selector_annotations_and_nested_constraints() {
        let typed = synthesize(&expression("let (#tag: {}) x = #tag: {}. x")).unwrap();
        assert!(matches!(typed.evidence.ty, Type::Selector(_)));
        assert_eq!(
            synthesize(&expression("let any ({} x) = {}. x"))
                .unwrap()
                .evidence
                .ty,
            Type::UNIT
        );
        assert!(matches!(
            synthesize(&expression("let {} (any x) = {}. x")),
            Err(TypeError::InvalidAnnotation { .. })
        ));
        let typed = synthesize(&expression("let any (#x: a z: b) = #x: {} z: {}. b")).unwrap();
        assert_eq!(typed.evidence.ty, Type::UNIT);
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
        let first = prepare_pattern(&binding.pattern).unwrap();
        let second = prepare_pattern(&binding.pattern).unwrap();
        assert_ne!(
            first.parameters[0].parameter.variable,
            second.parameters[0].parameter.variable
        );
        assert_eq!(first.parameters.len(), 1);
        assert!(matches!(first.expectation.ty, Type::Variable(_)));
    }

    #[test]
    fn selectors_bind_payloads_and_send_instantiates_replies() {
        for (source, expected) in [
            ("let #x = #x. #done", "#done"),
            ("let #x: y = #x: {}. y", "{}"),
            ("{ def value: x => x } value: {}", "{}"),
            (
                "{ def pair: x with: y => y } pair: #left with: #right",
                "#right",
            ),
            ("{ def + x => x } + #hello", "#hello"),
            ("{ def foo => {}. def bar => #reply } bar", "#reply"),
            ("{ def (x) => x } (#hello)", "#hello"),
            ("let #outer: (#inner: x) = #outer: (#inner: {}). x", "{}"),
            (
                "let a = { def value: x => #result: x }. let #result: y = a value: {}. y",
                "{}",
            ),
        ] {
            let typed = synthesize(&expression(source)).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert_eq!(typed.evidence.ty.to_string(), expected, "{source}");
        }
    }

    #[test]
    fn send_results_retain_message_component_provenance() {
        let source = "let a = { def value: x => #result: x }. let #result: y = a value: {}. y";
        let typed = synthesize(&expression(source)).unwrap();
        assert_eq!(typed.evidence.ty, Type::UNIT);
        assert_eq!(
            typed.evidence.producer().expression.start.col as usize,
            source.rfind("{}").unwrap() + 1
        );
        let nested = synthesize(&expression(
            "({ def value: x => { def get => x } } value: #hello) get",
        ))
        .unwrap();
        assert_eq!(nested.evidence.ty.to_string(), "#hello");
    }

    #[test]
    fn selectors_and_sends_reject_invalid_programs() {
        for source in [
            "{ def x => {}. def x => {} }",
            "{ def (x) => x. def foo => {} }",
            "{ def value: x => x. def value: _ => {} }",
        ] {
            assert!(
                matches!(
                    synthesize(&expression(source)),
                    Err(TypeError::OverlappingReceivers { .. })
                ),
                "{source}"
            );
        }
        assert!(matches!(
            synthesize(&expression("{ def pair: x with: x => x }")),
            Err(TypeError::DuplicateBinding { .. })
        ));
        assert!(matches!(
            synthesize(&expression("{} foo")),
            Err(TypeError::NoReceiver { .. })
        ));
        assert!(matches!(
            synthesize(&expression("(#foo) foo")),
            Err(TypeError::NotActor { .. })
        ));
        assert!(matches!(
            synthesize(&expression("{ def x => x }")),
            Err(TypeError::UnboundVariable { .. })
        ));
        let source = "let #outer: (#inner: _) = #outer: #wrong. {}";
        let TypeError::Mismatch(error) = synthesize(&expression(source)).unwrap_err() else {
            panic!()
        };
        assert_eq!(error.expected.origin.start.col, 13);
        assert_eq!(error.actual.expression.start.col, 35);
    }

    #[test]
    fn generic_receivers_preserve_dependency_and_scope() {
        for (source, expected) in [
            ("{ def (x) => x }", "{ <A <: any> (A) -> A }"),
            ("{ def (_) => {} }", "{ (any) -> {} }"),
            ("{ def (x) => let y = x. y }", "{ <A <: any> (A) -> A }"),
            (
                "{ def (x) => { def (y) => x } }",
                "{ <A <: any> (A) -> { <B <: any> (B) -> A } }",
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
        let identity = synthesize(&expression("{ def (x) => x }")).unwrap();
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
        assert!(!overlaps.is_well_formed());
        assert!(!overlaps.is_subtype_of(&broad));
        assert!(!broad.is_subtype_of(&overlaps));
        let nested_actual = actor(vec![(narrow.clone(), broad.clone())]);
        let nested_expected = actor(vec![(broad, narrow)]);
        assert!(nested_actual.is_subtype_of(&nested_expected));
        assert!(!nested_expected.is_subtype_of(&nested_actual));
    }

    #[test]
    fn receivers_capture_and_shadow_without_leaking() {
        let typed = synthesize(&expression(
            "let x = {}. { def value: x => x. def other => x }",
        ))
        .unwrap();
        let TypedExprKind::Let { body, bindings, .. } = &typed.kind else {
            panic!()
        };
        let TypedExprKind::Actor { methods } = &body.kind else {
            panic!()
        };
        assert_eq!(methods.len(), 2);
        assert!(matches!(methods[0].bindings[0].value.ty, Type::Variable(_)));
        assert_eq!(methods[0].body.evidence.ty, methods[0].bindings[0].value.ty);
        assert!(methods[1].bindings.is_empty());
        assert_eq!(methods[1].body.evidence.ty, Type::UNIT);
        let TypedExprKind::Variable { binding } = &methods[1].body.kind else {
            panic!()
        };
        assert_eq!(binding.pattern, bindings[0].pattern);
        for source in [
            "{ def (x) => x. def (_) => x }",
            "let _ = { def (x) => x }. x",
            "{ def (_) => x }",
        ] {
            assert!(
                matches!(
                    synthesize(&expression(source)),
                    Err(TypeError::UnboundVariable { .. })
                ),
                "{source}"
            );
        }
        let typed = synthesize(&expression("{ def (x) => { def (_) => x } }")).unwrap();
        assert_eq!(
            typed.evidence.ty.to_string(),
            "{ <A <: any> (A) -> { (any) -> A } }"
        );
    }

    #[test]
    fn structural_failure_keeps_components_through_aliases() {
        let expected = Expectation {
            ty: actor(vec![(Type::Any, Type::UNIT)]),
            origin: site(),
        };
        let TypeError::Mismatch(error) =
            check(&expression("let a = { def (x) => x }. a"), &expected).unwrap_err()
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
                selector: None,
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
        let mut plan = prepare_pattern(&pattern).unwrap();
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

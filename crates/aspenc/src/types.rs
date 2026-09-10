//! Bidirectional typing with source evidence kept separate from semantic types.

use std::{fmt, rc::Rc};

use crate::{Expr, Loc, Pattern, Program, Selector, Span, Stmt, TypeExpr};

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
    ReplyAnnotation,
}

/// A value's origin is explicit: expression and pattern trees need not align.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeEvidence {
    pub ty: Type,
    /// The originating source span; receiver values originate at their pattern.
    pub expression: Span,
    pub origin: EvidenceOrigin,
    pub value_from: Option<Rc<TypeEvidence>>,
    /// Actor components live at the producer, including through lexical aliases.
    pub methods: Vec<MethodEvidence>,
    pub selector: Option<Selector<Rc<TypeEvidence>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodEvidence {
    pub parameters: Vec<LocatedParameter>,
    pub span: Span,
    pub input: Expectation,
    pub reply: Option<Rc<TypeEvidence>>,
}

impl TypeEvidence {
    pub fn producer(&self) -> &Self {
        match &self.value_from {
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
    pub adaptations: Vec<AdaptationPlan>,
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
        adaptation: Option<AdaptationPlan>,
        bound_adaptations: Vec<BoundAdaptation>,
    },
    Actor {
        methods: Vec<TypedMethod>,
    },
    Variable {
        binding: Binding,
    },
    ReplyTo {
        binding: Binding,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedMethod {
    pub plan: PatternPlan,
    pub parameters: Vec<LocatedParameter>,
    pub span: Span,
    pub pattern: Span,
    pub expectation: Expectation,
    pub reply: Option<Rc<TypeEvidence>>,
    pub bindings: Vec<Binding>,
    pub body: Vec<TypedStatement>,
}

/// Statements have no value; only expression statements carry discarded evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedStatement {
    Let(CheckedBinding),
    Expr(TypedExpression),
    NoReplySend {
        span: Span,
        callee: TypedExpression,
        message: TypedExpression,
        method: usize,
        adaptation: AdaptationPlan,
        bound_adaptations: Vec<BoundAdaptation>,
    },
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
    NoReplyValue {
        span: Span,
    },
    ReplyToOutsideAnnotatedMethod {
        span: Span,
    },
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
            Self::NoReplyValue { .. } => f.write_str("a no-reply send cannot be used as a value"),
            Self::ReplyToOutsideAnnotatedMethod { .. } => f.write_str(
                "^ is reply-to actor is only available in a method with a reply annotation",
            ),
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
    pub reply_to: Option<Binding>,
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
                        reply: method
                            .reply
                            .as_ref()
                            .map(|reply| resolve_type(environment, reply))
                            .transpose()?,
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
pub struct BoundAdaptation {
    pub parameter: TypeVariable,
    pub actual: Type,
    pub expected: Type,
    pub plan: AdaptationPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterInstantiation {
    pub adaptation: AdaptationPlan,
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
                                    value_from: None,
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
                        adaptation: value
                            .ty
                            .adaptation_to(&expected.ty)
                            .expect("checked parameter bound"),
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
                    value_from: None,
                    methods: Vec::new(),
                    selector: None,
                }),
            }],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedBinding {
    pub adaptation: AdaptationPlan,
    pub plan: PatternPlan,
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
    let bounds = plan
        .parameters
        .iter()
        .map(|p| {
            (
                p.parameter.variable.clone(),
                (*p.parameter.variable.upper_bound).clone(),
            )
        })
        .collect::<Vec<_>>();
    let adaptation = value
        .evidence
        .ty
        .adaptation_to(&plan.expectation.ty.substitute(&bounds))
        .expect("checked binding has an adaptation");
    Ok(CheckedBinding {
        adaptation,
        plan,
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
                path.push("reply (covariant)".into());
                if let (Some(actual), Some(expected)) = (&provided.reply, &required.reply) {
                    path.extend(mismatch_path(actual, expected));
                } else {
                    path.push("reply mode".into());
                }
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
            value_from: Some(Rc::clone(actual)),
            methods: Vec::new(),
            selector: None,
        });
    }
    Rc::new(TypeEvidence {
        ty: evidence.ty.substitute(instances),
        expression: evidence.expression,
        origin: evidence.origin,
        value_from: evidence
            .value_from
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
                reply: method
                    .reply
                    .as_ref()
                    .map(|reply| substitute_evidence(reply, instances, arguments)),
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

fn type_send(
    environment: &Environment,
    span: Span,
    callee: &Loc<Expr>,
    message: &Loc<Expr>,
) -> TypeResult<TypedStatement> {
    let callee = synthesize_in(environment, callee)?;
    let message = synthesize_in(environment, message)?;
    let mut ty = &callee.evidence.ty;
    while let Type::Variable(variable) = ty {
        ty = &variable.upper_bound;
    }
    let (reply, method) = if *ty == Type::Never || message.evidence.ty == Type::Never {
        (Some(Type::Never), None)
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
        let Some((index, reply)) = actor.methods.iter().enumerate().find_map(|(i, method)| {
            method
                .instantiate(&message.evidence.ty)
                .map(|reply| (i, reply))
        }) else {
            return Err(TypeError::NoReceiver {
                callee: Rc::clone(&callee.evidence),
                message: Rc::clone(&message.evidence),
            });
        };
        (reply, Some(index))
    };
    let value_from = method.and_then(|index| {
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
            .and_then(|method| method.reply.as_ref())
            .map(|reply| substitute_evidence(reply, &instances, &arguments))
    });
    let bound_adaptations = method
        .map(|index| {
            let Type::Actor(actor) = ty else {
                unreachable!()
            };
            let selected = &actor.methods[index];
            let instances = selected
                .infer_instances(&message.evidence.ty)
                .expect("checked send instances");
            instances
                .iter()
                .map(|(parameter, actual)| {
                    let expected = parameter.upper_bound.substitute(&instances);
                    BoundAdaptation {
                        parameter: parameter.clone(),
                        actual: actual.clone(),
                        plan: actual.adaptation_to(&expected).expect("checked send bound"),
                        expected,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let adaptation = method.and_then(|index| {
        let Type::Actor(actor) = ty else {
            return None;
        };
        let method = &actor.methods[index];
        let instances = method.infer_instances(&message.evidence.ty)?;
        message
            .evidence
            .ty
            .adaptation_to(&method.input.substitute(&instances))
    });
    let Some(reply) = reply else {
        return Ok(TypedStatement::NoReplySend {
            span,
            callee,
            message,
            method: method.expect("no-reply send selects a method"),
            adaptation: adaptation.expect("checked send has an adaptation"),
            bound_adaptations,
        });
    };
    Ok(TypedStatement::Expr(TypedExpression {
        adaptations: Vec::new(),
        evidence: Rc::new(TypeEvidence {
            ty: reply,
            expression: span,
            origin: EvidenceOrigin::Expression,
            value_from,
            methods: Vec::new(),
            selector: None,
        }),
        kind: TypedExprKind::Send {
            callee: Box::new(callee),
            message: Box::new(message),
            method,
            adaptation,
            bound_adaptations,
        },
    }))
}

fn type_expression(
    environment: &Environment,
    expression: &Loc<Expr>,
    expected: Option<&Expectation>,
) -> TypeResult<TypedExpression> {
    let (ty, value_from, kind) = match &expression.value {
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
            return match type_send(environment, expression.span, callee, message)? {
                TypedStatement::Expr(mut typed) => {
                    if let Some(expected) = expected {
                        assert_type(&typed, expected)?;
                        typed
                            .adaptations
                            .push(typed.evidence.ty.adaptation_to(&expected.ty).unwrap());
                    }
                    Ok(typed)
                }
                TypedStatement::NoReplySend { .. } => Err(TypeError::NoReplyValue {
                    span: expression.span,
                }),
                TypedStatement::Let(_) => unreachable!(),
            };
        }
        Expr::Actor(actor) => {
            let mut methods = Vec::with_capacity(actor.methods.len());
            let mut method_types = Vec::with_capacity(actor.methods.len());
            for method in &actor.methods {
                let plan = prepare_pattern_in(&environment.types, &method.pattern)?;
                validate_bindings(&plan)?;
                let bindings = plan.receiver_bindings();
                let reply = method
                    .reply
                    .as_ref()
                    .map(|annotation| {
                        Ok(Rc::new(TypeEvidence {
                            ty: resolve_type(&environment.types, annotation)?,
                            expression: annotation.span,
                            origin: EvidenceOrigin::ReplyAnnotation,
                            value_from: None,
                            methods: Vec::new(),
                            selector: None,
                        }))
                    })
                    .transpose()?;
                let mut scope = environment.extended(bindings.iter().cloned());
                // Each method owns its implicit reply-to actor; lexical aliases still capture.
                scope.reply_to = reply.as_ref().map(|annotation| Binding {
                    name: "^".into(),
                    pattern: annotation.expression,
                    value: Rc::new(TypeEvidence {
                        ty: Type::Actor(ActorType {
                            methods: vec![MethodType {
                                parameters: Vec::new(),
                                input: annotation.ty.clone(),
                                reply: None,
                            }],
                        }),
                        expression: annotation.expression,
                        origin: EvidenceOrigin::ReplyAnnotation,
                        value_from: None,
                        methods: vec![MethodEvidence {
                            parameters: Vec::new(),
                            span: method.span,
                            input: Expectation {
                                ty: annotation.ty.clone(),
                                origin: annotation.expression,
                            },
                            reply: None,
                        }],
                        selector: None,
                    }),
                });
                let body = check_statements_in(&scope, &method.body)?;
                method_types.push(MethodType {
                    parameters: plan
                        .parameters
                        .iter()
                        .map(|p| p.parameter.clone())
                        .collect(),
                    input: plan.expectation.ty.clone(),
                    reply: reply.as_ref().map(|annotation| annotation.ty.clone()),
                });
                methods.push(TypedMethod {
                    plan: plan.clone(),
                    parameters: plan.parameters,
                    span: method.span,
                    pattern: method.pattern.span,
                    expectation: plan.expectation,
                    reply,
                    bindings,
                    body,
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
        Expr::ReplyTo => {
            let binding =
                environment
                    .reply_to
                    .as_ref()
                    .ok_or(TypeError::ReplyToOutsideAnnotatedMethod {
                        span: expression.span,
                    })?;
            (
                binding.value.ty.clone(),
                Some(Rc::clone(&binding.value)),
                TypedExprKind::ReplyTo {
                    binding: binding.clone(),
                },
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
    };
    let methods = match &kind {
        TypedExprKind::Actor { methods } => methods
            .iter()
            .map(|method| MethodEvidence {
                parameters: method.parameters.clone(),
                span: method.span,
                input: method.expectation.clone(),
                reply: method.reply.clone(),
            })
            .collect(),
        _ => Vec::new(),
    };
    let selector = match &kind {
        TypedExprKind::Selector(s) => Some(s.map(|v| Rc::clone(&v.evidence))),
        _ => None,
    };
    let mut typed = TypedExpression {
        adaptations: Vec::new(),
        evidence: Rc::new(TypeEvidence {
            ty,
            expression: expression.span,
            origin: EvidenceOrigin::Expression,
            value_from,
            methods,
            selector,
        }),
        kind,
    };
    if let Some(expected) = expected {
        assert_type(&typed, expected)?;
        typed
            .adaptations
            .push(typed.evidence.ty.adaptation_to(&expected.ty).unwrap());
    }
    Ok(typed)
}

/// Check a lexical statement sequence without mutating its enclosing scope.
pub fn check_statements_in(
    environment: &Environment,
    statements: &[Loc<Stmt>],
) -> TypeResult<Vec<TypedStatement>> {
    let mut scope = environment.clone();
    let mut typed = Vec::with_capacity(statements.len());
    for statement in statements {
        typed.push(match &statement.value {
            Stmt::Let(binding) => {
                let checked = check_binding(&scope, &binding.pattern, &binding.value)?;
                scope = scope.extended(checked.bindings.iter().cloned());
                TypedStatement::Let(checked)
            }
            Stmt::Expr(expression) => match &expression.value {
                Expr::Send { callee, message } => {
                    type_send(&scope, expression.span, callee, message)?
                }
                _ => TypedStatement::Expr(synthesize_in(&scope, expression)?),
            },
        });
    }
    Ok(typed)
}

/// Type-check a successfully parsed program; parse diagnostics remain separate.
pub fn check_program(program: &Program) -> TypeResult<Vec<TypedStatement>> {
    check_statements_in(&Environment::default(), &program.statements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lexer, Pos, parse};

    fn program(source: &str) -> Program {
        let mut diagnostics = Vec::new();
        let program = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        program
    }

    fn expression(source: &str) -> Loc<Expr> {
        let Stmt::Expr(expr) = program(&format!("{source}.")).statements.remove(0).value else {
            panic!("expected expression")
        };
        expr
    }

    fn statements(source: &str) -> TypeResult<Vec<TypedStatement>> {
        check_program(&program(source))
    }

    fn value(statement: &TypedStatement) -> &TypedExpression {
        match statement {
            TypedStatement::Expr(value) => value,
            TypedStatement::Let(binding) => &binding.value,
            _ => panic!("no value"),
        }
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
                .map(|(input, reply)| MethodType {
                    parameters: Vec::new(),
                    input,
                    reply: Some(reply),
                })
                .collect(),
        })
    }

    fn bound(name: &str, ty: Type) -> Binding {
        Binding {
            name: name.into(),
            pattern: site(),
            value: Rc::new(TypeEvidence {
                ty,
                expression: site(),
                origin: EvidenceOrigin::ReceiverPattern,
                value_from: None,
                methods: Vec::new(),
                selector: None,
            }),
        }
    }

    #[test]
    fn statements_export_precise_bindings_and_keep_origins() {
        let typed = statements("let x = {}. x.").unwrap();
        let TypedStatement::Let(binding) = &typed[0] else {
            panic!()
        };
        assert_eq!(binding.instantiations.len(), 1);
        assert_eq!(
            *binding.instantiations[0]
                .parameter
                .parameter
                .variable
                .upper_bound,
            Type::Any
        );
        assert!(Rc::ptr_eq(
            &binding.instantiations[0].actual,
            &binding.bindings[0].value
        ));
        let reference = value(&typed[1]);
        assert_eq!(reference.evidence.ty, Type::UNIT);
        assert_eq!(reference.evidence.expression.start.col, 13);
        assert_eq!(reference.evidence.producer().expression.start.col, 9);
        let TypedExprKind::Variable { binding } = &reference.kind else {
            panic!()
        };
        assert_eq!(binding.pattern.start.col, 5);
        assert_eq!(binding.value.ty, Type::UNIT);
    }

    #[test]
    fn initializer_is_nonrecursive_and_shadowing_sees_outer() {
        for source in [
            "x.",
            "let x = x.",
            "let _ = {}. x.",
            "{ def (_) => missing. }.",
        ] {
            assert!(
                matches!(statements(source), Err(TypeError::UnboundVariable { .. })),
                "{source}"
            );
        }
        let typed = statements("let x = {}. let x = x. x.").unwrap();
        let TypedStatement::Let(outer) = &typed[0] else {
            panic!()
        };
        let TypedStatement::Let(inner) = &typed[1] else {
            panic!()
        };
        let TypedExprKind::Variable {
            binding: read_outer,
        } = &inner.value.kind
        else {
            panic!()
        };
        let TypedExprKind::Variable {
            binding: read_inner,
        } = &value(&typed[2]).kind
        else {
            panic!()
        };
        assert_eq!(read_outer.pattern, outer.bindings[0].pattern);
        assert_eq!(read_inner.pattern, inner.bindings[0].pattern);
        assert_ne!(read_outer.pattern, read_inner.pattern);
    }

    #[test]
    fn method_scopes_capture_but_do_not_leak() {
        let typed =
            statements("let x = {}. { def value: x => let y = x. y. def other => x. }.").unwrap();
        let TypedExprKind::Actor { methods } = &value(&typed[1]).kind else {
            panic!()
        };
        assert_eq!(
            value(&methods[0].body[1]).evidence.ty,
            methods[0].bindings[0].value.ty
        );
        assert_eq!(value(&methods[1].body[0]).evidence.ty, Type::UNIT);
        for source in [
            "{ def first => let x = {}. def second => x. }.",
            "{ def first => let x = {}. }. x.",
            "{ def first: x => x. def second => x. }.",
        ] {
            assert!(
                matches!(statements(source), Err(TypeError::UnboundVariable { .. })),
                "{source}"
            );
        }
        let environment = Environment::default();
        check_statements_in(&environment, &program("let x = {}.").statements).unwrap();
        assert!(environment.lookup("x").is_none());
    }

    #[test]
    fn annotations_preserve_precision_and_locate_components() {
        for (source, ty) in [
            ("let any x = {}. x.", Type::UNIT),
            ("let any ({} x) = {}. x.", Type::UNIT),
            ("let any (#x: a z: b) = #x: {} z: {}. b.", Type::UNIT),
        ] {
            let typed = statements(source).unwrap();
            assert_eq!(value(typed.last().unwrap()).evidence.ty, ty);
        }
        for source in [
            "let #x: y z: {} abc = #x: {} z: #bad. abc.",
            "let (#x: {} z: any) (#x: a z: b) = #x: #bad z: {}. b.",
        ] {
            let TypeError::Mismatch(error) = statements(source).unwrap_err() else {
                panic!()
            };
            assert_eq!(error.expected.ty, Type::UNIT);
            assert_eq!(
                error.expected.origin.start.col as usize,
                source.find("{}").unwrap() + 1
            );
            assert_eq!(
                error.actual.expression.start.col as usize,
                source.find("#bad").unwrap() + 1
            );
        }
        assert!(matches!(
            statements("let never _ = {}."),
            Err(TypeError::Mismatch(_))
        ));
        assert!(matches!(
            statements("let {} (any x) = {}."),
            Err(TypeError::InvalidAnnotation { .. })
        ));
        assert!(matches!(
            statements("let Missing x = {}."),
            Err(TypeError::UnknownType { .. })
        ));
    }

    #[test]
    fn named_types_and_rigid_receiver_bounds() {
        let parameter = TypeVariable::fresh(Type::UNIT);
        let mut environment =
            Environment::default().extended([bound("input", Type::Variable(parameter.clone()))]);
        environment.types = environment
            .types
            .extended("T", Type::Variable(parameter.clone()));
        let typed =
            check_statements_in(&environment, &program("let T x = input. x.").statements).unwrap();
        assert_eq!(value(&typed[1]).evidence.ty, Type::Variable(parameter));
        assert!(matches!(
            check_statements_in(&environment, &program("let T x = {}.").statements),
            Err(TypeError::Mismatch(_))
        ));
        let typed = synthesize_in(&environment, &expression("{ def (T x) => x. }")).unwrap();
        let Type::Actor(actor) = &typed.evidence.ty else {
            panic!()
        };
        assert_eq!(
            *actor.methods[0].parameters[0].variable.upper_bound,
            *environment.types.lookup("T").unwrap()
        );
        assert_eq!(actor.methods[0].reply, None);
    }

    #[test]
    fn selectors_destructure_precisely() {
        for (source, expected) in [
            ("let #x = #x. #done.", "#done"),
            ("let #x: y = #x: {}. y.", "{}"),
            ("let #outer: (#inner: x) = #outer: (#inner: {}). x.", "{}"),
            ("let (#tag: {}) x = #tag: {}. x.", "#tag: ({})"),
        ] {
            let typed = statements(source).unwrap();
            assert_eq!(
                value(typed.last().unwrap()).evidence.ty.to_string(),
                expected
            );
        }
        let TypeError::Mismatch(error) =
            statements("let #outer: (#inner: _) = #outer: #wrong. {}.").unwrap_err()
        else {
            panic!()
        };
        assert_eq!(error.expected.origin.start.col, 13);
        assert_eq!(error.actual.expression.start.col, 35);
    }

    #[test]
    fn no_reply_is_not_a_value_even_when_discarded_by_pattern() {
        assert!(matches!(
            statements("{ def go => } go.").unwrap()[0],
            TypedStatement::NoReplySend { .. }
        ));
        for source in [
            "let x = { def go => } go.",
            "let _ = { def go => } go.",
            "#value: ({ def go => } go).",
            "({ def go => } go) next.",
            "{ def (x) => } ({ def go => } go).",
        ] {
            assert!(
                matches!(statements(source), Err(TypeError::NoReplyValue { .. })),
                "{source}"
            );
        }
        assert!(matches!(
            synthesize(&expression("{ def go => } go")),
            Err(TypeError::NoReplyValue { .. })
        ));
    }

    #[test]
    fn annotated_methods_declare_replies_without_counting_sends() {
        for body in ["", "{}. ", "^ (#ok). ", "^ (#ok). ^ (#ok). "] {
            let source = format!("let a = {{ def go -> any => {body}}}. let reply = a go. reply.");
            let typed = statements(&source).unwrap();
            assert_eq!(value(&typed[2]).evidence.ty, Type::Any);
            let evidence = value(&typed[0]).evidence.methods[0].reply.as_ref().unwrap();
            assert_eq!(evidence.ty, Type::Any);
            assert_eq!(evidence.origin, EvidenceOrigin::ReplyAnnotation);
            assert_eq!(
                evidence.expression.start.col as usize,
                source.find("any").unwrap() + 1
            );
            assert_eq!(value(&typed[2]).evidence.producer(), evidence.as_ref());
        }
        assert!(statements("{ def go -> never => }. ").is_ok());
        assert!(matches!(
            statements("{ def go -> {} => ^ (#bad). }."),
            Err(TypeError::NoReceiver { .. })
        ));
        assert!(matches!(
            statements("{ def go -> any => let x = ^ (#ok). }."),
            Err(TypeError::NoReplyValue { .. })
        ));
        assert!(matches!(
            statements("{ def go -> Missing => }."),
            Err(TypeError::UnknownType { .. })
        ));
    }

    #[test]
    fn reply_to_is_method_local_but_aliases_are_lexical() {
        for source in [
            "^.",
            "{ def go => ^. }.",
            "{ def go -> any => { def inner => ^ (#ok). }. }.",
            "{ def go -> any => ^ (#ok). def other => ^ (#ok). }.",
        ] {
            let TypeError::ReplyToOutsideAnnotatedMethod { span } = statements(source).unwrap_err()
            else {
                panic!("{source}")
            };
            assert_eq!(
                &source[span.start.col as usize - 1..span.end.col as usize - 1],
                "^"
            );
        }
        assert!(
            statements("{ def go -> {} => { def inner -> #ok => ^ (#ok). }. ^ ({}). }.").is_ok()
        );
        assert!(matches!(
            statements("{ def go -> any => { def inner -> {} => ^ (#bad). }. }."),
            Err(TypeError::NoReceiver { .. })
        ));
        let typed =
            statements("{ def go -> #ok => let reply_to = ^. { def inner => reply_to (#ok). }. }.")
                .unwrap();
        let TypedExprKind::Actor { methods } = &value(&typed[0]).kind else {
            panic!()
        };
        assert!(matches!(
            value(&methods[0].body[0]).kind,
            TypedExprKind::ReplyTo { .. }
        ));
        assert_eq!(value(&methods[0].body[0]).evidence.ty.to_string(), "{ ok }");
        assert!(
            statements("{ def go -> any => let { (any) } reply_to = ^. reply_to (#ok). }.").is_ok()
        );
    }

    #[test]
    fn replies_keep_annotations_and_obey_receiver_bounds() {
        assert!(statements("let { go -> any } a = { def go -> any => ^ ({}). }. a go.").is_ok());
        assert!(matches!(
            statements("let {} x = { def go -> any => ^ ({}). } go."),
            Err(TypeError::Mismatch(_))
        ));
        assert!(matches!(
            statements("let { go } a = { def go -> {} => }."),
            Err(TypeError::Mismatch(_))
        ));
        assert!(statements("let x = { def ({} x) -> {} => ^ (x). } ({}). x.").is_ok());
        assert!(matches!(
            statements("{ def (x) -> {} => ^ (x). }."),
            Err(TypeError::NoReceiver { .. })
        ));
        let parameter = TypeVariable::fresh(Type::UNIT);
        let mut environment = Environment::default();
        environment.types = environment
            .types
            .extended("T", Type::Variable(parameter.clone()));
        let typed =
            synthesize_in(&environment, &expression("{ def (T x) -> T => ^ (x). }")).unwrap();
        let Type::Actor(actor) = typed.evidence.ty.clone() else {
            panic!()
        };
        assert_eq!(actor.methods[0].reply, Some(Type::Variable(parameter)));
    }

    #[test]
    fn receiver_bodies_are_sequences_not_implicit_replies() {
        for source in [
            "{ def go => }",
            "{ def go => {}. }",
            "{ def go => #reply. }",
        ] {
            let typed = synthesize(&expression(source)).unwrap();
            assert_eq!(typed.evidence.ty.to_string(), "{ go }");
            assert!(typed.evidence.methods[0].reply.is_none());
        }
        assert!(statements("let { go } a = { def go => {}. }. a go.").is_ok());
        for reply in ["{}", "any", "never"] {
            assert!(matches!(
                statements(&format!(
                    "let {{ go -> {reply} }} a = {{ def go => {{}}. }}."
                )),
                Err(TypeError::Mismatch(_))
            ));
        }
        assert!(
            statements("{ def first => let a = { def go => }. a go. {}. def second => }.").is_ok()
        );
    }

    #[test]
    fn invalid_receivers_and_dispatch_are_rejected() {
        for source in [
            "{ def x => {}. def x => {}. }.",
            "{ def (x) => x. def foo => {}. }.",
            "{ def value: x => x. def value: _ => {}. }.",
        ] {
            assert!(matches!(
                statements(source),
                Err(TypeError::OverlappingReceivers { .. })
            ));
        }
        assert!(matches!(
            statements("{ def pair: x with: x => x. }."),
            Err(TypeError::DuplicateBinding { .. })
        ));
        assert!(matches!(
            statements("{} foo."),
            Err(TypeError::NoReceiver { .. })
        ));
        assert!(matches!(
            statements("(#foo) foo."),
            Err(TypeError::NotActor { .. })
        ));
        assert!(matches!(
            statements("{ def x => x. }."),
            Err(TypeError::UnboundVariable { .. })
        ));
        assert!(matches!(
            statements("{ def ({} x) => x. } bad."),
            Err(TypeError::NoReceiver { .. })
        ));
    }

    #[test]
    fn generic_receivers_keep_parameters_without_reply_dependency() {
        let typed = synthesize(&expression("{ def (x) => let y = x. y. }")).unwrap();
        assert_eq!(typed.evidence.ty.to_string(), "{ <A <: any> (A) }");
        let TypedExprKind::Actor { methods } = &typed.kind else {
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
    fn supplied_reply_signatures_still_instantiate_generic_replies() {
        let parameter = TypeParameter::new(Type::Any);
        let variable = Type::Variable(parameter.variable.clone());
        let environment = Environment::default().extended([bound(
            "identity",
            Type::Actor(ActorType {
                methods: vec![MethodType {
                    parameters: vec![parameter],
                    input: Type::Selector(Selector::Keyword(vec![(
                        "value".into(),
                        variable.clone(),
                    )])),
                    reply: Some(variable),
                }],
            }),
        )]);
        let typed = synthesize_in(&environment, &expression("identity value: #hello")).unwrap();
        assert_eq!(typed.evidence.ty.to_string(), "#hello");
        let typed = check_statements_in(
            &environment,
            &program("let x = identity value: {}. x.").statements,
        )
        .unwrap();
        assert_eq!(value(&typed[1]).evidence.ty, Type::UNIT);
    }

    #[test]
    fn supplied_reply_evidence_retains_message_component_provenance() {
        let parameter = TypeParameter::new(Type::Any);
        let variable = Type::Variable(parameter.variable.clone());
        let mut binding = bound(
            "identity",
            Type::Actor(ActorType {
                methods: vec![MethodType {
                    parameters: vec![parameter.clone()],
                    input: Type::Selector(Selector::Keyword(vec![(
                        "value".into(),
                        variable.clone(),
                    )])),
                    reply: Some(variable.clone()),
                }],
            }),
        );
        Rc::make_mut(&mut binding.value)
            .methods
            .push(MethodEvidence {
                parameters: vec![LocatedParameter {
                    parameter,
                    origin: site(),
                }],
                span: site(),
                input: Expectation {
                    ty: variable.clone(),
                    origin: site(),
                },
                reply: Some(bound("result", variable).value),
            });
        let environment = Environment::default().extended([binding]);
        let source = "let alias = identity. let x = alias value: #hello. x.";
        let typed = check_statements_in(&environment, &program(source).statements).unwrap();
        let result = value(&typed[2]);
        assert_eq!(result.evidence.ty.to_string(), "#hello");
        assert_eq!(
            result.evidence.producer().expression.start.col as usize,
            source.find("#hello").unwrap() + 1
        );
    }

    #[test]
    fn structural_failures_keep_actor_evidence_through_aliases() {
        let typed = statements("let a = { def go => {}. }. a.").unwrap();
        let expected = Expectation {
            ty: actor(vec![(
                Type::Selector(Selector::Atomic("go".into())),
                Type::UNIT,
            )]),
            origin: site(),
        };
        let TypeError::Mismatch(error) = assert_type(value(&typed[1]), &expected).unwrap_err()
        else {
            panic!()
        };
        assert!(error.comparison_path.contains(&"reply mode".into()));
        assert_eq!(error.expected.origin, site());
        assert!(error.actual.producer().methods[0].reply.is_none());
        let TypeError::Mismatch(error) = check(&expression("{}"), &expected).unwrap_err() else {
            panic!()
        };
        assert_eq!(error.comparison_path, ["expected.methods[0]"]);
    }

    #[test]
    fn never_remains_a_value_type_and_does_not_hide_later_errors() {
        let environment = Environment::default().extended([bound("x", Type::Never)]);
        let typed =
            check_statements_in(&environment, &program("let y = x. {}.").statements).unwrap();
        assert_eq!(value(&typed[0]).evidence.ty, Type::Never);
        assert_eq!(value(&typed[1]).evidence.ty, Type::UNIT);
        assert!(matches!(
            check_statements_in(&environment, &program("let _ = x. missing.").statements),
            Err(TypeError::UnboundVariable { .. })
        ));
        assert_eq!(
            synthesize_in(&environment, &expression("x go"))
                .unwrap()
                .evidence
                .ty,
            Type::Never
        );
        assert_eq!(
            synthesize_in(&environment, &expression("#value: x"))
                .unwrap()
                .evidence
                .ty,
            Type::Never
        );
    }

    #[test]
    fn structural_width_variance_and_lattice() {
        let broad = actor(vec![(Type::Any, Type::UNIT)]);
        let narrow = actor(vec![(Type::UNIT, Type::Any)]);
        assert!(broad.is_subtype_of(&narrow));
        assert!(!narrow.is_subtype_of(&broad));
        assert!(broad.is_subtype_of(&Type::UNIT));
        assert!(!Type::UNIT.is_subtype_of(&broad));
        let overlaps = actor(vec![(Type::Any, Type::Any), (Type::Any, Type::UNIT)]);
        assert!(!overlaps.is_well_formed());
        assert!(!overlaps.is_subtype_of(&broad));
        assert!(!broad.is_subtype_of(&overlaps));
        let nested_actual = actor(vec![(narrow.clone(), broad.clone())]);
        let nested_expected = actor(vec![(broad, narrow)]);
        assert!(nested_actual.is_subtype_of(&nested_expected));
        assert!(!nested_expected.is_subtype_of(&nested_actual));
        let types = [Type::Never, Type::UNIT, Type::Any];
        for (i, actual) in types.iter().enumerate() {
            for (j, expected) in types.iter().enumerate() {
                assert_eq!(actual.is_subtype_of(expected), i <= j);
            }
        }
        for ty in [Type::Any, Type::UNIT] {
            assert_eq!(
                check(&expression("{}"), &Expectation { ty, origin: site() })
                    .unwrap()
                    .evidence
                    .ty,
                Type::UNIT
            );
        }
    }
}

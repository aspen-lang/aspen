use crate::Selector;

use std::{
    cell::RefCell,
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, OnceLock, Weak},
};

/// Structural method order is preserved, but does not imply runtime dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorType {
    pub methods: Vec<MethodType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodType {
    pub parameters: Vec<TypeParameter>,
    pub input: Type,
    /// None denotes no reply, not a value type.
    pub reply: Option<Type>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Never,
    Actor(ActorType),
    Selector(Selector<Type>),
    Bytes,
    String,
    Int,
    Float,
    SelectorFamily,
    Atom,
    OpTagged,
    KeywordTagged,
    Variable(TypeVariable),
    /// A transparent recursive reference; its body is stored as the upper bound.
    Alias(TypeVariable),
}

struct VariableGroup {
    ids: OnceLock<Vec<u64>>,
    bounds: OnceLock<Vec<Type>>,
}

enum GroupHandle {
    Strong(Arc<VariableGroup>),
    Weak(Weak<VariableGroup>),
}

pub struct TypeVariable {
    id: u64,
    index: usize,
    group: GroupHandle,
}

impl Clone for TypeVariable {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            index: self.index,
            group: GroupHandle::Strong(self.group()),
        }
    }
}

impl fmt::Debug for TypeVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypeVariable")
            .field("id", &self.id)
            .finish()
    }
}

impl PartialEq for TypeVariable {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for TypeVariable {}

impl TypeVariable {
    fn group(&self) -> Arc<VariableGroup> {
        match &self.group {
            GroupHandle::Strong(group) => group.clone(),
            GroupHandle::Weak(group) => group.upgrade().expect("live recursive variable group"),
        }
    }

    pub fn upper_bound(&self) -> Type {
        self.group()
            .bounds
            .get()
            .map_or(Type::UNIT, |bounds| bounds[self.index].clone())
    }

    pub fn fresh(upper_bound: Type) -> Self {
        Self::recursive_group(1, |_| vec![upper_bound]).remove(0)
    }

    /// All variables are in scope in every bound. The builder must not inspect
    /// their final bounds until it returns (uninitialized bounds expose UNIT).
    /// Callers must reject unguarded cycles and validate the completed bounds.
    pub fn recursive_group(count: usize, builder: impl FnOnce(&[Self]) -> Vec<Type>) -> Vec<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let ids = (0..count)
            .map(|_| {
                NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                    .expect("type variable identity exhausted")
            })
            .collect();
        Self::group_with_ids(ids, builder)
    }

    pub fn try_recursive_group<E>(
        count: usize,
        builder: impl FnOnce(&[Self]) -> Result<Vec<Type>, E>,
    ) -> Result<Vec<Self>, E> {
        let mut error = None;
        let variables = Self::recursive_group(count, |variables| match builder(variables) {
            Ok(bounds) => bounds,
            Err(err) => {
                error = Some(err);
                vec![Type::UNIT; count]
            }
        });
        match error {
            Some(error) => Err(error),
            None => Ok(variables),
        }
    }

    fn group_with_ids(ids: Vec<u64>, builder: impl FnOnce(&[Self]) -> Vec<Type>) -> Vec<Self> {
        let group = Arc::new(VariableGroup {
            ids: OnceLock::new(),
            bounds: OnceLock::new(),
        });
        let variables: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, &id)| Self {
                id,
                index,
                group: GroupHandle::Strong(group.clone()),
            })
            .collect();
        let mut bounds = builder(&variables);
        assert_eq!(bounds.len(), variables.len());
        // Flatten initialized dependencies into one arena. In particular this
        // absorbs nested quantified groups that capture an outer placeholder;
        // merely weakening direct self-edges would leak that indirect cycle.
        fn visit(ty: &mut Type, f: &mut impl FnMut(&mut TypeVariable)) {
            match ty {
                Type::Variable(v) | Type::Alias(v) => f(v),
                Type::Selector(selector) => match selector {
                    Selector::Atomic(_) => {}
                    Selector::Operator { value, .. } => visit(value, f),
                    Selector::Keyword(parts) => {
                        for (_, value) in parts {
                            visit(value, f);
                        }
                    }
                },
                Type::Actor(actor) => {
                    for method in &mut actor.methods {
                        for parameter in &mut method.parameters {
                            f(&mut parameter.variable);
                        }
                        visit(&mut method.input, f);
                        if let Some(reply) = &mut method.reply {
                            visit(reply, f);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut ids = ids;
        let mut index = 0;
        while index < bounds.len() {
            let mut additions = Vec::new();
            visit(&mut bounds[index], &mut |v| {
                if !ids.contains(&v.id) && v.group().bounds.get().is_some() {
                    ids.push(v.id);
                    additions.push(v.upper_bound());
                }
            });
            bounds.extend(additions);
            index += 1;
        }
        for bound in &mut bounds {
            visit(bound, &mut |v| {
                if let Some(index) = ids.iter().position(|id| *id == v.id) {
                    v.index = index;
                    v.group = GroupHandle::Weak(Arc::downgrade(&group));
                }
            });
        }
        group.ids.set(ids).expect("new recursive group identities");
        group.bounds.set(bounds).expect("new recursive group");
        variables
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeParameter {
    pub variable: TypeVariable,
}

impl TypeParameter {
    pub fn new(upper_bound: Type) -> Self {
        Self {
            variable: TypeVariable::fresh(upper_bound),
        }
    }
}

/// A structural subsumption proof, not an executable runtime adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdaptationPlan {
    /// Equal types, bottom elimination, top introduction, or alpha-renaming.
    Identity,
    Actor {
        provided_method_count: usize,
        methods: Vec<MethodCorrespondence>,
    },
    /// Payload plans in selector order; the selector shape is unchanged.
    Selector(Vec<AdaptationPlan>),
    /// Expose a variable's upper bound before continuing the adaptation.
    Variable(Box<AdaptationPlan>),
}

/// Indices refer to structural declarations, never executable dispatch slots.
/// Multiple required methods may correspond to the same provided method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodCorrespondence {
    pub provided_index: usize,
    pub expected_index: usize,
    pub adaptation: MethodAdaptationPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodAdaptationPlan {
    /// Inferred provided parameter instances to their substituted upper bounds.
    pub parameter_bounds: Vec<AdaptationPlan>,
    /// Required input to instantiated provided input (contravariant).
    pub input: AdaptationPlan,
    /// Instantiated provided reply to required reply (covariant).
    /// None means both methods have no reply.
    pub reply: Option<AdaptationPlan>,
}

impl AdaptationPlan {
    /// Whether the current full-message-dispatch ABI can reuse the value.
    /// Structural correspondence is proof evidence, not a dispatch table:
    /// width, reordering, and many-to-one matches do not change representation.
    pub fn is_identity(&self) -> bool {
        match self {
            Self::Identity => true,
            Self::Actor { methods, .. } => {
                methods.iter().all(|method| method.adaptation.is_identity())
            }
            Self::Selector(payloads) => payloads.iter().all(Self::is_identity),
            Self::Variable(bound) => bound.is_identity(),
        }
    }
}

impl MethodAdaptationPlan {
    pub fn is_identity(&self) -> bool {
        self.parameter_bounds
            .iter()
            .all(AdaptationPlan::is_identity)
            && self.input.is_identity()
            && self.reply.as_ref().is_none_or(AdaptationPlan::is_identity)
    }
}

thread_local! {
    static WELL_FORMED: RefCell<Vec<Type>> = const { RefCell::new(Vec::new()) };
    static EMPTY: RefCell<Vec<Type>> = const { RefCell::new(Vec::new()) };
    static DISJOINT: RefCell<Vec<(Type, Type)>> = const { RefCell::new(Vec::new()) };
    static INFER: RefCell<Vec<(Type, Type)>> = const { RefCell::new(Vec::new()) };
    static ALPHA: RefCell<Vec<(Type, Type, Vec<(TypeVariable, TypeVariable)>)>> = const { RefCell::new(Vec::new()) };
    static ADAPTATIONS: RefCell<Vec<(Type, Type)>> = const { RefCell::new(Vec::new()) };
}

// Active obligations, rather than completed results, implement coinduction.
// The guard is scoped so failed candidate methods cannot poison later matches.
fn recursive_check<K: Clone + PartialEq + 'static, R>(
    stack: &'static std::thread::LocalKey<RefCell<Vec<K>>>,
    key: K,
    repeated: R,
    check: impl FnOnce() -> R,
) -> R {
    if stack.with(|s| s.borrow().contains(&key)) {
        return repeated;
    }
    struct Pop<K: 'static>(&'static std::thread::LocalKey<RefCell<Vec<K>>>);
    impl<K> Drop for Pop<K> {
        fn drop(&mut self) {
            self.0.with(|s| {
                s.borrow_mut().pop();
            });
        }
    }
    stack.with(|s| s.borrow_mut().push(key));
    let _pop = Pop(stack);
    check()
}

impl Type {
    pub const UNIT: Self = Self::Actor(ActorType {
        methods: Vec::new(),
    });

    pub fn is_subtype_of(&self, expected: &Self) -> bool {
        self.adaptation_to(expected).is_some()
    }

    pub fn adaptation_to(&self, expected: &Self) -> Option<AdaptationPlan> {
        recursive_check(
            &ADAPTATIONS,
            (self.clone(), expected.clone()),
            Some(AdaptationPlan::Identity),
            || {
                if !self.is_well_formed() || !expected.is_well_formed() {
                    return None;
                }
                if self == expected || *self == Self::Never || *expected == Self::UNIT {
                    return Some(AdaptationPlan::Identity);
                }
                if self.primitive_domain().is_some_and(|actual| {
                    expected
                        .primitive_domain()
                        .is_some_and(|required| actual & required == actual)
                        && !matches!(expected, Self::Selector(_))
                }) {
                    return Some(AdaptationPlan::Identity);
                }
                match (self, expected) {
                    (Self::Alias(alias), _) => alias.upper_bound().adaptation_to(expected),
                    (_, Self::Alias(alias)) => self.adaptation_to(&alias.upper_bound()),
                    (Self::Selector(actual), Self::Selector(expected)) => {
                        let payloads = selector_pairs(actual, expected)?
                            .into_iter()
                            .map(|(a, b)| a.adaptation_to(b))
                            .collect::<Option<Vec<_>>>()?;
                        Some(AdaptationPlan::Selector(payloads))
                    }
                    (Self::Variable(variable), _) => Some(AdaptationPlan::Variable(Box::new(
                        variable.upper_bound().adaptation_to(expected)?,
                    ))),
                    (Self::Actor(actual), Self::Actor(expected)) => {
                        let methods = expected
                            .methods
                            .iter()
                            .enumerate()
                            .map(|(expected_index, required)| {
                                actual.methods.iter().enumerate().find_map(
                                    |(provided_index, provided)| {
                                        Some(MethodCorrespondence {
                                            provided_index,
                                            expected_index,
                                            adaptation: provided.adaptation_to(required)?,
                                        })
                                    },
                                )
                            })
                            .collect::<Option<Vec<_>>>()?;
                        Some(AdaptationPlan::Actor {
                            provided_method_count: actual.methods.len(),
                            methods,
                        })
                    }
                    _ => None,
                }
            },
        )
    }

    /// Simultaneous substitution; locally bound variables are renamed before descent.
    pub fn substitute(&self, substitutions: &[(TypeVariable, Type)]) -> Self {
        self.substitute_in(substitutions, &[])
    }

    fn substitute_in(
        &self,
        substitutions: &[(TypeVariable, Type)],
        aliases: &[(TypeVariable, TypeVariable)],
    ) -> Self {
        if substitutions.is_empty() && aliases.is_empty() {
            return self.clone();
        }
        match self {
            Self::Variable(variable) | Self::Alias(variable) => {
                let is_alias = matches!(self, Self::Alias(_));
                if is_alias {
                    if let Some((_, replacement)) =
                        aliases.iter().rev().find(|(key, _)| key == variable)
                    {
                        return Self::Alias(replacement.clone());
                    }
                } else if let Some((_, replacement)) =
                    substitutions.iter().rev().find(|(key, _)| key == variable)
                {
                    return replacement.clone();
                }
                let group = variable.group();
                // Rebuilt aliases may capture different substitutions, so they must
                // not compare equal to the original recursive equation.
                let ids = if is_alias {
                    group
                        .ids
                        .get()
                        .unwrap()
                        .iter()
                        .map(|_| TypeVariable::fresh(Self::UNIT).id)
                        .collect()
                } else {
                    group.ids.get().unwrap().clone()
                };
                let variables = TypeVariable::group_with_ids(ids, |renamed| {
                    let mut scope = substitutions.to_vec();
                    let mut alias_scope = aliases.to_vec();
                    for (index, replacement) in renamed.iter().enumerate() {
                        let old = TypeVariable {
                            id: group.ids.get().unwrap()[index],
                            index,
                            group: GroupHandle::Strong(group.clone()),
                        };
                        if !scope.iter().any(|(key, _)| key == &old) {
                            scope.push((old.clone(), Self::Variable(replacement.clone())));
                        }
                        alias_scope.push((old, replacement.clone()));
                    }
                    group
                        .bounds
                        .get()
                        .unwrap()
                        .iter()
                        .map(|bound| bound.substitute_in(&scope, &alias_scope))
                        .collect()
                });
                if is_alias {
                    Self::Alias(variables[variable.index].clone())
                } else {
                    Self::Variable(variables[variable.index].clone())
                }
            }
            Self::Selector(selector) => {
                Self::Selector(selector.map(|ty| ty.substitute_in(substitutions, aliases)))
            }
            Self::Actor(actor) => Self::Actor(ActorType {
                methods: actor
                    .methods
                    .iter()
                    .map(|method| {
                        let mut scope = substitutions.to_vec();
                        let renamed =
                            TypeVariable::recursive_group(method.parameters.len(), |variables| {
                                scope.extend(
                                    method.parameters.iter().zip(variables).map(|(p, v)| {
                                        (p.variable.clone(), Self::Variable(v.clone()))
                                    }),
                                );
                                method
                                    .parameters
                                    .iter()
                                    .map(|p| {
                                        p.variable.upper_bound().substitute_in(&scope, aliases)
                                    })
                                    .collect()
                            });
                        let parameters = renamed
                            .into_iter()
                            .map(|variable| TypeParameter { variable })
                            .collect();
                        MethodType {
                            parameters,
                            input: method.input.substitute_in(&scope, aliases),
                            reply: method
                                .reply
                                .as_ref()
                                .map(|ty| ty.substitute_in(&scope, aliases)),
                        }
                    })
                    .collect(),
            }),
            _ => self.clone(),
        }
    }
}

impl ActorType {
    pub fn has_disjoint_inputs(&self) -> bool {
        let inputs: Vec<_> = self
            .methods
            .iter()
            .map(MethodType::accepted_input)
            .collect();
        inputs.iter().enumerate().all(|(i, input)| {
            inputs[i + 1..]
                .iter()
                .all(|other| input.is_disjoint_from(other))
        })
    }
}

impl Type {
    pub fn is_well_formed(&self) -> bool {
        recursive_check(&WELL_FORMED, self.clone(), true, || match self {
            Self::Actor(actor) => {
                actor.has_disjoint_inputs()
                    && actor.methods.iter().all(|method| {
                        method
                            .parameters
                            .iter()
                            .all(|p| p.variable.upper_bound().is_well_formed())
                            && method.input.is_well_formed()
                            && method.reply.as_ref().is_none_or(Type::is_well_formed)
                    })
            }
            Self::Selector(selector) => selector.values().iter().all(|ty| ty.is_well_formed()),
            Self::Variable(variable) | Self::Alias(variable) => {
                variable.upper_bound().is_well_formed()
            }
            _ => true,
        })
    }

    fn is_empty(&self) -> bool {
        recursive_check(&EMPTY, self.clone(), true, || match self {
            Self::Never => true,
            Self::Variable(variable) | Self::Alias(variable) => variable.upper_bound().is_empty(),
            Self::Selector(selector) => selector.values().iter().any(|ty| ty.is_empty()),
            _ => false,
        })
    }

    // Disjoint primitive domains; selector families are unions of selector shapes.
    fn primitive_domain(&self) -> Option<u8> {
        match self {
            // UTF-8 strings occupy one subset of the binary domain.
            Self::Bytes => Some(1 | 64),
            Self::String => Some(1),
            Self::Int => Some(2),
            Self::Float => Some(4),
            Self::SelectorFamily => Some(8 | 16 | 32),
            Self::Atom | Self::Selector(Selector::Atomic(_)) => Some(8),
            Self::OpTagged | Self::Selector(Selector::Operator { .. }) => Some(16),
            Self::KeywordTagged | Self::Selector(Selector::Keyword(_)) => Some(32),
            _ => None,
        }
    }

    /// A conservative proof that no inhabited value belongs to both types.
    pub fn is_disjoint_from(&self, other: &Self) -> bool {
        recursive_check(&DISJOINT, (self.clone(), other.clone()), false, || {
            if self.is_empty() || other.is_empty() {
                return true;
            }
            match (self, other) {
                (Self::Never, _) | (_, Self::Never) => true,
                (Self::Variable(v) | Self::Alias(v), _) => v.upper_bound().is_disjoint_from(other),
                (_, Self::Variable(v) | Self::Alias(v)) => self.is_disjoint_from(&v.upper_bound()),
                (Self::Selector(a), Self::Selector(b)) => selector_pairs(a, b)
                    .is_none_or(|pairs| pairs.into_iter().any(|(a, b)| a.is_disjoint_from(b))),
                // A structural actor requirement alone cannot rule out a primitive.
                _ => self
                    .primitive_domain()
                    .zip(other.primitive_domain())
                    .is_some_and(|(a, b)| a & b == 0),
            }
        })
    }
}

impl MethodType {
    pub fn accepted_input(&self) -> Type {
        let mut substitutions = Vec::new();
        for parameter in &self.parameters {
            let bound = parameter.variable.upper_bound().substitute(&substitutions);
            substitutions.push((parameter.variable.clone(), bound));
        }
        self.input.substitute(&substitutions)
    }

    /// Outer None means not applicable; Some(None) means an accepted no-reply send.
    pub fn instantiate(&self, message: &Type) -> Option<Option<Type>> {
        let instances = self.infer_instances(message)?;
        Some(self.reply.as_ref().map(|ty| ty.substitute(&instances)))
    }

    pub fn infer_instances(&self, message: &Type) -> Option<Vec<(TypeVariable, Type)>> {
        if !message.is_well_formed()
            || !self.input.is_well_formed()
            || !self.reply.as_ref().is_none_or(Type::is_well_formed)
        {
            return None;
        }
        let mut instances = Vec::new();
        infer_input(&self.input, message, &self.parameters, &mut instances)?;
        // Default unobserved parameters simultaneously so declaration order
        // cannot affect forward-dependent bounds. Preserve recursive defaults
        // as a finite variable group rather than repeatedly expanding trees.
        let missing: Vec<_> = self
            .parameters
            .iter()
            .filter(|p| !instances.iter().any(|(v, _)| *v == p.variable))
            .collect();
        if !missing.is_empty() {
            let defaults = TypeVariable::recursive_group(missing.len(), |variables| {
                let mut scope = instances.clone();
                scope.extend(
                    missing
                        .iter()
                        .zip(variables)
                        .map(|(p, v)| (p.variable.clone(), Type::Variable(v.clone()))),
                );
                missing
                    .iter()
                    .map(|p| p.variable.upper_bound().substitute(&scope))
                    .collect()
            });
            instances.extend(missing.iter().zip(&defaults).map(|(p, v)| {
                let mut bound = v.upper_bound();
                while let Type::Variable(variable) = &bound {
                    bound = variable.upper_bound();
                }
                (p.variable.clone(), bound)
            }));
        }
        for parameter in &self.parameters {
            let bound = parameter.variable.upper_bound().substitute(&instances);
            let (_, instance) = instances.iter().find(|(v, _)| *v == parameter.variable)?;
            if !instance.is_subtype_of(&bound) {
                return None;
            }
        }
        message
            .is_subtype_of(&self.input.substitute(&instances))
            .then_some(instances)
    }

    pub fn is_subtype_of(&self, expected: &Self) -> bool {
        self.adaptation_to(expected).is_some()
    }

    pub fn adaptation_to(&self, expected: &Self) -> Option<MethodAdaptationPlan> {
        if !self.input.is_well_formed()
            || !self.reply.as_ref().is_none_or(Type::is_well_formed)
            || !expected.input.is_well_formed()
            || !expected.reply.as_ref().is_none_or(Type::is_well_formed)
        {
            return None;
        }
        if alpha_method(self, expected, &[]) {
            // Alpha-renaming preserves representation, including nested actors.
            // Keep this escape hatch: inference intentionally does not solve
            // every externally supplied quantified signature (e.g. actor inputs).
            return Some(MethodAdaptationPlan {
                parameter_bounds: Vec::new(),
                input: AdaptationPlan::Identity,
                reply: self.reply.as_ref().map(|_| AdaptationPlan::Identity),
            });
        }
        // Required quantifiers remain rigid; only the provided method is instantiated.
        let mut skolems = Vec::new();
        let _rigids = TypeVariable::recursive_group(expected.parameters.len(), |variables| {
            skolems.extend(
                expected
                    .parameters
                    .iter()
                    .zip(variables)
                    .map(|(p, v)| (p.variable.clone(), Type::Variable(v.clone()))),
            );
            expected
                .parameters
                .iter()
                .map(|p| p.variable.upper_bound().substitute(&skolems))
                .collect()
        });
        let required_input = expected.input.substitute(&skolems);
        let required_reply = expected.reply.as_ref().map(|ty| ty.substitute(&skolems));
        let instances = self.infer_instances(&required_input)?;
        let parameter_bounds = self
            .parameters
            .iter()
            .map(|parameter| {
                let (_, instance) = instances
                    .iter()
                    .find(|(variable, _)| *variable == parameter.variable)?;
                instance.adaptation_to(&parameter.variable.upper_bound().substitute(&instances))
            })
            .collect::<Option<Vec<_>>>()?;
        let input = required_input.adaptation_to(&self.input.substitute(&instances))?;
        let reply = match (&self.reply, &required_reply) {
            (None, None) => None,
            (Some(actual), Some(expected)) => {
                Some(actual.substitute(&instances).adaptation_to(expected)?)
            }
            _ => return None,
        };
        Some(MethodAdaptationPlan {
            parameter_bounds,
            input,
            reply,
        })
    }
}

fn infer_input(
    pattern: &Type,
    message: &Type,
    parameters: &[TypeParameter],
    instances: &mut Vec<(TypeVariable, Type)>,
) -> Option<()> {
    recursive_check(&INFER, (pattern.clone(), message.clone()), Some(()), || {
        if let Type::Alias(alias) = pattern {
            return infer_input(&alias.upper_bound(), message, parameters, instances);
        }
        if let Type::Alias(alias) = message {
            return infer_input(pattern, &alias.upper_bound(), parameters, instances);
        }
        match pattern {
            Type::Variable(variable) if parameters.iter().any(|p| p.variable == *variable) => {
                if let Some((_, previous)) = instances.iter_mut().find(|(v, _)| v == variable) {
                    if !message.is_subtype_of(previous) {
                        if previous.is_subtype_of(message) {
                            *previous = message.clone();
                        } else {
                            *previous = variable.upper_bound();
                        }
                    }
                } else {
                    instances.push((variable.clone(), message.clone()));
                }
            }
            Type::Selector(pattern) => {
                // A captured message variable can expose selector structure through its bound.
                if let Type::Variable(variable) = message {
                    return infer_input(
                        &Type::Selector(pattern.clone()),
                        &variable.upper_bound(),
                        parameters,
                        instances,
                    );
                }
                if let Type::Selector(message) = message {
                    for (pattern, message) in selector_pairs(pattern, message)? {
                        infer_input(pattern, message, parameters, instances)?;
                    }
                } else if *message != Type::Never {
                    return None;
                }
            }
            _ => {}
        }
        Some(())
    })
}

fn selector_pairs<'a>(
    left: &'a Selector<Type>,
    right: &'a Selector<Type>,
) -> Option<Vec<(&'a Type, &'a Type)>> {
    match (left, right) {
        (Selector::Atomic(a), Selector::Atomic(b)) if a == b => Some(vec![]),
        (
            Selector::Operator {
                operator: a,
                value: av,
            },
            Selector::Operator {
                operator: b,
                value: bv,
            },
        ) if a == b => Some(vec![(av, bv)]),
        (Selector::Keyword(a), Selector::Keyword(b))
            if a.len() == b.len() && a.iter().zip(b).all(|((a, _), (b, _))| a == b) =>
        {
            Some(a.iter().zip(b).map(|((_, a), (_, b))| (a, b)).collect())
        }
        _ => None,
    }
}

fn alpha_type(left: &Type, right: &Type, scope: &[(TypeVariable, TypeVariable)]) -> bool {
    recursive_check(
        &ALPHA,
        (left.clone(), right.clone(), scope.to_vec()),
        true,
        || match (left, right) {
            (Type::Alias(alias), _) => alpha_type(&alias.upper_bound(), right, scope),
            (_, Type::Alias(alias)) => alpha_type(left, &alias.upper_bound(), scope),
            (Type::Never, Type::Never)
            | (Type::Bytes, Type::Bytes)
            | (Type::String, Type::String)
            | (Type::Int, Type::Int)
            | (Type::Float, Type::Float)
            | (Type::SelectorFamily, Type::SelectorFamily)
            | (Type::Atom, Type::Atom)
            | (Type::OpTagged, Type::OpTagged)
            | (Type::KeywordTagged, Type::KeywordTagged) => true,
            (Type::Variable(left), Type::Variable(right)) => {
                if let Some((_, mapped)) = scope.iter().rev().find(|(key, _)| key == left) {
                    mapped == right
                } else {
                    left == right && !scope.iter().any(|(_, bound)| bound == right)
                }
            }
            (Type::Selector(left), Type::Selector(right)) => selector_pairs(left, right)
                .is_some_and(|pairs| pairs.into_iter().all(|(a, b)| alpha_type(a, b, scope))),
            (Type::Actor(left), Type::Actor(right)) => {
                left.methods.len() == right.methods.len()
                    && left
                        .methods
                        .iter()
                        .zip(&right.methods)
                        .all(|(a, b)| alpha_method(a, b, scope))
            }
            _ => false,
        },
    )
}

fn alpha_method(
    left: &MethodType,
    right: &MethodType,
    outer: &[(TypeVariable, TypeVariable)],
) -> bool {
    if left.parameters.len() != right.parameters.len() {
        return false;
    }
    let mut scope = outer.to_vec();
    for (a, b) in left.parameters.iter().zip(&right.parameters) {
        let pair = (a.variable.clone(), b.variable.clone());
        if !scope.contains(&pair) {
            scope.push(pair);
        }
    }
    for (a, b) in left.parameters.iter().zip(&right.parameters) {
        if !alpha_type(&a.variable.upper_bound(), &b.variable.upper_bound(), &scope) {
            return false;
        }
    }
    alpha_type(&left.input, &right.input, &scope)
        && match (&left.reply, &right.reply) {
            (None, None) => true,
            (Some(left), Some(right)) => alpha_type(left, right, &scope),
            _ => false,
        }
}

fn variable_name(index: usize) -> String {
    if index < 26 {
        ((b'A' + index as u8) as char).to_string()
    } else {
        format!("T{}", index + 1)
    }
}

fn display_type(
    ty: &Type,
    f: &mut fmt::Formatter<'_>,
    scope: &mut Vec<TypeVariable>,
) -> fmt::Result {
    match ty {
        Type::Never => f.write_str("never"),
        Type::Bytes => f.write_str("bytes"),
        Type::String => f.write_str("string"),
        Type::Int => f.write_str("int"),
        Type::Float => f.write_str("float"),
        Type::SelectorFamily => f.write_str("selector"),
        Type::Atom => f.write_str("atom"),
        Type::OpTagged => f.write_str("optagged"),
        Type::KeywordTagged => f.write_str("keywordtagged"),
        Type::Alias(alias) => {
            if let Some(index) = scope.iter().position(|bound| bound == alias) {
                return write!(f, "rec{}", index + 1);
            }
            let base = scope.len();
            scope.push(alias.clone());
            write!(f, "rec{} = ", base + 1)?;
            let result = display_type(&alias.upper_bound(), f, scope);
            scope.truncate(base);
            result
        }
        Type::Variable(variable) => {
            let index = match scope.iter().position(|bound| bound == variable) {
                Some(index) => index,
                None => {
                    scope.push(variable.clone());
                    scope.len() - 1
                }
            };
            f.write_str(&variable_name(index))
        }
        Type::Selector(selector) => {
            f.write_str("#")?;
            display_selector(selector, f, scope)
        }
        Type::Actor(actor) => {
            if actor.methods.is_empty() {
                return f.write_str("{}");
            }
            f.write_str("{ ")?;
            for (index, method) in actor.methods.iter().enumerate() {
                if index > 0 {
                    f.write_str(". ")?;
                }
                let mut local = scope.clone();
                if !method.parameters.is_empty() {
                    f.write_str("<")?;
                    let base = local.len();
                    local.extend(method.parameters.iter().map(|p| p.variable.clone()));
                    for (index, parameter) in method.parameters.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        let name = variable_name(base + index);
                        write!(f, "{name} <: ")?;
                        display_type(&parameter.variable.upper_bound(), f, &mut local)?;
                    }
                    f.write_str("> ")?;
                }
                if let Type::Selector(selector) = &method.input {
                    display_selector(selector, f, &mut local)?;
                } else {
                    f.write_str("(")?;
                    display_type(&method.input, f, &mut local)?;
                    f.write_str(")")?;
                }
                if let Some(reply) = &method.reply {
                    f.write_str(" -> ")?;
                    display_type(reply, f, &mut local)?;
                }
            }
            f.write_str(" }")
        }
    }
}

fn display_selector(
    selector: &Selector<Type>,
    f: &mut fmt::Formatter<'_>,
    scope: &mut Vec<TypeVariable>,
) -> fmt::Result {
    match selector {
        Selector::Atomic(name) => f.write_str(name),
        Selector::Operator { operator, value } => {
            write!(f, "{operator} (")?;
            display_type(value, f, scope)?;
            f.write_str(")")
        }
        Selector::Keyword(parts) => {
            for (i, (name, value)) in parts.iter().enumerate() {
                if i > 0 {
                    f.write_str(" ")?;
                }
                write!(f, "{name}: (")?;
                display_type(value, f, scope)?;
                f.write_str(")")?;
            }
            Ok(())
        }
    }
}

fn free_variables(ty: &Type, bound: &[TypeVariable], free: &mut Vec<TypeVariable>) {
    match ty {
        Type::Alias(alias) => {
            if !bound.contains(alias) {
                let mut local = bound.to_vec();
                local.push(alias.clone());
                free_variables(&alias.upper_bound(), &local, free);
            }
        }
        Type::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => free_variables(value, bound, free),
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    free_variables(value, bound, free);
                }
            }
        },
        Type::Variable(variable) => {
            if !bound.contains(variable) && !free.contains(variable) {
                free.push(variable.clone());
            }
        }
        Type::Actor(actor) => {
            for method in &actor.methods {
                let mut local = bound.to_vec();
                local.extend(method.parameters.iter().map(|p| p.variable.clone()));
                for parameter in &method.parameters {
                    free_variables(&parameter.variable.upper_bound(), &local, free);
                }
                free_variables(&method.input, &local, free);
                if let Some(reply) = &method.reply {
                    free_variables(reply, &local, free);
                }
            }
        }
        _ => {}
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut scope = Vec::new();
        free_variables(self, &[], &mut scope);
        display_type(self, f, &mut scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(method: MethodType) -> Type {
        Type::Actor(ActorType {
            methods: vec![method],
        })
    }
    fn mono(input: Type, reply: Type) -> MethodType {
        MethodType {
            parameters: vec![],
            input,
            reply: Some(reply),
        }
    }
    fn identity(bound: Type) -> MethodType {
        let parameter = TypeParameter::new(bound);
        let ty = Type::Variable(parameter.variable.clone());
        MethodType {
            parameters: vec![parameter],
            input: ty.clone(),
            reply: Some(ty),
        }
    }

    #[test]
    fn recursive_aliases_with_quantified_methods_terminate() {
        fn recursive() -> Type {
            let aliases = TypeVariable::recursive_group(1, |aliases| {
                let parameter = TypeParameter::new(Type::UNIT);
                vec![actor(MethodType {
                    parameters: vec![parameter.clone()],
                    input: Type::Variable(parameter.variable.clone()),
                    reply: Some(Type::Alias(aliases[0].clone())),
                })]
            });
            Type::Alias(aliases[0].clone())
        }
        let a = recursive();
        let b = recursive();
        assert!(a.is_subtype_of(&b));
        assert!(b.is_subtype_of(&a));
        let renamed = a.substitute(&[(TypeVariable::fresh(Type::UNIT), Type::Int)]);
        assert!(a.is_subtype_of(&renamed));
    }

    #[test]
    fn recursive_aliases_are_transparent_on_both_sides_and_do_not_leak() {
        let aliases = TypeVariable::recursive_group(2, |v| {
            vec![
                actor(mono(atom("next"), Type::Alias(v[1].clone()))),
                actor(mono(atom("next"), Type::Alias(v[0].clone()))),
            ]
        });
        let weak = Arc::downgrade(&aliases[0].group());
        let a = Type::Alias(aliases[0].clone());
        let b = Type::Alias(aliases[1].clone());
        assert!(a.is_well_formed());
        assert!(a.is_subtype_of(&b));
        assert!(b.is_subtype_of(&a));
        assert!(a.is_subtype_of(&aliases[0].upper_bound()));
        assert!(aliases[0].upper_bound().is_subtype_of(&a));
        assert!(!Type::Int.is_subtype_of(&a));
        assert!(!a.is_subtype_of(&Type::Variable(aliases[0].clone())));
        assert!(a.adaptation_to(&b).unwrap().is_identity());
        assert!(a.to_string().contains("rec1"));
        drop(a);
        drop(b);
        drop(aliases);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn aliases_preserve_captures_during_substitution_and_inference() {
        let parameter = TypeParameter::new(Type::UNIT);
        let aliases = TypeVariable::recursive_group(1, |v| {
            vec![actor(mono(
                keyword("value", Type::Variable(parameter.variable.clone())),
                Type::Alias(v[0].clone()),
            ))]
        });
        let alias = Type::Alias(aliases[0].clone());
        let ints = alias.substitute(&[(parameter.variable.clone(), Type::Int)]);
        let strings = alias.substitute(&[(parameter.variable.clone(), Type::String)]);
        assert_ne!(ints, strings);
        assert!(!ints.is_subtype_of(&strings));
        assert!(ints.is_subtype_of(&ints));
        let input = Type::Alias(TypeVariable::fresh(keyword(
            "value",
            Type::Variable(parameter.variable.clone()),
        )));
        let method = MethodType {
            parameters: vec![parameter.clone()],
            input,
            reply: Some(Type::Variable(parameter.variable)),
        };
        assert_eq!(
            method.instantiate(&keyword("value", Type::Int)),
            Some(Some(Type::Int))
        );
        let int_alias = Type::Alias(TypeVariable::fresh(Type::Int));
        assert!(int_alias.is_subtype_of(&Type::Int));
        assert!(Type::Int.is_subtype_of(&int_alias));
        assert!(int_alias.is_disjoint_from(&Type::String));
    }

    #[test]
    fn mutually_recursive_bounds_are_finite_and_rigid() {
        let variables = TypeVariable::recursive_group(2, |v| {
            vec![
                actor(mono(atom("next"), Type::Variable(v[1].clone()))),
                actor(mono(atom("next"), Type::Variable(v[0].clone()))),
            ]
        });
        let weak = Arc::downgrade(&variables[0].group());
        let a = Type::Variable(variables[0].clone());
        assert!(a.is_well_formed());
        assert!(a.is_subtype_of(&actor(mono(atom("next"), Type::UNIT))));
        assert!(!a.is_subtype_of(&Type::Variable(variables[1].clone())));
        let method = MethodType {
            parameters: variables
                .iter()
                .cloned()
                .map(|variable| TypeParameter { variable })
                .collect(),
            input: a.clone(),
            reply: Some(a.clone()),
        };
        let original = actor(method);
        assert_eq!(
            original.to_string(),
            "{ <A <: { next -> B }, B <: { next -> A }> (A) -> A }"
        );
        let renamed = original.substitute(&[(TypeVariable::fresh(Type::UNIT), Type::Int)]);
        assert!(original.is_subtype_of(&renamed));
        drop(renamed);
        drop(original);
        drop(a);
        drop(variables);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn nested_groups_capturing_recursive_bounds_do_not_leak() {
        let variables = TypeVariable::recursive_group(1, |outer| {
            let inner = TypeVariable::fresh(Type::Variable(outer[0].clone()));
            vec![actor(MethodType {
                parameters: vec![TypeParameter {
                    variable: inner.clone(),
                }],
                input: Type::Variable(inner),
                reply: Some(Type::Variable(outer[0].clone())),
            })]
        });
        let weak = Arc::downgrade(&variables[0].group());
        assert!(variables[0].upper_bound().is_well_formed());
        drop(variables);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn recursive_bounds_check_concrete_actor_instances() {
        let variables = TypeVariable::recursive_group(2, |v| {
            vec![
                actor(mono(atom("next"), Type::Variable(v[1].clone()))),
                actor(mono(atom("next"), Type::Variable(v[0].clone()))),
            ]
        });
        let method = MethodType {
            parameters: variables
                .iter()
                .cloned()
                .map(|variable| TypeParameter { variable })
                .collect(),
            input: keyword("a", Type::Variable(variables[0].clone())),
            reply: Some(Type::Variable(variables[0].clone())),
        };
        let concrete = actor(mono(atom("next"), Type::Never));
        assert_eq!(
            method.instantiate(&keyword("a", concrete.clone())),
            Some(Some(concrete))
        );
        assert_eq!(
            method.instantiate(&keyword("a", actor(mono(atom("next"), Type::Int)))),
            None
        );
    }

    #[test]
    fn recursive_selector_bounds_terminate() {
        let variables = TypeVariable::recursive_group(1, |v| {
            vec![keyword("next", Type::Variable(v[0].clone()))]
        });
        let ty = Type::Variable(variables[0].clone());
        assert!(ty.is_well_formed());
        assert!(ty.is_disjoint_from(&ty));
        assert!(ty.is_disjoint_from(&atom("stop")));
        let replaced = ty.substitute(&[(TypeVariable::fresh(Type::UNIT), Type::Int)]);
        assert_eq!(replaced, ty);
    }

    #[test]
    fn reply_modes_are_distinct_from_all_value_types() {
        let no_reply = MethodType {
            parameters: vec![],
            input: Type::UNIT,
            reply: None,
        };
        assert_eq!(no_reply.instantiate(&Type::UNIT), Some(None));
        assert!(no_reply.is_subtype_of(&no_reply));
        assert_eq!(actor(no_reply.clone()).to_string(), "{ ({}) }");
        for ty in [Type::Never, Type::UNIT, Type::Int] {
            let reply = mono(Type::UNIT, ty.clone());
            assert_eq!(reply.instantiate(&Type::UNIT), Some(Some(ty)));
            assert!(!reply.is_subtype_of(&no_reply));
            assert!(!no_reply.is_subtype_of(&reply));
        }
        let generic = MethodType {
            reply: None,
            ..identity(Type::UNIT)
        };
        let renamed =
            actor(generic.clone()).substitute(&[(TypeVariable::fresh(Type::UNIT), Type::UNIT)]);
        assert!(actor(generic).is_subtype_of(&renamed));
        assert!(no_reply.is_subtype_of(&MethodType {
            input: Type::UNIT,
            ..no_reply.clone()
        }));
        assert_eq!(
            MethodType {
                input: atom("go"),
                ..no_reply
            }
            .instantiate(&atom("stop")),
            None
        );
    }

    #[test]
    fn universal_identity_and_rigid_replies() {
        let id = identity(Type::UNIT);
        assert!(id.is_subtype_of(&mono(Type::UNIT, Type::UNIT)));
        assert!(id.is_subtype_of(&identity(Type::UNIT)));
        assert!(!mono(Type::UNIT, Type::UNIT).is_subtype_of(&id));
        assert!(!mono(Type::UNIT, Type::UNIT).is_subtype_of(&id));
        assert!(mono(Type::UNIT, Type::Never).is_subtype_of(&id));
        assert_eq!(actor(id).to_string(), "{ <A <: {}> (A) -> A }");
    }

    #[test]
    fn bounds_restrict_instantiation_and_do_not_identify_variables() {
        let id = identity(Type::Int);
        assert!(id.is_subtype_of(&mono(Type::Int, Type::Int)));
        assert!(!id.is_subtype_of(&mono(Type::UNIT, Type::UNIT)));
        assert!(!id.is_subtype_of(&identity(Type::UNIT)));
        assert!(identity(Type::UNIT).is_subtype_of(&id));
        let a = TypeVariable::fresh(Type::UNIT);
        let b = TypeVariable::fresh(Type::UNIT);
        assert_ne!(a, b);
        assert!(Type::Variable(a.clone()).is_subtype_of(&Type::UNIT));
        assert!(!Type::Variable(a).is_subtype_of(&Type::Variable(b)));
    }

    #[test]
    fn nested_captures_substitute_without_capture() {
        let outer = TypeParameter::new(Type::UNIT);
        let inner = TypeParameter::new(Type::UNIT);
        let outer_ty = Type::Variable(outer.variable.clone());
        let inner_ty = Type::Variable(inner.variable.clone());
        let nested = actor(MethodType {
            parameters: vec![inner.clone()],
            input: inner_ty.clone(),
            reply: Some(outer_ty.clone()),
        });
        let method = MethodType {
            parameters: vec![outer.clone()],
            input: outer_ty,
            reply: Some(nested.clone()),
        };
        assert_eq!(
            actor(method.clone()).to_string(),
            "{ <A <: {}> (A) -> { <B <: {}> (B) -> A } }"
        );
        assert!(method.is_subtype_of(&mono(Type::Int, actor(mono(Type::UNIT, Type::Int)))));
        assert!(!method.is_subtype_of(&mono(Type::UNIT, actor(mono(Type::UNIT, Type::Int)))));
        let replaced = nested.substitute(&[(outer.variable, inner_ty.clone())]);
        let Type::Actor(replaced) = replaced else {
            panic!()
        };
        assert_eq!(replaced.methods[0].reply, Some(inner_ty));
        assert_ne!(replaced.methods[0].parameters[0].variable, inner.variable);
        assert_eq!(
            replaced.methods[0].input,
            Type::Variable(replaced.methods[0].parameters[0].variable.clone())
        );
    }

    #[test]
    fn substitution_respects_bound_variables() {
        let id = identity(Type::UNIT);
        let substituted =
            actor(id.clone()).substitute(&[(id.parameters[0].variable.clone(), Type::UNIT)]);
        assert!(substituted.is_subtype_of(&actor(id.clone())));
        assert!(actor(id).is_subtype_of(&substituted));
    }
    fn atom(name: &str) -> Type {
        Type::Selector(Selector::Atomic(name.into()))
    }

    fn keyword(name: &str, value: Type) -> Type {
        Type::Selector(Selector::Keyword(vec![(name.into(), value)]))
    }

    #[test]
    fn recursive_selector_instantiation_and_bounds() {
        let a = TypeParameter::new(Type::UNIT);
        let b = TypeParameter::new(Type::Int);
        let method = MethodType {
            parameters: vec![a.clone(), b.clone()],
            input: Type::Selector(Selector::Keyword(vec![
                (
                    "first".into(),
                    keyword("nested", Type::Variable(a.variable.clone())),
                ),
                ("second".into(), Type::Variable(b.variable.clone())),
            ])),
            reply: Some(keyword("reply", Type::Variable(a.variable.clone()))),
        };
        let message = |second| {
            Type::Selector(Selector::Keyword(vec![
                ("first".into(), keyword("nested", atom("yes"))),
                ("second".into(), second),
            ]))
        };
        assert_eq!(
            method.instantiate(&message(Type::Int)),
            Some(Some(keyword("reply", atom("yes"))))
        );
        assert_eq!(method.instantiate(&message(atom("no"))), None);
        assert!(method.is_subtype_of(&method.clone()));
        let renamed =
            actor(method.clone()).substitute(&[(TypeVariable::fresh(Type::UNIT), Type::UNIT)]);
        assert!(actor(method.clone()).is_subtype_of(&renamed));
        assert!(method.is_subtype_of(&mono(message(Type::Int), keyword("reply", atom("yes")))));
        let captured = TypeVariable::fresh(Type::UNIT);
        assert_eq!(
            mono(keyword("x", Type::Variable(captured)), Type::UNIT)
                .instantiate(&keyword("x", Type::UNIT)),
            None
        );
    }

    #[test]
    fn selector_covariance_and_disjoint_domains() {
        assert!(keyword("x", Type::Int).is_subtype_of(&keyword("x", Type::UNIT)));
        assert!(!keyword("x", Type::UNIT).is_subtype_of(&keyword("x", Type::Int)));
        assert!(atom("yes").is_disjoint_from(&atom("no")));
        assert!(keyword("x", atom("yes")).is_disjoint_from(&keyword("x", atom("no"))));
        assert!(!keyword("x", Type::UNIT).is_disjoint_from(&keyword("x", Type::UNIT)));
        assert!(!Type::UNIT.is_disjoint_from(&atom("yes")));
        assert!(!Type::UNIT.is_disjoint_from(&actor(identity(Type::UNIT))));
        assert!(Type::Never.is_disjoint_from(&Type::UNIT));
        assert!(keyword("empty", Type::Never).is_disjoint_from(&Type::UNIT));
        let actor = ActorType {
            methods: vec![mono(atom("yes"), Type::UNIT), mono(atom("no"), Type::UNIT)],
        };
        assert!(actor.has_disjoint_inputs());
        let invalid = ActorType {
            methods: vec![identity(Type::UNIT), mono(atom("yes"), Type::UNIT)],
        };
        assert!(!invalid.has_disjoint_inputs());
        let invalid = Type::Actor(invalid);
        assert!(!invalid.is_subtype_of(&invalid));
        assert!(!invalid.is_subtype_of(&Type::UNIT));
    }

    fn two_methods() -> Type {
        Type::Actor(ActorType {
            methods: vec![mono(atom("yes"), Type::UNIT), mono(atom("no"), Type::UNIT)],
        })
    }

    #[test]
    fn actor_plans_record_reordering_width_and_many_to_one() {
        let provided = two_methods();
        let Type::Actor(mut reversed) = provided.clone() else {
            panic!()
        };
        reversed.methods.reverse();
        let plan = provided.adaptation_to(&Type::Actor(reversed)).unwrap();
        assert!(plan.is_identity());
        let AdaptationPlan::Actor { methods, .. } = plan else {
            panic!()
        };
        assert_eq!(
            methods
                .iter()
                .map(|m| (m.expected_index, m.provided_index))
                .collect::<Vec<_>>(),
            vec![(0, 1), (1, 0)]
        );
        assert!(methods.iter().all(|m| m.adaptation.is_identity()));
        assert!(
            provided
                .adaptation_to(&actor(mono(atom("no"), Type::UNIT)))
                .unwrap()
                .is_identity()
        );
        assert!(provided.adaptation_to(&Type::UNIT).unwrap().is_identity());
        assert!(provided.adaptation_to(&provided).unwrap().is_identity());

        let broad = actor(mono(Type::UNIT, Type::UNIT));
        let plan = broad.adaptation_to(&provided).unwrap();
        assert!(plan.is_identity());
        let AdaptationPlan::Actor { methods, .. } = plan else {
            panic!()
        };
        assert_eq!(
            methods.iter().map(|m| m.provided_index).collect::<Vec<_>>(),
            vec![0, 0]
        );
        assert!(provided.adaptation_to(&broad).is_none());
    }

    #[test]
    fn method_plans_have_contravariant_inputs_and_covariant_replies() {
        let wide = two_methods();
        let narrow = actor(mono(atom("yes"), Type::UNIT));
        let provided = mono(narrow.clone(), wide.clone());
        let required = mono(wide.clone(), narrow.clone());
        let plan = provided.adaptation_to(&required).unwrap();
        assert_eq!(plan.input, wide.adaptation_to(&narrow).unwrap());
        assert_eq!(plan.reply, wide.adaptation_to(&narrow));
        assert!(plan.is_identity());
        assert!(required.adaptation_to(&provided).is_none());
        let outer = actor(provided).adaptation_to(&actor(required)).unwrap();
        assert!(outer.is_identity());
    }

    #[test]
    fn payload_and_variable_plans_preserve_nested_adaptations() {
        let wide = two_methods();
        let narrow = actor(mono(atom("yes"), Type::UNIT));
        let nested = keyword("outer", keyword("inner", wide.clone()));
        let required = keyword("outer", keyword("inner", narrow.clone()));
        let plan = nested.adaptation_to(&required).unwrap();
        assert!(plan.is_identity());
        assert!(matches!(plan, AdaptationPlan::Selector(_)));
        let variable = Type::Variable(TypeVariable::fresh(wide));
        let plan = variable.adaptation_to(&narrow).unwrap();
        assert!(matches!(plan, AdaptationPlan::Variable(_)));
        assert!(plan.is_identity());
        assert!(
            keyword("x", Type::UNIT)
                .adaptation_to(&keyword("x", Type::UNIT))
                .unwrap()
                .is_identity()
        );
        assert!(
            nested
                .adaptation_to(&keyword("different", Type::UNIT))
                .is_none()
        );
    }

    #[test]
    fn generic_bounds_retain_structural_adaptation_evidence() {
        let wide = two_methods();
        let narrow = actor(mono(atom("yes"), Type::UNIT));
        let plan = identity(narrow.clone())
            .adaptation_to(&mono(wide.clone(), wide.clone()))
            .unwrap();
        assert_eq!(
            plan.parameter_bounds,
            vec![wide.adaptation_to(&narrow).unwrap()]
        );
        assert!(matches!(
            plan.parameter_bounds[0],
            AdaptationPlan::Actor { .. }
        ));
        assert!(plan.is_identity());
        assert!(
            identity(Type::UNIT)
                .adaptation_to(&identity(Type::UNIT))
                .unwrap()
                .parameter_bounds
                .is_empty()
        );
    }

    #[test]
    fn generic_plans_instantiate_and_keep_required_variables_rigid() {
        assert!(
            identity(Type::UNIT)
                .adaptation_to(&mono(Type::UNIT, Type::UNIT))
                .unwrap()
                .is_identity()
        );
        assert!(
            identity(Type::UNIT)
                .adaptation_to(&identity(Type::UNIT))
                .unwrap()
                .is_identity()
        );
        assert!(
            mono(Type::UNIT, Type::UNIT)
                .adaptation_to(&identity(Type::UNIT))
                .is_none()
        );
        // Inference only descends through selectors, so an alpha-equivalent
        // quantified actor input must retain the representation-identity escape.
        let parameter = TypeParameter::new(Type::UNIT);
        let ty = Type::Variable(parameter.variable.clone());
        let method = MethodType {
            parameters: vec![parameter],
            input: actor(mono(atom("get"), ty.clone())),
            reply: Some(ty),
        };
        let original = actor(method);
        let renamed = original.substitute(&[(TypeVariable::fresh(Type::UNIT), Type::UNIT)]);
        assert_ne!(original, renamed);
        assert!(original.adaptation_to(&renamed).unwrap().is_identity());
    }

    #[test]
    fn plans_preserve_reply_modes_and_reject_invalid_actors() {
        let no_reply = MethodType {
            parameters: vec![],
            input: Type::UNIT,
            reply: None,
        };
        let plan = no_reply
            .adaptation_to(&MethodType {
                input: Type::UNIT,
                ..no_reply.clone()
            })
            .unwrap();
        assert_eq!(plan.reply, None);
        assert!(plan.is_identity());
        for reply in [Type::Never, Type::UNIT, Type::Int] {
            let replying = mono(Type::UNIT, reply);
            assert!(no_reply.adaptation_to(&replying).is_none());
            assert!(replying.adaptation_to(&no_reply).is_none());
        }
        let invalid = Type::Actor(ActorType {
            methods: vec![no_reply.clone(), no_reply],
        });
        assert!(invalid.adaptation_to(&invalid).is_none());
        assert!(invalid.adaptation_to(&Type::UNIT).is_none());
        assert!(
            Type::Never
                .adaptation_to(&two_methods())
                .unwrap()
                .is_identity()
        );
        assert!(
            two_methods()
                .adaptation_to(&Type::UNIT)
                .unwrap()
                .is_identity()
        );
    }

    #[test]
    fn strings_are_a_proper_subtype_of_bytes() {
        assert!(Type::String.is_subtype_of(&Type::Bytes));
        assert!(!Type::Bytes.is_subtype_of(&Type::String));
        assert!(Type::Bytes.is_subtype_of(&Type::UNIT));
        assert!(!Type::String.is_disjoint_from(&Type::Bytes));
        assert!(!Type::Bytes.is_disjoint_from(&Type::String));
        assert!(Type::Bytes.is_disjoint_from(&Type::Int));
        assert!(Type::Bytes.is_disjoint_from(&Type::Float));
        assert!(Type::Bytes.is_disjoint_from(&Type::SelectorFamily));
        assert!(keyword("data", Type::String).is_subtype_of(&keyword("data", Type::Bytes)));
        assert!(!keyword("data", Type::Bytes).is_subtype_of(&keyword("data", Type::String)));
        assert!(Type::Never.is_subtype_of(&Type::Bytes));
        assert_eq!(Type::Bytes.to_string(), "bytes");
    }

    #[test]
    fn primitive_lattice_has_actor_top_and_disjoint_families() {
        let families = [
            Type::Bytes,
            Type::String,
            Type::Int,
            Type::Float,
            Type::SelectorFamily,
            Type::Atom,
            Type::OpTagged,
            Type::KeywordTagged,
        ];
        for family in &families {
            assert!(family.is_subtype_of(&Type::UNIT));
            assert!(Type::Never.is_subtype_of(family));
            assert!(!Type::UNIT.is_subtype_of(family));
            assert!(!family.is_disjoint_from(&Type::UNIT));
            assert!(identity(family.clone()).is_subtype_of(&identity(family.clone())));
        }
        for a in [Type::String, Type::Int, Type::Float, Type::SelectorFamily] {
            for b in [Type::String, Type::Int, Type::Float, Type::SelectorFamily] {
                assert_eq!(a.is_disjoint_from(&b), a != b);
                assert_eq!(a.is_subtype_of(&b), a == b);
            }
        }
        let shapes = [
            atom("abc"),
            Type::Selector(Selector::Operator {
                operator: "+".into(),
                value: Box::new(Type::Int),
            }),
            keyword("a", Type::Int),
        ];
        for (i, family) in [Type::Atom, Type::OpTagged, Type::KeywordTagged]
            .iter()
            .enumerate()
        {
            assert!(family.is_subtype_of(&Type::SelectorFamily));
            for (j, shape) in shapes.iter().enumerate() {
                assert_eq!(shape.is_subtype_of(family), i == j);
                assert_eq!(shape.is_disjoint_from(family), i != j);
                assert!(!family.is_subtype_of(shape));
                assert!(shape.is_subtype_of(&Type::SelectorFamily));
                assert!(shape.is_subtype_of(&Type::UNIT));
            }
        }
        assert!(!Type::Int.is_subtype_of(&two_methods()));
    }

    #[test]
    fn selectors_display_with_context() {
        assert_eq!(keyword("x", atom("yes")).to_string(), "#x: (#yes)");
        assert_eq!(
            actor(mono(keyword("x", Type::UNIT), atom("ok"))).to_string(),
            "{ x: ({}) -> #ok }"
        );
    }
}

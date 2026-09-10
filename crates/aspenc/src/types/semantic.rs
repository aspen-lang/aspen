use crate::Selector;

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
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
    Any,
    Variable(TypeVariable),
}

#[derive(Clone, Debug)]
pub struct TypeVariable {
    id: u64,
    pub upper_bound: Box<Type>,
}

impl PartialEq for TypeVariable {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for TypeVariable {}

impl TypeVariable {
    pub fn fresh(upper_bound: Type) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("type variable identity exhausted");
        Self {
            id,
            upper_bound: Box::new(upper_bound),
        }
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

impl Type {
    pub const UNIT: Self = Self::Actor(ActorType {
        methods: Vec::new(),
    });

    pub fn is_subtype_of(&self, expected: &Self) -> bool {
        if !self.is_well_formed() || !expected.is_well_formed() {
            return false;
        }
        if self == expected || *self == Self::Never || *expected == Self::Any {
            return true;
        }
        match (self, expected) {
            (Self::Selector(actual), Self::Selector(expected)) => selector_pairs(actual, expected)
                .is_some_and(|pairs| pairs.into_iter().all(|(a, b)| a.is_subtype_of(b))),
            (Self::Variable(variable), _) => variable.upper_bound.is_subtype_of(expected),
            (Self::Actor(actual), Self::Actor(expected)) => {
                expected.methods.iter().all(|required| {
                    actual
                        .methods
                        .iter()
                        .any(|provided| provided.is_subtype_of(required))
                })
            }
            _ => false,
        }
    }

    /// Simultaneous substitution; locally bound variables are renamed before descent.
    pub fn substitute(&self, substitutions: &[(TypeVariable, Type)]) -> Self {
        if substitutions.is_empty() {
            return self.clone();
        }
        match self {
            Self::Variable(variable) => substitutions
                .iter()
                .rev()
                .find(|(key, _)| key == variable)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| {
                    Self::Variable(TypeVariable {
                        id: variable.id,
                        upper_bound: Box::new(variable.upper_bound.substitute(substitutions)),
                    })
                }),
            Self::Selector(selector) => {
                Self::Selector(selector.map(|ty| ty.substitute(substitutions)))
            }
            Self::Actor(actor) => Self::Actor(ActorType {
                methods: actor
                    .methods
                    .iter()
                    .map(|method| {
                        let mut scope = substitutions.to_vec();
                        let parameters = method
                            .parameters
                            .iter()
                            .map(|parameter| {
                                let variable = TypeVariable::fresh(
                                    parameter.variable.upper_bound.substitute(&scope),
                                );
                                scope.push((
                                    parameter.variable.clone(),
                                    Self::Variable(variable.clone()),
                                ));
                                TypeParameter { variable }
                            })
                            .collect();
                        MethodType {
                            parameters,
                            input: method.input.substitute(&scope),
                            reply: method.reply.as_ref().map(|ty| ty.substitute(&scope)),
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
        match self {
            Self::Actor(actor) => {
                actor.has_disjoint_inputs()
                    && actor.methods.iter().all(|method| {
                        method
                            .parameters
                            .iter()
                            .all(|p| p.variable.upper_bound.is_well_formed())
                            && method.input.is_well_formed()
                            && method.reply.as_ref().is_none_or(Type::is_well_formed)
                    })
            }
            Self::Selector(selector) => selector.values().iter().all(|ty| ty.is_well_formed()),
            Self::Variable(variable) => variable.upper_bound.is_well_formed(),
            _ => true,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Never => true,
            Self::Variable(variable) => variable.upper_bound.is_empty(),
            Self::Selector(selector) => selector.values().iter().any(|ty| ty.is_empty()),
            _ => false,
        }
    }

    /// A conservative proof that no inhabited value belongs to both types.
    pub fn is_disjoint_from(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return true;
        }
        match (self, other) {
            (Self::Never, _) | (_, Self::Never) => true,
            (Self::Variable(v), _) => v.upper_bound.is_disjoint_from(other),
            (_, Self::Variable(v)) => self.is_disjoint_from(&v.upper_bound),
            (Self::Selector(a), Self::Selector(b)) => selector_pairs(a, b)
                .is_none_or(|pairs| pairs.into_iter().any(|(a, b)| a.is_disjoint_from(b))),
            (Self::Actor(_), Self::Selector(_)) | (Self::Selector(_), Self::Actor(_)) => true,
            _ => false,
        }
    }
}

impl MethodType {
    pub fn accepted_input(&self) -> Type {
        let mut substitutions = Vec::new();
        for parameter in &self.parameters {
            let bound = parameter.variable.upper_bound.substitute(&substitutions);
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
        for parameter in &self.parameters {
            let bound = parameter.variable.upper_bound.substitute(&instances);
            if let Some((_, instance)) = instances.iter().find(|(v, _)| *v == parameter.variable) {
                if !instance.is_subtype_of(&bound) {
                    return None;
                }
            } else {
                instances.push((parameter.variable.clone(), bound));
            }
        }
        message
            .is_subtype_of(&self.input.substitute(&instances))
            .then_some(instances)
    }

    pub fn is_subtype_of(&self, expected: &Self) -> bool {
        if !self.input.is_well_formed()
            || !self.reply.as_ref().is_none_or(Type::is_well_formed)
            || !expected.input.is_well_formed()
            || !expected.reply.as_ref().is_none_or(Type::is_well_formed)
        {
            return false;
        }
        if alpha_method(self, expected, &[]) {
            return true;
        }
        // Required quantifiers remain rigid; only the provided method is instantiated.
        let mut skolems = Vec::new();
        for parameter in &expected.parameters {
            let rigid = TypeVariable::fresh(parameter.variable.upper_bound.substitute(&skolems));
            skolems.push((parameter.variable.clone(), Type::Variable(rigid)));
        }
        let required_input = expected.input.substitute(&skolems);
        let required_reply = expected.reply.as_ref().map(|ty| ty.substitute(&skolems));
        self.instantiate(&required_input)
            .is_some_and(|reply| match (reply, &required_reply) {
                (None, None) => true,
                (Some(actual), Some(expected)) => actual.is_subtype_of(expected),
                _ => false,
            })
    }
}

fn infer_input(
    pattern: &Type,
    message: &Type,
    parameters: &[TypeParameter],
    instances: &mut Vec<(TypeVariable, Type)>,
) -> Option<()> {
    match pattern {
        Type::Variable(variable) if parameters.iter().any(|p| p.variable == *variable) => {
            if let Some((_, previous)) = instances.iter_mut().find(|(v, _)| v == variable) {
                if !message.is_subtype_of(previous) {
                    if previous.is_subtype_of(message) {
                        *previous = message.clone();
                    } else {
                        *previous = (*variable.upper_bound).clone();
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
                    &variable.upper_bound,
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
    match (left, right) {
        (Type::Never, Type::Never) | (Type::Any, Type::Any) => true,
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
    }
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
        if !alpha_type(&a.variable.upper_bound, &b.variable.upper_bound, &scope) {
            return false;
        }
        scope.push((a.variable.clone(), b.variable.clone()));
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
        Type::Any => f.write_str("any"),
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
                    for (index, parameter) in method.parameters.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        let name = variable_name(local.len());
                        write!(f, "{name} <: ")?;
                        display_type(&parameter.variable.upper_bound, f, &mut local)?;
                        local.push(parameter.variable.clone());
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
                for parameter in &method.parameters {
                    free_variables(&parameter.variable.upper_bound, &local, free);
                    local.push(parameter.variable.clone());
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
    fn reply_modes_are_distinct_from_all_value_types() {
        let no_reply = MethodType {
            parameters: vec![],
            input: Type::Any,
            reply: None,
        };
        assert_eq!(no_reply.instantiate(&Type::UNIT), Some(None));
        assert!(no_reply.is_subtype_of(&no_reply));
        assert_eq!(actor(no_reply.clone()).to_string(), "{ (any) }");
        for ty in [Type::Never, Type::UNIT, Type::Any] {
            let reply = mono(Type::Any, ty.clone());
            assert_eq!(reply.instantiate(&Type::UNIT), Some(Some(ty)));
            assert!(!reply.is_subtype_of(&no_reply));
            assert!(!no_reply.is_subtype_of(&reply));
        }
        let generic = MethodType {
            reply: None,
            ..identity(Type::Any)
        };
        let renamed =
            actor(generic.clone()).substitute(&[(TypeVariable::fresh(Type::Any), Type::UNIT)]);
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
        let id = identity(Type::Any);
        assert!(id.is_subtype_of(&mono(Type::UNIT, Type::UNIT)));
        assert!(id.is_subtype_of(&identity(Type::Any)));
        assert!(!mono(Type::Any, Type::Any).is_subtype_of(&id));
        assert!(!mono(Type::UNIT, Type::UNIT).is_subtype_of(&id));
        assert!(mono(Type::Any, Type::Never).is_subtype_of(&id));
        assert_eq!(actor(id).to_string(), "{ <A <: any> (A) -> A }");
    }

    #[test]
    fn bounds_restrict_instantiation_and_do_not_identify_variables() {
        let id = identity(Type::UNIT);
        assert!(id.is_subtype_of(&mono(Type::UNIT, Type::UNIT)));
        assert!(!id.is_subtype_of(&mono(Type::Any, Type::Any)));
        assert!(!id.is_subtype_of(&identity(Type::Any)));
        assert!(identity(Type::Any).is_subtype_of(&id));
        let a = TypeVariable::fresh(Type::UNIT);
        let b = TypeVariable::fresh(Type::UNIT);
        assert_ne!(a, b);
        assert!(Type::Variable(a.clone()).is_subtype_of(&Type::UNIT));
        assert!(!Type::Variable(a).is_subtype_of(&Type::Variable(b)));
    }

    #[test]
    fn nested_captures_substitute_without_capture() {
        let outer = TypeParameter::new(Type::Any);
        let inner = TypeParameter::new(Type::Any);
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
            "{ <A <: any> (A) -> { <B <: any> (B) -> A } }"
        );
        assert!(method.is_subtype_of(&mono(Type::UNIT, actor(mono(Type::Any, Type::UNIT)))));
        assert!(!method.is_subtype_of(&mono(Type::Any, actor(mono(Type::Any, Type::UNIT)))));
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
        let id = identity(Type::Any);
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
        let a = TypeParameter::new(Type::Any);
        let b = TypeParameter::new(Type::UNIT);
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
            method.instantiate(&message(Type::UNIT)),
            Some(Some(keyword("reply", atom("yes"))))
        );
        assert_eq!(method.instantiate(&message(atom("no"))), None);
        assert!(method.is_subtype_of(&method.clone()));
        let renamed =
            actor(method.clone()).substitute(&[(TypeVariable::fresh(Type::Any), Type::UNIT)]);
        assert!(actor(method.clone()).is_subtype_of(&renamed));
        assert!(method.is_subtype_of(&mono(message(Type::UNIT), keyword("reply", atom("yes")))));
        let captured = TypeVariable::fresh(Type::Any);
        assert_eq!(
            mono(keyword("x", Type::Variable(captured)), Type::UNIT)
                .instantiate(&keyword("x", Type::UNIT)),
            None
        );
    }

    #[test]
    fn selector_covariance_and_disjoint_domains() {
        assert!(keyword("x", Type::UNIT).is_subtype_of(&keyword("x", Type::Any)));
        assert!(!keyword("x", Type::Any).is_subtype_of(&keyword("x", Type::UNIT)));
        assert!(atom("yes").is_disjoint_from(&atom("no")));
        assert!(keyword("x", atom("yes")).is_disjoint_from(&keyword("x", atom("no"))));
        assert!(!keyword("x", Type::Any).is_disjoint_from(&keyword("x", Type::UNIT)));
        assert!(Type::UNIT.is_disjoint_from(&atom("yes")));
        assert!(!Type::UNIT.is_disjoint_from(&actor(identity(Type::Any))));
        assert!(Type::Never.is_disjoint_from(&Type::Any));
        assert!(keyword("empty", Type::Never).is_disjoint_from(&Type::Any));
        let actor = ActorType {
            methods: vec![mono(atom("yes"), Type::UNIT), mono(atom("no"), Type::UNIT)],
        };
        assert!(actor.has_disjoint_inputs());
        let invalid = ActorType {
            methods: vec![identity(Type::Any), mono(atom("yes"), Type::UNIT)],
        };
        assert!(!invalid.has_disjoint_inputs());
        let invalid = Type::Actor(invalid);
        assert!(!invalid.is_subtype_of(&invalid));
        assert!(!invalid.is_subtype_of(&Type::Any));
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

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
    pub output: Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Never,
    Actor(ActorType),
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
        if self == expected || *self == Self::Never || *expected == Self::Any {
            return true;
        }
        match (self, expected) {
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
                            output: method.output.substitute(&scope),
                        }
                    })
                    .collect(),
            }),
            _ => self.clone(),
        }
    }
}

impl MethodType {
    pub fn is_subtype_of(&self, expected: &Self) -> bool {
        if alpha_method(self, expected, &[]) {
            return true;
        }
        // This is deliberately input-directed, not a general higher-rank solver.
        if self.parameters.len() > 1 || expected.parameters.len() > 1 {
            return false;
        }
        let mut skolems = Vec::new();
        for parameter in &expected.parameters {
            let rigid = TypeVariable::fresh(parameter.variable.upper_bound.substitute(&skolems));
            skolems.push((parameter.variable.clone(), Type::Variable(rigid)));
        }
        let required_input = expected.input.substitute(&skolems);
        let required_output = expected.output.substitute(&skolems);
        let mut instances = Vec::new();
        if let Some(parameter) = self.parameters.first() {
            if self.input != Type::Variable(parameter.variable.clone()) {
                return false;
            }
            if !required_input.is_subtype_of(&parameter.variable.upper_bound) {
                return false;
            }
            instances.push((parameter.variable.clone(), required_input.clone()));
        }
        required_input.is_subtype_of(&self.input.substitute(&instances))
            && self
                .output
                .substitute(&instances)
                .is_subtype_of(&required_output)
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
    alpha_type(&left.input, &right.input, &scope) && alpha_type(&left.output, &right.output, &scope)
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
                display_type(&method.input, f, &mut local)?;
                f.write_str(" -> ")?;
                display_type(&method.output, f, &mut local)?;
            }
            f.write_str(" }")
        }
    }
}

fn free_variables(ty: &Type, bound: &[TypeVariable], free: &mut Vec<TypeVariable>) {
    match ty {
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
                free_variables(&method.output, &local, free);
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
    fn mono(input: Type, output: Type) -> MethodType {
        MethodType {
            parameters: vec![],
            input,
            output,
        }
    }
    fn identity(bound: Type) -> MethodType {
        let parameter = TypeParameter::new(bound);
        let ty = Type::Variable(parameter.variable.clone());
        MethodType {
            parameters: vec![parameter],
            input: ty.clone(),
            output: ty,
        }
    }

    #[test]
    fn universal_identity_and_rigid_results() {
        let id = identity(Type::Any);
        assert!(id.is_subtype_of(&mono(Type::UNIT, Type::UNIT)));
        assert!(id.is_subtype_of(&identity(Type::Any)));
        assert!(!mono(Type::Any, Type::Any).is_subtype_of(&id));
        assert!(!mono(Type::UNIT, Type::UNIT).is_subtype_of(&id));
        assert!(mono(Type::Any, Type::Never).is_subtype_of(&id));
        assert_eq!(actor(id).to_string(), "{ <A <: any> A -> A }");
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
            output: outer_ty.clone(),
        });
        let method = MethodType {
            parameters: vec![outer.clone()],
            input: outer_ty,
            output: nested.clone(),
        };
        assert_eq!(
            actor(method.clone()).to_string(),
            "{ <A <: any> A -> { <B <: any> B -> A } }"
        );
        assert!(method.is_subtype_of(&mono(Type::UNIT, actor(mono(Type::Any, Type::UNIT)))));
        assert!(!method.is_subtype_of(&mono(Type::Any, actor(mono(Type::Any, Type::UNIT)))));
        let replaced = nested.substitute(&[(outer.variable, inner_ty.clone())]);
        let Type::Actor(replaced) = replaced else {
            panic!()
        };
        assert_eq!(replaced.methods[0].output, inner_ty);
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
}

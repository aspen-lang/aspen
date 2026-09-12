//! Runtime-observable input shapes, independent of actor interfaces.
//!
//! These shapes describe dispatch domains, not subtype checks. Actor references
//! are opaque handles: lowering intentionally forgets every method and reply.

use crate::{
    Selector,
    types::{ActorType, MethodType, Type},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeShape {
    /// No runtime value, including selectors with an empty component.
    Empty,
    /// Any runtime value.
    Any,
    /// Any opaque actor handle, regardless of its interface.
    Actor,
    Bytes,
    String,
    Int,
    Float,
    SelectorFamily,
    Atom,
    OpTagged,
    KeywordTagged,
    Selector(Selector<RuntimeShape>),
}

impl RuntimeShape {
    /// Erase static information while retaining recursive selector tags.
    pub fn from_type(ty: &Type) -> Self {
        Self::from_type_in(ty, &mut Vec::new())
    }

    fn from_type_in(ty: &Type, active: &mut Vec<crate::types::TypeVariable>) -> Self {
        match ty {
            Type::Never => Self::Empty,
            Type::Actor(actor) if actor.methods.is_empty() => Self::Any,
            Type::Actor(_) => Self::Actor,
            Type::Bytes => Self::Bytes,
            Type::String => Self::String,
            Type::Int => Self::Int,
            Type::Float => Self::Float,
            Type::SelectorFamily => Self::SelectorFamily,
            Type::Atom => Self::Atom,
            Type::OpTagged => Self::OpTagged,
            Type::KeywordTagged => Self::KeywordTagged,
            Type::Variable(variable) | Type::Alias(variable) => {
                // Selector payloads are finite values. A constructor cycle
                // without an intervening actor handle has no runtime inhabitant.
                if active.contains(variable) {
                    return Self::Empty;
                }
                active.push(variable.clone());
                let shape = Self::from_type_in(&variable.upper_bound(), active);
                active.pop();
                shape
            }
            Type::Selector(selector) => {
                let selector = selector.map(|child| Self::from_type_in(child, active));
                if selector.values().iter().any(|shape| shape.is_empty()) {
                    Self::Empty
                } else {
                    Self::Selector(selector)
                }
            }
        }
    }

    /// Quantified receiver inputs accept their bounds, not rigid variables.
    pub fn accepted_input(method: &MethodType) -> Self {
        Self::from_type(&method.accepted_input())
    }

    /// Also handles non-normalized shapes constructed through the public enum.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Selector(selector) => selector.values().iter().any(|shape| shape.is_empty()),
            _ => false,
        }
    }

    // Each bit denotes one disjoint runtime value family.
    fn kind_mask(&self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Any => 0xff,
            Self::Actor => 1,
            Self::Bytes => 2 | 0x80,
            Self::String => 2,
            Self::Int => 4,
            Self::Float => 8,
            Self::SelectorFamily => 0x70,
            Self::Atom | Self::Selector(Selector::Atomic(_)) => 0x10,
            Self::OpTagged | Self::Selector(Selector::Operator { .. }) => 0x20,
            Self::KeywordTagged | Self::Selector(Selector::Keyword(_)) => 0x40,
        }
    }

    /// Prove domains disjoint using only value kind and recursive selector tags.
    pub fn is_disjoint_from(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return true;
        }
        match (self, other) {
            (Self::Selector(a), Self::Selector(b)) => {
                !a.same_shape(b)
                    || a.values()
                        .into_iter()
                        .zip(b.values())
                        .any(|(a, b)| a.is_disjoint_from(b))
            }
            _ => self.kind_mask() & other.kind_mask() == 0,
        }
    }
}

impl From<&Type> for RuntimeShape {
    fn from(ty: &Type) -> Self {
        Self::from_type(ty)
    }
}

/// Zero-based method indices whose lowered domains overlap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchOverlap {
    pub first: usize,
    pub second: usize,
}

impl std::fmt::Display for DispatchOverlap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "runtime input shapes overlap for methods {} and {}",
            self.first, self.second
        )
    }
}

impl std::error::Error for DispatchOverlap {}

/// Lower and validate a receiver table without inspecting actor interfaces.
///
/// This is not semantic well-formedness validation: callers must perform that
/// separately. In particular, nested actor method sets do not affect dispatch.
/// Empty receivers remain in the table so indices still refer to source methods.
pub fn validate_actor_inputs(actor: &ActorType) -> Result<Vec<RuntimeShape>, DispatchOverlap> {
    let shapes: Vec<_> = actor
        .methods
        .iter()
        .map(RuntimeShape::accepted_input)
        .collect();
    for second in 0..shapes.len() {
        for first in 0..second {
            if !shapes[first].is_disjoint_from(&shapes[second]) {
                return Err(DispatchOverlap { first, second });
            }
        }
    }
    Ok(shapes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TypeParameter, TypeVariable};

    fn atom(name: &str) -> Type {
        Type::Selector(Selector::Atomic(name.into()))
    }

    fn tag(name: &str, value: Type) -> Type {
        Type::Selector(Selector::Keyword(vec![(name.into(), value)]))
    }

    fn method(input: Type) -> MethodType {
        MethodType {
            parameters: vec![],
            input,
            reply: None,
        }
    }

    #[test]
    fn nested_tags_and_ordered_labels_are_preserved() {
        let a = tag("outer", tag("inner", atom("yes")));
        let b = tag("outer", tag("inner", atom("no")));
        assert!(RuntimeShape::from_type(&a).is_disjoint_from(&RuntimeShape::from_type(&b)));
        assert_eq!(
            RuntimeShape::from_type(&a),
            RuntimeShape::Selector(Selector::Keyword(vec![(
                "outer".into(),
                RuntimeShape::Selector(Selector::Keyword(vec![(
                    "inner".into(),
                    RuntimeShape::Selector(Selector::Atomic("yes".into()))
                )]))
            )]))
        );
        let a = Type::Selector(Selector::Keyword(vec![
            ("a".into(), Type::UNIT),
            ("b".into(), Type::UNIT),
        ]));
        let b = Type::Selector(Selector::Keyword(vec![
            ("b".into(), Type::UNIT),
            ("a".into(), Type::UNIT),
        ]));
        assert!(RuntimeShape::from_type(&a).is_disjoint_from(&RuntimeShape::from_type(&b)));
        let operator = Type::Selector(Selector::Operator {
            operator: "a".into(),
            value: Box::new(Type::UNIT),
        });
        assert!(
            RuntimeShape::from_type(&operator)
                .is_disjoint_from(&RuntimeShape::from_type(&tag("a", Type::UNIT)))
        );
    }

    #[test]
    fn top_is_a_wildcard_and_selector_families_overlap_their_members() {
        assert_eq!(RuntimeShape::from_type(&Type::UNIT), RuntimeShape::Any);
        let atomic = RuntimeShape::from_type(&atom("ok"));
        assert!(!RuntimeShape::Any.is_disjoint_from(&atomic));
        assert!(!RuntimeShape::SelectorFamily.is_disjoint_from(&atomic));
        assert!(!RuntimeShape::Atom.is_disjoint_from(&atomic));
        assert!(RuntimeShape::KeywordTagged.is_disjoint_from(&atomic));
        assert!(RuntimeShape::Int.is_disjoint_from(&RuntimeShape::Float));
        assert!(RuntimeShape::Actor.is_disjoint_from(&RuntimeShape::String));
        assert_eq!(RuntimeShape::from_type(&Type::Bytes), RuntimeShape::Bytes);
        assert!(!RuntimeShape::Bytes.is_disjoint_from(&RuntimeShape::String));
        assert!(!RuntimeShape::String.is_disjoint_from(&RuntimeShape::Bytes));
        assert!(RuntimeShape::Bytes.is_disjoint_from(&RuntimeShape::Int));
        assert!(
            validate_actor_inputs(&ActorType {
                methods: vec![method(Type::Bytes), method(Type::String)],
            })
            .is_err()
        );
    }

    #[test]
    fn actor_handles_do_not_expose_interfaces() {
        let left = Type::Actor(ActorType {
            methods: vec![method(atom("left"))],
        });
        let right = Type::Actor(ActorType {
            methods: vec![method(atom("right"))],
        });
        assert_eq!(RuntimeShape::from_type(&left), RuntimeShape::Actor);
        assert_eq!(RuntimeShape::from_type(&right), RuntimeShape::Actor);
        assert!(!RuntimeShape::Actor.is_disjoint_from(&RuntimeShape::Actor));
        assert!(RuntimeShape::Actor.is_disjoint_from(&RuntimeShape::from_type(&atom("left"))));
        assert!(!RuntimeShape::Any.is_disjoint_from(&RuntimeShape::Actor));
        assert_eq!(
            validate_actor_inputs(&ActorType {
                methods: vec![method(left), method(right)]
            }),
            Err(DispatchOverlap {
                first: 0,
                second: 1
            })
        );
    }

    #[test]
    fn bottom_propagates_through_nested_selectors_and_bounds() {
        let empty = tag(
            "outer",
            tag("inner", Type::Variable(TypeVariable::fresh(Type::Never))),
        );
        assert_eq!(RuntimeShape::from_type(&empty), RuntimeShape::Empty);
        let raw =
            RuntimeShape::Selector(Selector::Keyword(vec![("x".into(), RuntimeShape::Empty)]));
        assert!(raw.is_empty());
        assert!(raw.is_disjoint_from(&RuntimeShape::Any));
        let shapes = validate_actor_inputs(&ActorType {
            methods: vec![method(Type::UNIT), method(empty)],
        })
        .unwrap();
        assert_eq!(shapes, [RuntimeShape::Any, RuntimeShape::Empty]);
    }

    #[test]
    fn quantified_and_dependent_bounds_lower_to_accepted_inputs() {
        let a = TypeParameter::new(tag("x", Type::UNIT));
        let b = TypeParameter::new(Type::Variable(a.variable.clone()));
        let input = Type::Variable(b.variable.clone());
        let receiver = MethodType {
            parameters: vec![a, b],
            input,
            reply: Some(Type::UNIT),
        };
        assert_eq!(
            RuntimeShape::accepted_input(&receiver),
            RuntimeShape::from_type(&tag("x", Type::UNIT))
        );
        assert!(
            validate_actor_inputs(&ActorType {
                methods: vec![receiver, method(tag("y", Type::UNIT))]
            })
            .is_ok()
        );
        assert_eq!(
            validate_actor_inputs(&ActorType {
                methods: vec![method(atom("a")), method(atom("b")), method(Type::UNIT)]
            }),
            Err(DispatchOverlap {
                first: 0,
                second: 2
            })
        );
    }

    // Exhaustive finite grammar: two levels of unary selectors/variables plus
    // every ordered binary product over the primitive domains. No random seed.
    fn generated_types() -> Vec<Type> {
        let base = vec![
            Type::Never,
            Type::UNIT,
            Type::Bytes,
            Type::String,
            Type::Int,
            Type::Float,
            Type::SelectorFamily,
            Type::Atom,
            Type::OpTagged,
            Type::KeywordTagged,
            atom("a"),
            atom("b"),
            Type::Actor(ActorType {
                methods: vec![method(atom("a"))],
            }),
        ];
        let mut types = base.clone();
        let mut frontier = base.clone();
        for _ in 0..2 {
            let mut next = Vec::new();
            for ty in frontier {
                next.push(tag("a", ty.clone()));
                next.push(tag("b", ty.clone()));
                next.push(Type::Selector(Selector::Operator {
                    operator: "+".into(),
                    value: Box::new(ty.clone()),
                }));
                next.push(Type::Variable(TypeVariable::fresh(ty)));
            }
            types.extend(next.iter().cloned());
            frontier = next;
        }
        for a in &base {
            for b in &base {
                for labels in [["a", "b"], ["b", "a"], ["a", "a"]] {
                    types.push(Type::Selector(Selector::Keyword(vec![
                        (labels[0].into(), a.clone()),
                        (labels[1].into(), b.clone()),
                    ])));
                }
            }
        }
        types
    }

    #[test]
    fn static_disjointness_implies_shape_disjointness_exhaustively() {
        let types = generated_types();
        assert_eq!(types.len(), 780);
        let shapes: Vec<_> = types.iter().map(RuntimeShape::from_type).collect();
        for (i, a) in types.iter().enumerate() {
            for (j, b) in types.iter().enumerate() {
                if a.is_disjoint_from(b) {
                    assert!(shapes[i].is_disjoint_from(&shapes[j]), "{a:?} and {b:?}");
                }
                assert_eq!(
                    shapes[i].is_disjoint_from(&shapes[j]),
                    shapes[j].is_disjoint_from(&shapes[i])
                );
            }
        }
    }
}

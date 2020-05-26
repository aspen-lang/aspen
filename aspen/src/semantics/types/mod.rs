use crate::syntax::ObjectDeclaration;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;

mod behaviour;
mod trace;

pub use self::behaviour::*;
pub use self::trace::*;

#[derive(Clone, Debug)]
pub enum Type {
    Failed { diagnosed: bool },
    Object(Arc<ObjectDeclaration>),
    Unbounded(String, usize),
    Integer(Option<i128>),
    Float(Option<f64>),
    Atom(Option<String>),
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Type::*;
        match self {
            Failed { .. } => write!(f, "?"),
            Object(o) => write!(f, "{}", o.symbol()),
            Unbounded(s, _) => write!(f, "{}", s),
            Integer(Some(i)) => write!(f, "Integer ({})", i),
            Integer(None) => write!(f, "Integer"),
            Float(Some(v)) => write!(f, "Float ({})", v),
            Float(None) => write!(f, "Float"),
            Atom(Some(a)) => write!(f, "{}", a),
            Atom(None) => write!(f, "Atom"),
        }
    }
}

impl Type {
    fn check_equality(&self, other: &Type) -> TypeCheck {
        use Type::*;
        match (self, other) {
            (Failed { .. }, _) | (_, Failed { .. }) => Ok(()),
            (Unbounded(_, a), Unbounded(_, b)) if a == b => Ok(()),
            (Unbounded(_, _), _) | (_, Unbounded(_, _)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
            (Object(a), Object(b)) => {
                if Arc::ptr_eq(a, b) {
                    Ok(())
                } else {
                    Err(TypeError::ObjectsAreNotEqual(a.clone(), b.clone()))
                }
            }
            (Integer(i), Integer(j)) => {
                if i == j {
                    Ok(())
                } else {
                    Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
                }
            }
            (Integer(_), _) | (_, Integer(_)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
            (Float(i), Float(j)) => {
                if i == j {
                    Ok(())
                } else {
                    Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
                }
            }
            (Float(_), _) | (_, Float(_)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
            (Atom(a), Atom(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
                }
            }
            (Atom(_), _) | (_, Atom(_)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
        }
    }

    fn check_assignability(&self, other: &Type) -> TypeCheck {
        use Type::*;
        match (self, other) {
            (Failed { .. }, _) | (_, Failed { .. }) => Ok(()),
            (Unbounded(_, _), Unbounded(_, _)) => Ok(()),
            (Object(_), Object(_)) => self.check_equality(other),
            (Unbounded(_, _), Object(_)) => Ok(()),
            (Object(object), Unbounded(_, _)) => Err(TypeError::ObjectsHaveNoSubTypes(
                object.clone(),
                other.clone(),
            )),
            (Integer(None), Integer(Some(_))) => Ok(()),
            (Integer(i), Integer(j)) => {
                if i == j {
                    Ok(())
                } else {
                    Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
                }
            }
            (Integer(_), _) | (_, Integer(_)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
            (Float(None), Float(Some(_))) => Ok(()),
            (Float(i), Float(j)) => {
                if i == j {
                    Ok(())
                } else {
                    Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
                }
            }
            (Float(_), _) | (_, Float(_)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
            (Atom(None), Atom(Some(_))) => Ok(()),
            (Atom(a), Atom(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
                }
            }
            (Atom(_), _) | (_, Atom(_)) => {
                Err(TypeError::TypesAreNotEqual(self.clone(), other.clone()))
            }
        }
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        self.check_equality(other).is_ok()
    }
}

impl PartialOrd for Type {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else if self.check_assignability(other).is_ok() {
            Some(Ordering::Greater)
        } else if other.check_assignability(self).is_ok() {
            Some(Ordering::Less)
        } else {
            None
        }
    }
}

enum Variance {
    Invariant,
    Covariant,
    Contravariant,
}

pub struct TypeSlot {
    variance: Variance,
    apparent: Mutex<Option<Type>>,
    required: Mutex<Option<Type>>,
}

impl TypeSlot {
    pub fn invariant() -> Arc<TypeSlot> {
        Self::new_with_variance(Variance::Invariant)
    }

    pub fn covariant() -> Arc<TypeSlot> {
        Self::new_with_variance(Variance::Covariant)
    }

    pub fn contravariant() -> Arc<TypeSlot> {
        Self::new_with_variance(Variance::Contravariant)
    }

    fn new_with_variance(variance: Variance) -> Arc<TypeSlot> {
        Arc::new(TypeSlot {
            variance,
            apparent: Mutex::new(None),
            required: Mutex::new(None),
        })
    }

    pub async fn resolve_apparent(&self, apparent: Type) {
        let mut opt = self.apparent.lock().await;
        *opt = Some(apparent);
    }

    pub async fn resolve_required(&self, required: Type) {
        let mut opt = self.required.lock().await;
        *opt = Some(required);
    }

    pub async fn get_apparent(&self) -> Option<Type> {
        self.apparent.lock().await.clone()
    }

    pub async fn get_required(&self) -> Option<Type> {
        self.required.lock().await.clone()
    }

    pub async fn wait_for_apparent(&self) -> Type {
        loop {
            if let Some(t) = self.get_apparent().await {
                return t;
            }
        }
    }

    pub async fn wait_for_required(&self) -> Type {
        loop {
            if let Some(t) = self.get_required().await {
                return t.clone();
            }
        }
    }

    pub async fn check(&self) -> TypeCheck {
        let apparent = self.wait_for_apparent().await;
        let required = self.wait_for_required().await;

        match self.variance {
            Variance::Invariant => required.check_equality(&apparent)?,
            Variance::Covariant => required.check_assignability(&apparent)?,
            Variance::Contravariant => apparent.check_assignability(&required)?,
        }

        Ok(())
    }
}

pub type TypeCheck<T = ()> = Result<T, TypeError>;

#[derive(Debug)]
pub enum TypeError {
    ObjectsAreNotEqual(Arc<ObjectDeclaration>, Arc<ObjectDeclaration>),
    TypesAreNotEqual(Type, Type),
    ObjectsHaveNoSubTypes(Arc<ObjectDeclaration>, Type),
}

#[cfg(test)]
mod tests {
    use tokio::task;

    use crate::syntax::{Declaration, Parser, Root};
    use crate::Source;

    use super::*;

    #[tokio::test]
    async fn type_slot() {
        let slot = TypeSlot::invariant();
        let assert_slot = slot.clone();
        let assertion = task::spawn(async move {
            assert!(assert_slot.check().await.is_ok());
        });

        let apparent_slot = slot.clone();
        task::spawn(async move {
            apparent_slot
                .resolve_apparent(Type::Failed { diagnosed: false })
                .await;
        });

        let required_slot = slot.clone();
        task::spawn(async move {
            required_slot
                .resolve_required(Type::Failed { diagnosed: false })
                .await;
        });

        assertion.await.unwrap();
    }

    async fn object(name: &str) -> Arc<ObjectDeclaration> {
        let source = Source::new("test:object", format!("object {}.", name));
        let (root, _) = Parser::new(source).parse().await;
        if let Root::Module(module) = root.as_ref() {
            if let Declaration::Object(object) = module.declarations[0].as_ref() {
                return object.clone();
            }
        }
        panic!("module parsed incorrectly");
    }

    #[tokio::test]
    async fn equal_objects() {
        let object = object("X").await;
        let slot = TypeSlot::invariant();
        slot.resolve_required(Type::Object(object.clone())).await;
        slot.resolve_apparent(Type::Object(object.clone())).await;
        slot.check().await.unwrap();
    }

    #[tokio::test]
    async fn unequal_objects() {
        let x = object("X").await;
        let y = object("Y").await;
        let slot = TypeSlot::invariant();
        slot.resolve_required(Type::Object(x.clone())).await;
        slot.resolve_apparent(Type::Object(y.clone())).await;
        assert!(slot.check().await.is_err());
    }

    #[tokio::test]
    async fn unequal_unbounded() {
        let slot = TypeSlot::invariant();

        slot.resolve_required(Type::Unbounded("a".into(), 1)).await;
        slot.resolve_apparent(Type::Unbounded("b".into(), 2)).await;

        assert!(slot.check().await.is_err());
    }

    #[tokio::test]
    async fn equal_unbounded() {
        let slot = TypeSlot::invariant();

        slot.resolve_required(Type::Unbounded("a".into(), 1)).await;
        slot.resolve_apparent(Type::Unbounded("b".into(), 1)).await;

        assert!(slot.check().await.is_ok());
    }

    #[tokio::test]
    async fn object_assignable_to_unbounded() {
        let x = object("X").await;

        let slot = TypeSlot::covariant();

        slot.resolve_required(Type::Unbounded("a".into(), 1)).await;
        slot.resolve_apparent(Type::Object(x)).await;

        assert!(slot.check().await.is_ok());
    }

    #[tokio::test]
    async fn unbounded_not_assignable_to_object() {
        let x = object("X").await;

        let slot = TypeSlot::covariant();

        slot.resolve_required(Type::Object(x)).await;
        slot.resolve_apparent(Type::Unbounded("a".into(), 1)).await;

        assert!(slot.check().await.is_err());
    }
}

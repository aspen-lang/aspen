use std::sync::Arc;

use tokio::sync::Mutex;

use crate::syntax::{ObjectDeclaration, ClassDeclaration};

#[derive(Clone, Debug)]
pub enum Type {
    Failed,
    Object(Arc<ObjectDeclaration>),
    Unbounded(String, usize),
}

impl Type {
    fn check_equality(&self, other: &Type) -> TypeCheck {
        use Type::*;
        match (self, other) {
            (Failed, _) | (_, Failed) => Ok(()),
            (Unbounded(_, a), Unbounded(_, b)) if a == b => Ok(()),
            (Unbounded(_, _), _) | (_, Unbounded(_, _)) => Err(TypeError::TypesAreNotEqual(self.clone(), other.clone())),
            (Object(a), Object(b)) => {
                if Arc::ptr_eq(a, b) {
                    Ok(())
                } else {
                    Err(TypeError::ObjectsAreNotEqual(a.clone(), b.clone()))
                }
            }
        }
    }

    fn check_assignability(&self, other: &Type) -> TypeCheck {
        use Type::*;
        match (self, other) {
            (Failed, _) | (_, Failed) => Ok(()),
            (Unbounded(_, _), Unbounded(_, _)) => Ok(()),
            (Object(_), Object(_)) => self.check_equality(other),
            (Unbounded(_, _), Object(_)) => Ok(()),
            (Object(object), Unbounded(_, _)) => Err(TypeError::ObjectsHaveNoSubTypes(object.clone(), other.clone())),
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

    pub async fn check(&self) -> TypeCheck {
        let apparent;
        let required;
        loop {
            if let Some(a) = self.apparent.lock().await.as_ref() {
                apparent = a.clone();
                break;
            } else {
                continue;
            }
        }
        loop {
            if let Some(r) = self.required.lock().await.as_ref() {
                required = r.clone();
                break;
            } else {
                continue;
            }
        }

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
    BoundsAreNotTheSame(Vec<Arc<ClassDeclaration>>, Vec<Arc<ClassDeclaration>>),
}

#[cfg(test)]
mod tests {
    use tokio::task;

    use crate::Source;
    use crate::syntax::{Declaration, Parser, Root};

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
            apparent_slot.resolve_apparent(Type::Failed).await;
        });

        let required_slot = slot.clone();
        task::spawn(async move {
            required_slot.resolve_required(Type::Failed).await;
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

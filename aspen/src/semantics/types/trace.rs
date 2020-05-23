use crate::semantics::types::{Type, TypeSlot};
use crate::semantics::Module;
use crate::syntax::{
    Declaration, Expression, ReferenceExpression, ReferenceTypeExpression, TypeExpression,
};
use std::sync::Arc;

pub struct TypeTracer {
    module: Arc<Module>,
    slot: Arc<TypeSlot>,
}

impl TypeTracer {
    pub fn new(module: Arc<Module>, slot: Arc<TypeSlot>) -> TypeTracer {
        TypeTracer { module, slot }
    }

    pub async fn trace_apparent_expression(&self, expression: &Arc<Expression>) -> Type {
        if let Some(t) = self.slot.get_apparent().await {
            return t;
        }

        let t = match expression.as_ref() {
            Expression::Reference(reference) => self.trace_reference(reference),
        }
        .await;

        self.slot.resolve_apparent(t.clone()).await;
        t
    }

    pub async fn trace_apparent_type_expression(&self, expression: &Arc<TypeExpression>) -> Type {
        if let Some(t) = self.slot.get_apparent().await {
            return t;
        }

        let t = match expression.as_ref() {
            TypeExpression::Reference(reference) => self.trace_type_reference(reference),
        }
        .await;

        self.slot.resolve_apparent(t.clone()).await;
        t
    }

    pub async fn trace_reference(&self, reference: &Arc<ReferenceExpression>) -> Type {
        match self
            .module
            .declaration_referenced_by(reference.clone())
            .await
        {
            None => Type::Failed { diagnosed: true },
            Some(declaration) => match declaration.as_ref() {
                Declaration::Object(o) => Type::Object(o.clone()),
            },
        }
    }

    pub async fn trace_type_reference(&self, reference: &Arc<ReferenceTypeExpression>) -> Type {
        match self
            .module
            .declaration_referenced_by_type(reference.clone())
            .await
        {
            None => Type::Failed { diagnosed: true },
            Some(declaration) => match declaration.as_ref() {
                Declaration::Object(o) => Type::Object(o.clone()),
            },
        }
    }
}

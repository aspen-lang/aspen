use crate::semantics::types::{Type, TypeSlot};
use crate::semantics::Module;
use crate::syntax::{
    Declaration, Expression, MessageSend, ReferenceExpression, ReferenceTypeExpression, TokenKind,
    TypeExpression,
};
use futures::future::join;
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
            Expression::Reference(reference) => self.trace_reference(reference).await,
            Expression::Integer(i) => match i.literal.kind {
                TokenKind::IntegerLiteral(i, true) => Type::Integer(Some(i)),
                _ => Type::Failed { diagnosed: true },
            },
            Expression::Float(f) => match f.literal.kind {
                TokenKind::FloatLiteral(f, true) => Type::Float(Some(f)),
                _ => Type::Failed { diagnosed: true },
            },
            Expression::NullaryAtom(a) => Type::Atom(Some(a.atom.lexeme().into())),
            Expression::MessageSend(m) => self.trace_message_send(m).await,
        };

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

    pub async fn trace_message_send<'a>(&'a self, send: &'a Arc<MessageSend>) -> Type {
        match join(
            self.module.get_type_of(send.receiver.clone()),
            self.module.get_type_of(send.message.clone()),
        )
        .await
        {
            (Type::Failed { .. }, _) | (_, Type::Failed { .. }) => Type::Failed { diagnosed: true },

            (Type::Integer(Some(a)), Type::Integer(Some(b))) => Type::Integer(Some(a * b)),
            (Type::Integer(Some(a)), Type::Atom(Some(s))) if s == "increment!" => {
                Type::Integer(Some(a + 1))
            }

            (receiver, message) => {
                for behaviour in self.module.get_behaviours_of_type(receiver).await {
                    if message <= behaviour.selector {
                        return behaviour.reply.clone();
                    }
                }

                Type::Failed { diagnosed: true }
            }
        }
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

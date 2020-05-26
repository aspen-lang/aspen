use crate::semantics::types::{Behaviour, Type};
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Method, ObjectDeclaration};
use futures::future::join_all;
use std::sync::Arc;

#[derive(Clone)]
pub struct GetBehavioursOfObject;

#[async_trait]
impl Analyzer for GetBehavioursOfObject {
    type Input = Arc<ObjectDeclaration>;
    type Output = Vec<Behaviour>;

    async fn analyze(&self, ctx: AnalysisContext<Self::Input>) -> Self::Output {
        join_all(ctx.input.methods().map(|method| {
            let module = ctx.module.clone();
            async move {
                let Method { pattern, .. } = method.as_ref();
                Behaviour {
                    selector: module.get_type_of_pattern(pattern.clone()).await,
                    reply: Type::Failed { diagnosed: true },
                }
            }
        }))
        .await
    }
}

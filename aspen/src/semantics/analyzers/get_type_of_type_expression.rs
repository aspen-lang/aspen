use crate::semantics::types::{Type, TypeSlot, TypeTracer};
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::TypeExpression;
use std::sync::Arc;

#[derive(Clone)]
pub struct GetTypeOfTypeExpression;

#[async_trait]
impl Analyzer for GetTypeOfTypeExpression {
    type Input = Arc<TypeExpression>;
    type Output = Type;

    async fn analyze(&self, ctx: AnalysisContext<Self::Input>) -> Self::Output {
        let slot = TypeSlot::covariant();
        let tracer = TypeTracer::new(ctx.module, slot.clone());
        tracer.trace_apparent_type_expression(&ctx.input).await
    }
}

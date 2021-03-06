use crate::semantics::types::{Type, TypeSlot, TypeTracer};
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::Expression;
use std::sync::Arc;

#[derive(Clone)]
pub struct GetTypeOfExpression;

#[async_trait]
impl Analyzer for GetTypeOfExpression {
    type Input = Arc<Expression>;
    type Output = Type;

    async fn analyze(&self, ctx: AnalysisContext<Self::Input>) -> Self::Output {
        let slot = TypeSlot::covariant();
        let tracer = TypeTracer::new(ctx.module, slot.clone());
        tracer.trace_apparent_expression(&ctx.input).await
    }
}

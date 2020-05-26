use crate::semantics::types::Behaviour;
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::ObjectDeclaration;
use std::sync::Arc;

#[derive(Clone)]
pub struct GetBehavioursOfObject;

#[async_trait]
impl Analyzer for GetBehavioursOfObject {
    type Input = Arc<ObjectDeclaration>;
    type Output = Vec<Behaviour>;

    async fn analyze(&self, _ctx: AnalysisContext<Self::Input>) -> Self::Output {
        vec![]
    }
}

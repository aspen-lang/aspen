use crate::semantics::{AnalysisContext, Analyzer};
use crate::{Diagnostics, UndefinedReference};
use std::convert::identity;

pub struct CheckAllReferencesAreDefined;

#[async_trait]
impl Analyzer for CheckAllReferencesAreDefined {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(&self, ctx: AnalysisContext<()>) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let module = &ctx.module.clone();
        for diagnostic in
            futures::future::join_all(ctx.navigator.traverse().map(async move |child| {
                if let Some(reference) = child.node.clone().as_reference_expression() {
                    if let None = module.declaration_referenced_by(reference.clone()).await {
                        return Some(UndefinedReference(reference));
                    }
                }
                return None;
            }))
            .await
            .into_iter()
            .filter_map(identity)
        {
            diagnostics.push(diagnostic);
        }
        diagnostics
    }
}

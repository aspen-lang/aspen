use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::Declaration;
use std::sync::Arc;

#[derive(Clone)]
pub struct GetExportedDeclarations;

#[async_trait]
impl Analyzer for GetExportedDeclarations {
    type Input = ();
    type Output = Vec<(String, Arc<Declaration>)>;

    async fn analyze(&self, ctx: AnalysisContext<()>) -> Self::Output {
        let mut exported_declarations = vec![];
        if let Some(module) = ctx.navigator.down_to_cast(|n| n.as_module()) {
            for declaration in module.declarations.iter() {
                exported_declarations.push((declaration.symbol().to_string(), declaration.clone()));
            }
        }
        exported_declarations
    }
}

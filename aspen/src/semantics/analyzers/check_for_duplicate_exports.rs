use crate::semantics::{AnalysisContext, Analyzer};
use crate::{Diagnostics, DuplicateExport};
use std::collections::HashSet;

pub struct CheckForDuplicateExports;

#[async_trait]
impl Analyzer for &CheckForDuplicateExports {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(self, ctx: AnalysisContext<()>) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let mut names = HashSet::new();
        for (name, node) in ctx.current_module().await.exported_declarations().await {
            if names.contains(&name) {
                diagnostics.push(DuplicateExport(name, node));
            } else {
                names.insert(name);
            }
        }
        diagnostics
    }
}

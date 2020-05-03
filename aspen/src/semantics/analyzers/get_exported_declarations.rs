use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Node, NodeKind};
use std::sync::Arc;

pub struct GetExportedDeclarations;

#[async_trait]
impl Analyzer for &GetExportedDeclarations {
    type Input = ();
    type Output = Vec<(String, Arc<Node>)>;

    async fn analyze(self, ctx: AnalysisContext<()>) -> Self::Output {
        let mut exported_declarations = vec![];
        for declaration in ctx.navigator.children() {
            match &declaration.node.kind {
                NodeKind::ObjectDeclaration { symbol, .. }
                | NodeKind::ClassDeclaration { symbol, .. } => {
                    if let Some(symbol) = symbol.as_ref().map(Arc::as_ref).and_then(Node::symbol) {
                        exported_declarations.push((symbol, declaration.node.clone()));
                    }
                }
                _ => {}
            }
        }
        exported_declarations
    }
}

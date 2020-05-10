use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Node, ReferenceExpression, ReferenceTypeExpression};
use crate::{Diagnostic, Diagnostics, Range, Severity, Source};
use std::convert::identity;
use std::sync::Arc;

pub struct CheckAllReferencesAreDefined;

#[async_trait]
impl Analyzer for CheckAllReferencesAreDefined {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(&self, ctx: AnalysisContext<()>) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let module = &ctx.module.clone();
        for diagnostic in futures::future::join_all(ctx.navigator.traverse().map(
            async move |child| -> Option<Arc<dyn Diagnostic>> {
                if let Some(reference) = child.node.clone().as_reference_expression() {
                    if let None = module.declaration_referenced_by(reference.clone()).await {
                        return Some(Arc::new(UndefinedReference(reference)));
                    }
                }
                if let Some(reference) = child.node.clone().as_reference_type_expression() {
                    if let None = module
                        .declaration_referenced_by_type(reference.clone())
                        .await
                    {
                        return Some(Arc::new(UndefinedTypeReference(reference)));
                    }
                }
                return None;
            },
        ))
        .await
        .into_iter()
        .filter_map(identity)
        {
            diagnostics.push_dyn(diagnostic);
        }
        diagnostics
    }
}

#[derive(Debug, Clone)]
pub struct UndefinedReference(pub Arc<ReferenceExpression>);

impl Diagnostic for UndefinedReference {
    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn source(&self) -> &Arc<Source> {
        &self.0.source()
    }

    fn range(&self) -> Range {
        self.0.range()
    }

    fn message(&self) -> String {
        format!(
            "Undefined reference `{}`",
            self.0.symbol.identifier.lexeme()
        )
    }
}

#[derive(Debug, Clone)]
pub struct UndefinedTypeReference(pub Arc<ReferenceTypeExpression>);

impl Diagnostic for UndefinedTypeReference {
    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn source(&self) -> &Arc<Source> {
        &self.0.source()
    }

    fn range(&self) -> Range {
        self.0.range()
    }

    fn message(&self) -> String {
        format!(
            "Undefined reference `{}`",
            self.0.symbol.identifier.lexeme()
        )
    }
}

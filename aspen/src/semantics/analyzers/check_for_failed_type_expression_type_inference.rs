use crate::semantics::types::Type;
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Node, TypeExpression};
use crate::{Diagnostic, Diagnostics, Range, Severity, Source};
use futures::FutureExt;
use std::sync::Arc;

pub struct CheckForFailedTypeExpressionTypeInference;

#[async_trait]
impl Analyzer for CheckForFailedTypeExpressionTypeInference {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(&self, ctx: AnalysisContext<()>) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let types = futures::future::join_all(
            ctx.navigator
                .all_type_expressions()
                .map(|e| ctx.module.resolve_type(e.clone()).map(|t| (t, e))),
        )
        .await;
        for (type_, e) in types {
            if let Type::Failed { diagnosed: false } = type_ {
                diagnostics.push(TypeExpressionTypeInferenceFailed(e));
            }
        }
        diagnostics
    }
}

#[derive(Debug)]
struct TypeExpressionTypeInferenceFailed(Arc<TypeExpression>);

impl Diagnostic for TypeExpressionTypeInferenceFailed {
    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn source(&self) -> &Arc<Source> {
        self.0.source()
    }

    fn range(&self) -> Range {
        self.0.range()
    }

    fn message(&self) -> String {
        "Could not figure this type out".into()
    }
}

use crate::semantics::types::Type;
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Expression, Node};
use crate::{Diagnostic, Diagnostics, Range, Severity, Source};
use futures::FutureExt;
use std::sync::Arc;

pub struct CheckForFailedTypeInference;

#[async_trait]
impl Analyzer for &CheckForFailedTypeInference {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(self, ctx: AnalysisContext<()>) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let types = futures::future::join_all(
            ctx.navigator
                .all_expressions()
                .map(|e| ctx.module.get_type_of(e.clone()).map(|t| (t, e))),
        )
        .await;
        for (type_, e) in types {
            if let Type::Failed { diagnosed: false } = type_ {
                diagnostics.push(TypeInferenceFailed(e));
            }
        }
        diagnostics
    }
}

#[derive(Debug)]
struct TypeInferenceFailed(Arc<Expression>);

impl Diagnostic for TypeInferenceFailed {
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
        "Could not figure out the type of this expression".into()
    }
}

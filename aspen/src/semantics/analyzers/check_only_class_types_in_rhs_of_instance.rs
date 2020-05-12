use crate::semantics::types::Type;
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Node, TypeExpression};
use crate::{Diagnostic, Diagnostics, Range, Severity, Source};
use std::sync::Arc;

pub struct CheckOnlyClassTypesInRHSOfInstance;

#[async_trait]
impl Analyzer for CheckOnlyClassTypesInRHSOfInstance {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(&self, ctx: AnalysisContext<()>) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        for dec in ctx.navigator.all_instance_declarations() {
            let rhs = ctx.module.resolve_type(dec.rhs.clone()).await;
            match rhs {
                Type::Class(_) | Type::Failed { diagnosed: true } => {} // OK
                t => {
                    diagnostics.push(CanOnlyImplementClasses(dec.rhs.clone(), t));
                }
            }
        }
        diagnostics
    }
}

#[derive(Debug)]
struct CanOnlyImplementClasses(Arc<TypeExpression>, Type);

impl Diagnostic for CanOnlyImplementClasses {
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
        format!(
            "Only classes can be assigned an instance. This type is {}.",
            match &self.1 {
                Type::Class(_) => "a class",
                Type::Object(_) => "an object",
                Type::Failed { .. } => "unknown",
                Type::Unbounded(_, _) => "a type variable",
            }
        )
    }
}

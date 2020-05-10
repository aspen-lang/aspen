use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Declaration, Inline, IntoNode, ReferenceExpression, Root};
use crate::SourceKind;
use std::option::NoneError;
use std::sync::Arc;

#[derive(Clone)]
pub struct FindDeclaration;

#[async_trait]
impl Analyzer for FindDeclaration {
    type Input = Arc<ReferenceExpression>;
    type Output = Result<Arc<Declaration>, FindDeclarationError>;

    async fn analyze(&self, ctx: AnalysisContext<Self::Input>) -> Self::Output {
        let reference = ctx.input.clone();
        let name = reference.symbol.identifier.lexeme();

        let navigator = ctx.navigator.down_to(&ctx.input.into_node())?;

        let declaration_in_scope = navigator
            .find_upward(|node| {
                if let Some(dec) = node.clone().as_declaration() {
                    if dec.symbol() == name {
                        return true;
                    }
                }
                false
            })
            .and_then(|d| d.as_declaration());

        if let Some(declaration) = declaration_in_scope {
            return Ok(declaration);
        }

        match reference.source.kind {
            SourceKind::Inline => {
                for module in ctx.host.modules().await {
                    if module.uri() != ctx.module.uri() {
                        if let Root::Inline(other_inline) = module.syntax_tree().as_ref() {
                            if let Inline::Declaration(dec) = other_inline.as_ref() {
                                if dec.symbol() == name {
                                    return Ok(dec.clone());
                                }
                            }
                        }
                    }
                }
            }

            SourceKind::Module => {
                // TODO: Imports
            }
        }

        Err(FindDeclarationError::NotFound)
    }
}

#[derive(Debug, Clone)]
pub enum FindDeclarationError {
    NotFound,
}

impl From<NoneError> for FindDeclarationError {
    fn from(_: NoneError) -> Self {
        FindDeclarationError::NotFound
    }
}

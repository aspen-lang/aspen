use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::Node;
use std::option::NoneError;
use std::sync::Arc;

pub struct FindDeclaration;

#[async_trait]
impl<'a> Analyzer for &'a FindDeclaration {
    type Input = Arc<Node>;
    type Output = Result<Arc<Node>, FindDeclarationError>;

    async fn analyze(self, ctx: AnalysisContext<Self::Input>) -> Self::Output {
        let navigator = ctx.navigator.down_to(&ctx.input)?;

        Ok(navigator.find_upward(|_node| {
            // CHECK IF NODE IS A DECLARATION OF THE SAME SYMBOL
            // AS BEING REFERENCED BY THE INPUT NODE
            false
        })?)
    }
}

#[derive(Debug)]
pub enum FindDeclarationError {
    NotFound,
}

impl From<NoneError> for FindDeclarationError {
    fn from(_: NoneError) -> Self {
        FindDeclarationError::NotFound
    }
}

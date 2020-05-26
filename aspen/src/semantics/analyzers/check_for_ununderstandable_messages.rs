use crate::semantics::types::{Behaviour, Type};
use crate::semantics::{AnalysisContext, Analyzer};
use crate::syntax::{Expression, MessageSend, Node};
use crate::{Diagnostic, Diagnostics, Range, Severity, Source};
use futures::future::{join, join_all};
use std::sync::Arc;

pub struct CheckForUnunderstandableMessages;

#[async_trait]
impl Analyzer for CheckForUnunderstandableMessages {
    type Input = ();
    type Output = Diagnostics;

    async fn analyze(&self, ctx: AnalysisContext<()>) -> Diagnostics {
        join_all(ctx.navigator.all_message_sends().map(|send| {
            let module = ctx.module.clone();
            async move {
                let MessageSend {
                    receiver, message, ..
                } = send.as_ref();

                let ((receiver_type, behaviours), message_type) = join(
                    async {
                        let receiver = module.get_type_of(receiver.clone()).await;
                        let behaviours = module.get_behaviours_of_type(receiver.clone()).await;
                        (receiver, behaviours)
                    },
                    module.get_type_of(message.clone()),
                )
                .await;

                if let Type::Failed { .. } = receiver_type {
                    return None;
                }

                for Behaviour { selector, .. } in behaviours {
                    if message_type <= selector {
                        return None;
                    }
                }

                Some(UnunderstandableMessage {
                    receiver: (receiver_type, receiver.clone()),
                    message: (message_type, message.clone()),
                })
            }
        }))
        .await
        .into_iter()
        .filter_map(|o| o)
        .map(|u| Arc::new(u) as Arc<dyn Diagnostic>)
        .collect()
    }
}

#[derive(Debug)]
struct UnunderstandableMessage {
    pub receiver: (Type, Arc<Expression>),
    pub message: (Type, Arc<Expression>),
}

impl Diagnostic for UnunderstandableMessage {
    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn source(&self) -> &Arc<Source> {
        self.receiver.1.source()
    }

    fn range(&self) -> Range {
        self.receiver.1.range().through(self.message.1.range())
    }

    fn message(&self) -> String {
        format!("{} does not understand {}", self.receiver.0, self.message.0)
    }
}

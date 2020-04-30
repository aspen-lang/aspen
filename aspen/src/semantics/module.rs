use crate::semantics::host::Host;
use crate::syntax::{Lexer, Navigator, Node, NodeKind, Parser};
use crate::{Diagnostics, Source};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Module {
    root_node: Arc<Node>,
    diagnostics: Diagnostics,

    #[allow(unused)]
    host: Host,

    exported_declarations: HashMap<String, Arc<Node>>,
}

impl Module {
    pub async fn parse(source: Arc<Source>, host: Host) -> Module {
        let (root_node, diagnostics) = Parser::new(Lexer::tokenize(&source)).parse_module().await;

        let mut module = Module {
            root_node,
            diagnostics,
            host,

            exported_declarations: HashMap::new(),
        };

        module.analyze().await;

        module
    }

    #[inline]
    fn navigate<'a>(&self) -> Navigator<'a> {
        Navigator::new(self.root_node.clone())
    }

    async fn analyze(&mut self) {
        for declaration in self.navigate().children() {
            match &declaration.node.kind {
                NodeKind::ObjectDeclaration { symbol, .. }
                | NodeKind::ClassDeclaration { symbol, .. } => {
                    if let Some(symbol) = symbol.as_ref().map(Arc::as_ref).and_then(Node::symbol) {
                        self.exported_declarations
                            .insert(symbol, declaration.node.clone());
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_source() {
        let mut host = Host::new();
        host.set(Source::new("test:x", "object X.")).await;
        host.get(&"test:x".into(), |module| {
            assert_eq!(module.unwrap().exported_declarations.len(), 1);
            assert!(module.unwrap().exported_declarations.get("X").is_some());
        })
        .await;
    }
}

use crate::syntax::Node;
use std::sync::Arc;

#[derive(Clone)]
pub struct Navigator {
    parent: Option<Arc<Navigator>>,
    pub node: Arc<Node>,
}

impl Navigator {
    pub fn new(root: Arc<Node>) -> Arc<Navigator> {
        Arc::new(Navigator {
            parent: None,
            node: root,
        })
    }

    pub fn children<'a>(self: &'a Arc<Self>) -> impl Iterator<Item = Arc<Navigator>> + 'a {
        self.node.children().map(move |node| {
            Arc::new(Navigator {
                parent: Some(self.clone()),
                node: node.clone(),
            })
        })
    }

    pub fn parent(&self) -> Option<&Arc<Navigator>> {
        self.parent.as_ref()
    }

    pub fn traverse(self: &Arc<Self>) -> impl Iterator<Item = Arc<Navigator>> {
        Traverse {
            stack: vec![self.clone()],
        }
    }

    pub fn down_to(self: &Arc<Self>, node: &Arc<Node>) -> Option<Arc<Navigator>> {
        for nav in self.traverse() {
            if Arc::ptr_eq(&nav.node, node) {
                return Some(nav.clone());
            }
        }
        None
    }

    pub fn find_upward<F: Fn(&Arc<Node>) -> bool>(&self, predicate: F) -> Option<Arc<Node>> {
        let mut parent = self.parent.as_ref();

        while let Some(p) = parent {
            for child in p.children() {
                if predicate(&child.node) {
                    return Some(child.node.clone());
                }
            }

            parent = p.parent();
        }

        None
    }
}

struct Traverse {
    stack: Vec<Arc<Navigator>>,
}

impl Iterator for Traverse {
    type Item = Arc<Navigator>;

    fn next(&mut self) -> Option<Self::Item> {
        self.stack.pop().map(|parent| {
            let mut children: Vec<_> = parent.children().collect();
            children.reverse();

            self.stack.extend(children);
            parent.clone()
        })
    }
}

use crate::syntax::Node;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Navigator {
    parent: Option<Arc<Navigator>>,
    pub node: Arc<dyn Node>,
}

impl Navigator {
    pub fn new(root: Arc<dyn Node>) -> Arc<Navigator> {
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

    pub fn down_to(self: &Arc<Self>, node: &Arc<dyn Node>) -> Option<Arc<Navigator>> {
        for nav in self.traverse() {
            if nav.node.range() == node.range() {
                return Some(nav.clone());
            }
        }
        None
    }

    pub fn down_to_cast<R, F: Fn(Arc<dyn Node>) -> Option<R>>(self: &Arc<Self>, f: F) -> Option<R> {
        for nav in self.traverse() {
            if let Some(r) = f(nav.node.clone()) {
                return Some(r);
            }
        }
        None
    }

    pub fn find_upward<F: Fn(&Arc<dyn Node>) -> bool>(
        &self,
        predicate: F,
    ) -> Option<Arc<dyn Node>> {
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

#[derive(Debug)]
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

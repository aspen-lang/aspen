use crate::syntax::Node;
use std::sync::Arc;

#[derive(Clone)]
pub struct Navigator<'a> {
    parent: Option<&'a Navigator<'a>>,
    pub node: Arc<Node>,
}

impl Navigator<'static> {
    pub fn new(root: Arc<Node>) -> Navigator<'static> {
        Navigator {
            parent: None,
            node: root,
        }
    }
}

impl<'a> Navigator<'a> {
    pub fn children(&self) -> impl Iterator<Item = Navigator> {
        self.node.children().map(move |node| Navigator {
            parent: Some(self),
            node: node.clone(),
        })
    }

    pub fn parent(&self) -> Option<&Navigator> {
        self.parent
    }

    pub fn traverse(&self) -> impl Iterator<Item = &Arc<Node>> {
        Traverse {
            stack: vec![&self.node],
        }
    }
}

struct Traverse<'a> {
    stack: Vec<&'a Arc<Node>>,
}

impl<'a> Iterator for Traverse<'a> {
    type Item = &'a Arc<Node>;

    fn next(&mut self) -> Option<Self::Item> {
        self.stack.pop().map(|parent| {
            let mut children: Vec<_> = parent.children().collect();
            children.reverse();

            self.stack.extend(children);
            parent
        })
    }
}

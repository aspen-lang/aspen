use crate::syntax::{Expression, InstanceDeclaration, Node, TypeExpression};
use crate::Location;
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

    pub fn to_location(self: &Arc<Self>, location: &Location) -> Option<Arc<Navigator>> {
        let mut result = None;
        for nav in self.traverse() {
            let range = nav.node.range();

            if &range.start <= location && &range.end > location {
                result = Some(nav.clone())
            }

            if &range.start > location {
                break;
            }
        }
        result
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

    pub fn up_to_cast<R, F: Fn(Arc<dyn Node>) -> Option<R>>(self: &Arc<Self>, f: F) -> Option<R> {
        let mut current = Some(self.clone());
        while let Some(nav) = current {
            if let Some(n) = f(nav.node.clone()) {
                return Some(n);
            }
            current = nav.parent.clone();
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

    pub fn all_expressions(self: &Arc<Self>) -> impl Iterator<Item = Arc<Expression>> {
        self.traverse()
            .filter_map(|n| n.node.clone().as_expression())
    }

    pub fn all_type_expressions(self: &Arc<Self>) -> impl Iterator<Item = Arc<TypeExpression>> {
        self.traverse()
            .filter_map(|n| n.node.clone().as_type_expression())
    }

    pub fn all_instance_declarations(
        self: &Arc<Self>,
    ) -> impl Iterator<Item = Arc<InstanceDeclaration>> {
        self.traverse()
            .filter_map(|n| n.node.clone().as_instance_declaration())
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

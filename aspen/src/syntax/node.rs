use crate::syntax::Token;
use std::fmt;
use std::iter::empty;
use std::sync::Arc;

pub struct Node {
    pub kind: NodeKind,
}

impl Node {
    pub fn new(kind: NodeKind) -> Arc<Node> {
        Arc::new(Node { kind })
    }

    pub fn children(&self) -> impl Iterator<Item = &Arc<Node>> {
        self.kind.children()
    }

    pub fn symbol(&self) -> Option<String> {
        if let NodeKind::Symbol(token) = &self.kind {
            Some(token.lexeme().into())
        } else {
            None
        }
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.kind.fmt(f)
    }
}

#[derive(Debug)]
pub enum NodeKind {
    EOF,
    Unknown(Arc<Token>),

    /// ```bnf
    /// Module :=
    ///   Declaration*
    /// ```
    Module {
        declarations: Vec<Arc<Node>>,
    },

    /// ```bnf
    /// Declaration :=
    ///   ObjectDeclaration |
    ///   ClassDeclaration
    /// ```

    /// ```bnf
    /// ObjectDeclaration :=
    ///   OBJECT_KEYWORD
    ///   Symbol
    ///   PERIOD
    /// ```
    ObjectDeclaration {
        keyword: Arc<Token>,
        symbol: Option<Arc<Node>>,
        period: Option<Arc<Token>>,
    },

    /// ```bnf
    /// ClassDeclaration :=
    ///   CLASS_KEYWORD
    ///   Symbol
    ///   PERIOD
    /// ```
    ClassDeclaration {
        keyword: Arc<Token>,
        symbol: Option<Arc<Node>>,
        period: Option<Arc<Token>>,
    },

    /// ```bnf
    /// Symbol :=
    ///   IDENTIFIER
    /// ```
    Symbol(Arc<Token>),
}

impl NodeKind {
    fn children(&self) -> NodeChildren {
        use NodeKind::*;
        match self {
            EOF | Unknown(_) | Symbol(_) => empty().into(),
            Module { declarations } => declarations.iter().into(),
            ObjectDeclaration { symbol, .. } => symbol.iter().into(),
            ClassDeclaration { symbol, .. } => symbol.iter().into(),
        }
    }
}

enum NodeChildren<'a> {
    Empty,
    Slice(std::slice::Iter<'a, Arc<Node>>),
    Option(std::option::Iter<'a, Arc<Node>>),
}

impl<'a> From<std::iter::Empty<&'a Arc<Node>>> for NodeChildren<'a> {
    fn from(_: std::iter::Empty<&'a Arc<Node>>) -> Self {
        NodeChildren::Empty
    }
}

impl<'a> From<std::slice::Iter<'a, Arc<Node>>> for NodeChildren<'a> {
    fn from(i: std::slice::Iter<'a, Arc<Node>>) -> Self {
        NodeChildren::Slice(i)
    }
}

impl<'a> From<std::option::Iter<'a, Arc<Node>>> for NodeChildren<'a> {
    fn from(i: std::option::Iter<'a, Arc<Node>>) -> Self {
        NodeChildren::Option(i)
    }
}

impl<'a> Iterator for NodeChildren<'a> {
    type Item = &'a Arc<Node>;

    fn next(&mut self) -> Option<Self::Item> {
        use NodeChildren::*;
        match self {
            Empty => None,
            Slice(i) => i.next(),
            Option(i) => i.next(),
        }
    }
}

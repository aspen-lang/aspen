use crate::syntax::Token;
use crate::{Range, Source};
use std::fmt;
use std::iter::empty;
use std::sync::Arc;
use std::convert::identity;

pub struct Node {
    pub source: Arc<Source>,
    pub kind: NodeKind,
}

impl Node {
    pub fn new(source: Arc<Source>, kind: NodeKind) -> Arc<Node> {
        Arc::new(Node { source, kind })
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

    pub fn range(&self) -> Range {
        self.kind.range(self.source.as_ref())
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use NodeKind::*;
        match &self.kind {
            Symbol(t) => write!(f, "Symbol({:?})", t.lexeme()),
            k => k.fmt(f)
        }
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

    fn range(&self, source: &Source) -> Range {
        use NodeKind::*;
        let opts = match self {
            EOF => return source.eof_range(),
            Unknown(t) => return t.range.clone(),
            Module { .. } => return source.range_all(),
            Symbol(t) => return t.range.clone(),
            ObjectDeclaration {
                keyword,
                symbol,
                period,
            } => vec![
                Some(keyword.range.clone()),
                symbol.as_ref().map(|n| n.range()),
                period.as_ref().map(|n| n.range.clone()),
            ],
            ClassDeclaration {
                keyword,
                symbol,
                period,
            } => vec![
                Some(keyword.range.clone()),
                symbol.as_ref().map(|n| n.range()),
                period.as_ref().map(|n| n.range.clone()),
            ],
        };
        Range::over(opts.into_iter().filter_map(identity))
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

use crate::syntax::Token;
use std::sync::Arc;
use std::fmt;

pub struct Node {
    pub kind: NodeKind,
}

impl Node {
    pub fn new(kind: NodeKind) -> Arc<Node> {
        Arc::new(Node { kind })
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

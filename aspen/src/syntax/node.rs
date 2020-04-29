use crate::syntax::Token;
use std::sync::Arc;

#[derive(Debug)]
pub enum Node {
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

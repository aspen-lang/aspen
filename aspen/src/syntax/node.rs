use crate::syntax::Token;
use crate::{Range, Source};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub trait Node: fmt::Debug + Send + Sync {
    fn source(&self) -> &Arc<Source>;
    fn range(&self) -> Range;
    fn children(&self) -> Children;

    fn as_module(self: Arc<Self>) -> Option<Arc<Module>> {
        None
    }

    fn as_declaration(self: Arc<Self>) -> Option<Arc<Declaration>> {
        None
    }

    fn as_reference_expression(self: Arc<Self>) -> Option<Arc<ReferenceExpression>> {
        None
    }
}

fn hash_node<N: Node, H: Hasher>(node: &N, state: &mut H) {
    Arc::into_raw(node.source().clone()).hash(state);
    node.range().hash(state);
}

pub trait IntoNode {
    fn into_node(self) -> Arc<dyn Node>;
}

impl<N: Node + 'static> IntoNode for Arc<N> {
    fn into_node(self) -> Arc<dyn Node> {
        self
    }
}

pub enum Children {
    None,
    Single(Option<Arc<dyn Node>>),
    Iter(Box<dyn Iterator<Item = Arc<dyn Node>>>),
}

impl Iterator for Children {
    type Item = Arc<dyn Node>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Children::None | Children::Single(None) => None,
            Children::Single(Some(_)) => {
                if let Children::Single(child) = std::mem::replace(self, Children::None) {
                    child
                } else {
                    None
                }
            }
            Children::Iter(iter) => iter.next(),
        }
    }
}

#[derive(Debug)]
pub struct Unknown {
    pub source: Arc<Source>,
    pub unknown: Arc<Token>,
}

impl Node for Unknown {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.unknown.range.clone()
    }

    fn children(&self) -> Children {
        Children::None
    }
}

/// ```bnf
/// Root :=
///   Module |
///   Inline
/// ```
pub enum Root {
    Module(Arc<Module>),
    Inline(Arc<Inline>),
}

impl fmt::Debug for Root {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Root::Module(n) => f.debug_tuple("Root::Module").field(n).finish(),
            Root::Inline(n) => f.debug_tuple("Root::Inline").field(n).finish(),
        }
    }
}

impl Node for Root {
    fn source(&self) -> &Arc<Source> {
        match self {
            Root::Module(n) => n.source(),
            Root::Inline(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Root::Module(n) => n.range(),
            Root::Inline(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Root::Module(n) => Children::Single(Some(n.clone())),
            Root::Inline(n) => Children::Single(Some(n.clone())),
        }
    }

    fn as_module(self: Arc<Self>) -> Option<Arc<Module>> {
        match &*self {
            Root::Module(m) => Some(m.clone()),
            _ => None,
        }
    }
}

/// ```bnf
/// Module :=
///   Declaration*
/// ```
pub struct Module {
    pub source: Arc<Source>,
    pub declarations: Vec<Arc<Declaration>>,
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} ", self.source)?;
        f.debug_list().entries(&self.declarations).finish()
    }
}

impl Node for Module {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        match (self.declarations.first(), self.declarations.last()) {
            (Some(first), Some(last)) => Range::over(vec![first.range(), last.range()]),
            (Some(single), None) | (None, Some(single)) => single.range(),
            (None, None) => self.source.range_all(),
        }
    }

    fn children(&self) -> Children {
        Children::Iter(Box::new(
            self.declarations
                .clone()
                .into_iter()
                .map(IntoNode::into_node),
        ))
    }

    fn as_module(self: Arc<Self>) -> Option<Arc<Module>> {
        Some(self)
    }
}

/// ```bnf
/// Inline :=
///   Declaration |
///   Expression
/// ```
pub enum Inline {
    Declaration(Arc<Declaration>),
    Expression(Arc<Expression>),
}

impl fmt::Debug for Inline {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Inline::Declaration(n) => f.debug_tuple("Inline::Declaration").field(n).finish(),
            Inline::Expression(n) => f.debug_tuple("Inline::Expression").field(n).finish(),
        }
    }
}

impl Node for Inline {
    fn source(&self) -> &Arc<Source> {
        match self {
            Inline::Declaration(n) => n.source(),
            Inline::Expression(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Inline::Declaration(n) => n.range(),
            Inline::Expression(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Inline::Declaration(n) => Children::Single(Some(n.clone())),
            Inline::Expression(n) => Children::Single(Some(n.clone())),
        }
    }
}

/// ```bnf
/// Declaration :=
///   ObjectDeclaration |
///   ClassDeclaration
/// ```
pub enum Declaration {
    Object(Arc<ObjectDeclaration>),
    Class(Arc<ClassDeclaration>),
}

impl fmt::Debug for Declaration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Declaration::Object(n) => f.debug_tuple("Declaration::Object").field(n).finish(),
            Declaration::Class(n) => f.debug_tuple("Declaration::Class").field(n).finish(),
        }
    }
}

impl Declaration {
    pub fn symbol(&self) -> &str {
        match self {
            Declaration::Object(n) => n.symbol(),
            Declaration::Class(n) => n.symbol(),
        }
    }
}

impl Node for Declaration {
    fn source(&self) -> &Arc<Source> {
        match self {
            Declaration::Object(n) => n.source(),
            Declaration::Class(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Declaration::Object(n) => n.range(),
            Declaration::Class(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Declaration::Object(n) => Children::Single(Some(n.clone())),
            Declaration::Class(n) => Children::Single(Some(n.clone())),
        }
    }

    fn as_declaration(self: Arc<Self>) -> Option<Arc<Declaration>> {
        Some(self)
    }
}

/// ```bnf
/// ObjectDeclaration :=
///   OBJECT_KEYWORD
///   Symbol
///   PERIOD
/// ```
pub struct ObjectDeclaration {
    pub source: Arc<Source>,
    pub keyword: Arc<Token>,
    pub symbol: Arc<Symbol>,
    pub period: Option<Arc<Token>>,
}

impl fmt::Debug for ObjectDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ObjectDeclaration")
            .field("symbol", &self.symbol)
            .finish()
    }
}

impl ObjectDeclaration {
    pub fn symbol(&self) -> &str {
        (*self.symbol).as_ref()
    }
}

impl Node for ObjectDeclaration {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.keyword.range.through(
            self.period
                .as_ref()
                .map(|t| t.range.clone())
                .unwrap_or(self.symbol.range()),
        )
    }

    fn children(&self) -> Children {
        Children::Single(Some(self.symbol.clone().into_node()))
    }
}

/// ```bnf
/// ClassDeclaration :=
///   CLASS_KEYWORD
///   Symbol
///   PERIOD
/// ```
pub struct ClassDeclaration {
    pub source: Arc<Source>,
    pub keyword: Arc<Token>,
    pub symbol: Arc<Symbol>,
    pub period: Option<Arc<Token>>,
}

impl fmt::Debug for ClassDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ClassDeclaration")
            .field("symbol", &self.symbol)
            .finish()
    }
}

impl ClassDeclaration {
    pub fn symbol(&self) -> &str {
        (*self.symbol).as_ref()
    }
}

impl Node for ClassDeclaration {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.keyword.range.through(
            self.period
                .as_ref()
                .map(|t| t.range.clone())
                .unwrap_or(self.symbol.range()),
        )
    }

    fn children(&self) -> Children {
        Children::Single(Some(self.symbol.clone().into_node()))
    }
}

/// ```bnf
/// Symbol :=
///   IDENTIFIER
/// ```
pub struct Symbol {
    pub source: Arc<Source>,
    pub identifier: Arc<Token>,
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Symbol({:?})", self.identifier.lexeme())
    }
}

impl Node for Symbol {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.identifier.range.clone()
    }

    fn children(&self) -> Children {
        Children::None
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        self.identifier.lexeme()
    }
}

/// ```bnf
/// Expression :=
///   ReferenceExpression
/// ```
pub enum Expression {
    Reference(Arc<ReferenceExpression>),
}

impl fmt::Debug for Expression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Expression::Reference(n) => f.debug_tuple("Expression::Reference").field(n).finish(),
        }
    }
}

impl Node for Expression {
    fn source(&self) -> &Arc<Source> {
        match self {
            Expression::Reference(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Expression::Reference(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Expression::Reference(n) => Children::Single(Some(n.clone())),
        }
    }
}

/// ```bnf
/// ReferenceExpression :=
///   Symbol
/// ```
pub struct ReferenceExpression {
    pub source: Arc<Source>,
    pub symbol: Arc<Symbol>,
}

impl fmt::Debug for ReferenceExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ReferenceExpression")
            .field("symbol", &self.symbol)
            .finish()
    }
}

impl Hash for ReferenceExpression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_node(self, state);
    }
}

impl Node for ReferenceExpression {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.symbol.range()
    }

    fn children(&self) -> Children {
        Children::Single(Some(self.symbol.clone()))
    }

    fn as_reference_expression(self: Arc<Self>) -> Option<Arc<ReferenceExpression>> {
        Some(self)
    }
}

use crate::syntax::Token;
use crate::{Range, Source};
use std::fmt;
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

    fn as_expression(self: Arc<Self>) -> Option<Arc<Expression>> {
        None
    }

    fn as_type_expression(self: Arc<Self>) -> Option<Arc<TypeExpression>> {
        None
    }

    fn as_reference_type_expression(self: Arc<Self>) -> Option<Arc<ReferenceTypeExpression>> {
        None
    }

    fn as_message_send(self: Arc<Self>) -> Option<Arc<MessageSend>> {
        None
    }
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
///   (Expression PERIOD)
/// ```
pub enum Inline {
    Declaration(Arc<Declaration>),
    Expression(Arc<Expression>, Option<Arc<Token>>),
}

impl fmt::Debug for Inline {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Inline::Declaration(n) => f.debug_tuple("Inline::Declaration").field(n).finish(),
            Inline::Expression(n, _) => f.debug_tuple("Inline::Expression").field(n).finish(),
        }
    }
}

impl Node for Inline {
    fn source(&self) -> &Arc<Source> {
        match self {
            Inline::Declaration(n) => n.source(),
            Inline::Expression(n, _) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Inline::Declaration(n) => n.range(),
            Inline::Expression(n, p) => {
                let range = n.range();
                match p {
                    Some(p) => range.through(p.range.clone()),
                    None => range,
                }
            }
        }
    }

    fn children(&self) -> Children {
        match self {
            Inline::Declaration(n) => Children::Single(Some(n.clone())),
            Inline::Expression(n, _) => Children::Single(Some(n.clone())),
        }
    }
}

/// ```bnf
/// Declaration :=
///   ObjectDeclaration
/// ```
pub enum Declaration {
    Object(Arc<ObjectDeclaration>),
}

impl fmt::Debug for Declaration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Declaration::Object(n) => f.debug_tuple("Declaration::Object").field(n).finish(),
        }
    }
}

impl Declaration {
    pub fn symbol(&self) -> &str {
        match self {
            Declaration::Object(n) => n.symbol(),
        }
    }
}

impl Node for Declaration {
    fn source(&self) -> &Arc<Source> {
        match self {
            Declaration::Object(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Declaration::Object(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Declaration::Object(n) => Children::Single(Some(n.clone())),
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
///   (PERIOD | ObjectBody)
/// ```
pub struct ObjectDeclaration {
    pub source: Arc<Source>,
    pub keyword: Arc<Token>,
    pub symbol: Arc<Symbol>,
    pub period: Option<Arc<Token>>,
    pub body: Option<Arc<ObjectBody>>,
}

impl fmt::Debug for ObjectDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ObjectDeclaration")
            .field("symbol", &self.symbol)
            .field("body", &self.body)
            .finish()
    }
}

impl ObjectDeclaration {
    pub fn symbol(&self) -> &str {
        (*self.symbol).as_ref()
    }

    pub fn methods(&self) -> impl Iterator<Item = &Arc<Method>> {
        static EMPTY: Vec<Arc<ObjectMember>> = vec![];
        (match &self.body {
            None => &EMPTY,
            Some(body) => &body.members,
        })
        .iter()
        .filter_map(|member| match member.as_ref() {
            ObjectMember::Method(m) => Some(m),
        })
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
                .or_else(|| self.body.as_ref().map(|b| b.range()))
                .unwrap_or(self.symbol.range()),
        )
    }

    fn children(&self) -> Children {
        match &self.body {
            None => Children::Single(Some(self.symbol.clone())),
            Some(body) => Children::Iter(Box::new(
                vec![self.symbol.clone().into_node(), body.clone().into_node()].into_iter(),
            )),
        }
    }
}

/// ```bnf
/// ObjectBody :=
///   OPEN_CURLY
///   ObjectMember*
///   CLOSE_CURLY
/// ```
pub struct ObjectBody {
    pub source: Arc<Source>,
    pub open_curly: Arc<Token>,
    pub members: Vec<Arc<ObjectMember>>,
    pub close_curly: Option<Arc<Token>>,
}

impl fmt::Debug for ObjectBody {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ObjectBody")
            .field("members", &self.members)
            .finish()
    }
}

impl Node for ObjectBody {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.open_curly.range.clone().through(
            self.close_curly
                .as_ref()
                .unwrap_or(&self.open_curly)
                .range
                .clone(),
        )
    }

    fn children(&self) -> Children {
        Children::Iter(Box::new(
            self.members.clone().into_iter().map(|m| m as Arc<dyn Node>),
        ))
    }
}

/// ```bnf
/// ObjectMember :=
///   ReferenceTypeExpression
/// ```
pub enum ObjectMember {
    Method(Arc<Method>),
}

impl fmt::Debug for ObjectMember {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ObjectMember::Method(n) => f.debug_tuple("ObjectMember::Method").field(n).finish(),
        }
    }
}

impl Node for ObjectMember {
    fn source(&self) -> &Arc<Source> {
        match self {
            ObjectMember::Method(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            ObjectMember::Method(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            ObjectMember::Method(n) => Children::Single(Some(n.clone())),
        }
    }
}

/// ```bnf
/// Method :=
///   Pattern
/// ```
pub struct Method {
    pub source: Arc<Source>,
    pub pattern: Arc<Pattern>,
}

impl fmt::Debug for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Method")
            .field("pattern", &self.pattern)
            .finish()
    }
}

impl Node for Method {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.pattern.range()
    }

    fn children(&self) -> Children {
        Children::None
    }
}

/// ```bnf
/// Pattern :=
///   Integer
/// ```
pub enum Pattern {
    Integer(Arc<Integer>),
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Pattern::Integer(n) => f.debug_tuple("Pattern::Integer").field(n).finish(),
        }
    }
}

impl Node for Pattern {
    fn source(&self) -> &Arc<Source> {
        match self {
            Pattern::Integer(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Pattern::Integer(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Pattern::Integer(n) => Children::Single(Some(n.clone())),
        }
    }
}

/// ```bnf
/// TypeExpression :=
///   ReferenceTypeExpression
/// ```
pub enum TypeExpression {
    Reference(Arc<ReferenceTypeExpression>),
}

impl fmt::Debug for TypeExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TypeExpression::Reference(n) => {
                f.debug_tuple("TypeExpression::Reference").field(n).finish()
            }
        }
    }
}

impl Node for TypeExpression {
    fn source(&self) -> &Arc<Source> {
        match self {
            TypeExpression::Reference(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            TypeExpression::Reference(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            TypeExpression::Reference(n) => Children::Single(Some(n.clone())),
        }
    }

    fn as_type_expression(self: Arc<Self>) -> Option<Arc<TypeExpression>> {
        Some(self)
    }
}

/// ```bnf
/// ReferenceTypeExpression :=
///   Symbol
/// ```
pub struct ReferenceTypeExpression {
    pub source: Arc<Source>,
    pub symbol: Arc<Symbol>,
}

impl fmt::Debug for ReferenceTypeExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ReferenceTypeExpression")
            .field("symbol", &self.symbol)
            .finish()
    }
}

impl Node for ReferenceTypeExpression {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.symbol.range()
    }

    fn children(&self) -> Children {
        Children::Single(Some(self.symbol.clone()))
    }

    fn as_reference_type_expression(self: Arc<Self>) -> Option<Arc<ReferenceTypeExpression>> {
        Some(self)
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
///   Integer |
///   Float |
///   ReferenceExpression |
///   MessageSend |
///   NullaryAtomExpression
/// ```
pub enum Expression {
    Integer(Arc<Integer>),
    Float(Arc<Float>),
    Reference(Arc<ReferenceExpression>),
    MessageSend(Arc<MessageSend>),
    NullaryAtom(Arc<NullaryAtomExpression>),
}

impl fmt::Debug for Expression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Expression::Reference(n) => f.debug_tuple("Expression::Reference").field(n).finish(),
            Expression::Integer(n) => f.debug_tuple("Expression::Integer").field(n).finish(),
            Expression::Float(n) => f.debug_tuple("Expression::Float").field(n).finish(),
            Expression::MessageSend(n) => {
                f.debug_tuple("Expression::MessageSend").field(n).finish()
            }
            Expression::NullaryAtom(n) => f.debug_tuple("Expression::Atom").field(n).finish(),
        }
    }
}

impl Node for Expression {
    fn source(&self) -> &Arc<Source> {
        match self {
            Expression::Reference(n) => n.source(),
            Expression::Integer(n) => n.source(),
            Expression::Float(n) => n.source(),
            Expression::MessageSend(n) => n.source(),
            Expression::NullaryAtom(n) => n.source(),
        }
    }

    fn range(&self) -> Range {
        match self {
            Expression::Reference(n) => n.range(),
            Expression::Integer(n) => n.range(),
            Expression::Float(n) => n.range(),
            Expression::MessageSend(n) => n.range(),
            Expression::NullaryAtom(n) => n.range(),
        }
    }

    fn children(&self) -> Children {
        match self {
            Expression::Reference(n) => Children::Single(Some(n.clone())),
            Expression::Integer(n) => Children::Single(Some(n.clone())),
            Expression::Float(n) => Children::Single(Some(n.clone())),
            Expression::MessageSend(n) => Children::Single(Some(n.clone())),
            Expression::NullaryAtom(n) => Children::Single(Some(n.clone())),
        }
    }

    fn as_expression(self: Arc<Self>) -> Option<Arc<Expression>> {
        Some(self)
    }
}

/// ```bnf
/// MessageSend :=
///   Expression
///   Expression
/// ```
pub struct MessageSend {
    pub source: Arc<Source>,
    pub receiver: Arc<Expression>,
    pub message: Arc<Expression>,
}

impl fmt::Debug for MessageSend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MessageSend")
            .field("receiver", &self.receiver)
            .field("message", &self.message)
            .finish()
    }
}

impl Node for MessageSend {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.receiver.range().through(self.message.range())
    }

    fn children(&self) -> Children {
        Children::Iter(Box::new(
            vec![self.receiver.clone(), self.message.clone()]
                .into_iter()
                .map(IntoNode::into_node),
        ))
    }

    fn as_message_send(self: Arc<Self>) -> Option<Arc<MessageSend>> {
        Some(self)
    }
}

/// ```bnf
/// Integer :=
///   INTEGER_LITERAL
/// ```
pub struct Integer {
    pub source: Arc<Source>,
    pub literal: Arc<Token>,
}

impl fmt::Debug for Integer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Integer").field(&self.literal).finish()
    }
}

impl Node for Integer {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.literal.range.clone()
    }

    fn children(&self) -> Children {
        Children::None
    }
}

/// ```bnf
/// Float :=
///   FLOAT_LITERAL
/// ```
pub struct Float {
    pub source: Arc<Source>,
    pub literal: Arc<Token>,
}

impl fmt::Debug for Float {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Float").field(&self.literal).finish()
    }
}

impl Node for Float {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.literal.range.clone()
    }

    fn children(&self) -> Children {
        Children::None
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

/// ```bnf
/// NullaryAtomExpression :=
///   NULLARY_ATOM
/// ```
pub struct NullaryAtomExpression {
    pub source: Arc<Source>,
    pub atom: Arc<Token>,
}

impl fmt::Debug for NullaryAtomExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("NullaryAtomExpression")
            .field("atom", &self.atom)
            .finish()
    }
}

impl Node for NullaryAtomExpression {
    fn source(&self) -> &Arc<Source> {
        &self.source
    }

    fn range(&self) -> Range {
        self.atom.range.clone()
    }

    fn children(&self) -> Children {
        Children::None
    }
}

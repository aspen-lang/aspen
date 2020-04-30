use crate::syntax::Token;
use crate::{Range, Source};
use std::fmt::{self, Debug, Display};
use std::sync::Arc;

#[derive(Debug, PartialEq)]
pub enum Severity {
    Error,
    // Warning,
    // Hint,
}

pub trait Diagnostic
where
    Self: Send + Debug,
{
    fn severity(&self) -> Severity;
    fn source(&self) -> &Arc<Source>;
    fn range(&self) -> &Range;
    fn message(&self) -> String;
}

impl<'a> Display for &'a dyn Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}:{}: {:?}: {}",
            self.source().uri(),
            self.range(),
            self.severity(),
            self.message()
        )
    }
}

impl Display for Box<dyn Diagnostic> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.as_ref(), f)
    }
}

#[derive(Debug)]
pub struct Expected(pub String, pub Arc<Token>);

impl Diagnostic for Expected {
    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn source(&self) -> &Arc<Source> {
        &self.1.source
    }

    fn range(&self) -> &Range {
        &self.1.range
    }

    fn message(&self) -> String {
        format!("Expected {} but encountered {:?}", self.0, self.1)
    }
}

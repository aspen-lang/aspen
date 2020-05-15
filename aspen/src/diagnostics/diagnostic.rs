use crate::syntax::Node;
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
    Self: Send + Sync + Debug,
{
    fn severity(&self) -> Severity;
    fn source(&self) -> &Arc<Source>;
    fn range(&self) -> Range;
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

#[derive(Debug, Clone)]
pub struct DuplicateExport(pub String, pub Arc<dyn Node>);

impl Diagnostic for DuplicateExport {
    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn source(&self) -> &Arc<Source> {
        &self.1.source()
    }

    fn range(&self) -> Range {
        self.1.range()
    }

    fn message(&self) -> String {
        format!("Duplicate export `{}`", self.0)
    }
}

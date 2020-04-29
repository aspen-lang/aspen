use crate::Diagnostic;
use std::fmt;
use std::iter::FromIterator;

pub struct Diagnostics {
    diagnostics: Vec<Box<dyn Diagnostic>>,
}

impl Diagnostics {
    pub fn new() -> Diagnostics {
        Diagnostics {
            diagnostics: vec![],
        }
    }

    pub fn and(self, others: Diagnostics) -> Diagnostics {
        self.diagnostics
            .into_iter()
            .chain(others.diagnostics)
            .collect()
    }

    pub fn looks_more_promising_than(&self, other: &Diagnostics) -> bool {
        self.diagnostics.len() < other.diagnostics.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Diagnostic> {
        self.diagnostics.iter().map(Box::as_ref)
    }

    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn push<D: Diagnostic + 'static>(&mut self, diagnostic: D) {
        self.diagnostics.push(Box::new(diagnostic));
    }

    pub fn push_all<D: Into<Diagnostics>>(&mut self, diagnostics: D) {
        self.diagnostics.extend(diagnostics.into().diagnostics);
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl From<Box<dyn Diagnostic>> for Diagnostics {
    fn from(diagnostic: Box<dyn Diagnostic>) -> Self {
        Diagnostics {
            diagnostics: vec![diagnostic],
        }
    }
}

impl From<Vec<Box<dyn Diagnostic>>> for Diagnostics {
    fn from(diagnostics: Vec<Box<dyn Diagnostic>>) -> Self {
        Diagnostics { diagnostics }
    }
}

impl FromIterator<Box<dyn Diagnostic>> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = Box<dyn Diagnostic>>>(iter: T) -> Self {
        Diagnostics {
            diagnostics: iter.into_iter().collect(),
        }
    }
}

impl fmt::Debug for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DIAGNOSTICS ({})", self.len())?;

        for (i, d) in self.iter().enumerate() {
            write!(f, "\n\n{}: {}", i + 1, d)?;
        }

        Ok(())
    }
}

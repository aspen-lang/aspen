use crate::{Diagnostic, Severity, Source};
use std::collections::HashMap;
use std::fmt;
use std::iter::FromIterator;
use std::sync::Arc;

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

    pub fn is_ok(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity() == Severity::Error)
    }

    pub fn group_by_source(self) -> HashMap<Arc<Source>, Diagnostics> {
        let mut map = HashMap::new();
        for d in self.diagnostics {
            let uri = d.source().clone();
            if !map.contains_key(&uri) {
                map.insert(uri.clone(), Diagnostics::new());
            }
            map.get_mut(&uri).unwrap().diagnostics.push(d);
        }
        map
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

impl IntoIterator for Diagnostics {
    type Item = Box<dyn Diagnostic>;
    type IntoIter = std::vec::IntoIter<Box<dyn Diagnostic>>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_iter()
    }
}

impl fmt::Debug for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Diagnostics ({})", self.len())?;

        for (i, d) in self.iter().enumerate() {
            write!(f, "\n- {}: {}", i + 1, d)?;
        }

        Ok(())
    }
}

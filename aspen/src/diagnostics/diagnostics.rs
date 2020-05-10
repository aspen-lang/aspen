use crate::{Diagnostic, Severity, Source};
use std::collections::HashMap;
use std::fmt;
use std::iter::FromIterator;
use std::sync::Arc;

#[derive(Clone)]
pub struct Diagnostics {
    diagnostics: Vec<Arc<dyn Diagnostic>>,
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
        self.diagnostics.iter().map(Arc::as_ref)
    }

    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn push<D: Diagnostic + 'static>(&mut self, diagnostic: D) {
        self.push_dyn(Arc::new(diagnostic));
    }

    pub fn push_dyn(&mut self, diagnostic: Arc<dyn Diagnostic>) {
        self.diagnostics.push(diagnostic);
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

impl From<Arc<dyn Diagnostic>> for Diagnostics {
    fn from(diagnostic: Arc<dyn Diagnostic>) -> Self {
        Diagnostics {
            diagnostics: vec![diagnostic],
        }
    }
}

impl From<Vec<Arc<dyn Diagnostic>>> for Diagnostics {
    fn from(diagnostics: Vec<Arc<dyn Diagnostic>>) -> Self {
        Diagnostics { diagnostics }
    }
}

impl From<Vec<Diagnostics>> for Diagnostics {
    fn from(diagnostics: Vec<Diagnostics>) -> Self {
        diagnostics.into_iter().collect()
    }
}

impl FromIterator<Arc<dyn Diagnostic>> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = Arc<dyn Diagnostic>>>(iter: T) -> Self {
        Diagnostics {
            diagnostics: iter.into_iter().collect(),
        }
    }
}

impl FromIterator<Diagnostics> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostics>>(iter: I) -> Diagnostics {
        let mut result = Diagnostics::new();
        for d in iter {
            result.push_all(d);
        }
        result
    }
}

impl IntoIterator for Diagnostics {
    type Item = Arc<dyn Diagnostic>;
    type IntoIter = std::vec::IntoIter<Arc<dyn Diagnostic>>;

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

impl Default for Diagnostics {
    fn default() -> Self {
        Diagnostics::new()
    }
}

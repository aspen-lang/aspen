use crate::source::Source;
use std::fmt;

#[derive(Clone, PartialEq)]
pub struct Location {
    pub offset: usize,
    pub line: usize,
    pub character: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.line)
    }
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.character)
    }
}

pub trait IntoLocation {
    fn into_location(self, source: &Source) -> Location;
}

impl IntoLocation for usize {
    fn into_location(self, source: &Source) -> Location {
        source.location_at(self)
    }
}

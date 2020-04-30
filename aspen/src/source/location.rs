use crate::source::Source;
use std::fmt;
use std::cmp::Ordering;

#[derive(Clone, PartialEq, Eq)]
pub struct Location {
    pub offset: usize,
    pub line: usize,
    pub character: usize,
}

impl Default for Location {
    fn default() -> Self {
        Location {
            offset: Default::default(),
            line: Default::default(),
            character: Default::default(),
        }
    }
}

impl PartialOrd for Location {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Location {
    fn cmp(&self, other: &Self) -> Ordering {
        self.offset.cmp(&other.offset)
    }
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

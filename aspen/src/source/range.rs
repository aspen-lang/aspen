use crate::source::{IntoLocation, Location, Source};
use std::fmt;

#[derive(Clone, PartialEq)]
pub struct Range {
    pub start: Location,
    pub end: Location,
}

impl fmt::Debug for Range {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}->{}", self.start, self.end)
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.start)
    }
}

pub trait IntoRange {
    fn into_range(self, source: &Source) -> Range;
}

impl<T: IntoLocation> IntoRange for std::ops::Range<T> {
    fn into_range(self, source: &Source) -> Range {
        let start = self.start.into_location(source);
        let end = self.end.into_location(source);

        Range { start, end }
    }
}

impl<'a> Into<std::ops::Range<usize>> for &'a Range {
    fn into(self) -> std::ops::Range<usize> {
        self.start.offset..self.end.offset
    }
}
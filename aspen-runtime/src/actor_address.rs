use core::fmt;

#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct ActorAddress(pub usize);

impl fmt::Display for ActorAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<0.0.{:x}>", self.0)
    }
}

impl fmt::Debug for ActorAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<0.0.{:x}>", self.0)
    }
}

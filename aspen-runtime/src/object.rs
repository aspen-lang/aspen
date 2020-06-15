use crate::ActorRef;
use core::fmt;

#[derive(Debug, PartialEq)]
pub enum Object {
    Noop,
    Int(i128),
    Float(f64),
    Atom(&'static str),
    Actor(ActorRef),
}

impl Object {
    pub fn matches(&self, matcher: &Matcher) -> bool {
        matcher.matches(self)
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Object::Noop => write!(f, "_"),
            Object::Int(v) => write!(f, "{}", v),
            Object::Float(v) => write!(f, "{}", v),
            Object::Atom(v) => write!(f, "{}", v),
            Object::Actor(v) => write!(f, "{}", v),
        }
    }
}

pub enum Matcher {
    Equal(Object),
}

impl Matcher {
    pub fn matches(&self, object: &Object) -> bool {
        match self {
            Matcher::Equal(o) => o == object,
        }
    }
}

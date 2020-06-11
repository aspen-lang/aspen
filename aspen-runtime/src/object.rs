use crate::{ActorRef, ObjectRef};
use core::fmt;

pub enum Object {
    Int(i128),
    Float(f64),
    Atom(&'static str),
    Actor(ActorRef),
}

impl Object {
    pub fn send(&self, message: ObjectRef) {
        match self {
            Object::Int(i) => {
                println!("Handle builtin {} -> {}", message, i);
            }
            Object::Float(f) => {
                println!("Handle builtin {} -> {}", message, f);
            }
            Object::Atom(a) => {
                println!("Handle builtin {} -> {}", message, a);
            }
            Object::Actor(a) => {
                a.dispatch(message);
            }
        }
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Object::Int(v) => write!(f, "{}", v),
            Object::Float(v) => write!(f, "{}", v),
            Object::Atom(v) => write!(f, "{}", v),
            Object::Actor(v) => write!(f, "{}", v),
        }
    }
}

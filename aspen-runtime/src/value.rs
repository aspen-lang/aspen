use crate::{Object, PendingReply, Recv, Reply};
use std::fmt;
use std::sync::Arc;

#[derive(Debug)]
pub enum Value {
    Integer(i128),
    Float(f64),
    String(String),
    Nullary(&'static str),
    Object(Object),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Integer(a), Integer(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (String(a), String(b)) => a == b,
            (Nullary(a), Nullary(b)) => a == b,
            (Object(_), Object(_)) => self as *const _ == other as *const _,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Value::*;
        match self {
            Integer(v) => write!(f, "{}", v),
            Float(v) => write!(f, "{}", v),
            String(v) => write!(f, "{}", v),
            Object(_) => write!(f, "object"),
            Nullary(v) => write!(f, "#{}", v),
        }
    }
}

impl Value {
    pub fn new_int(value: i128) -> Arc<Value> {
        Arc::new(Value::Integer(value))
    }

    pub fn new_float(value: f64) -> Arc<Value> {
        Arc::new(Value::Float(value))
    }

    pub fn new_string(value: String) -> Arc<Value> {
        Arc::new(Value::String(value))
    }

    pub fn new_nullary(value: &'static str) -> Arc<Value> {
        Arc::new(Value::Nullary(value))
    }

    pub fn new_object(size: usize, recv: Recv) -> Arc<Value> {
        Arc::new(Value::Object(Object::new(size, recv)))
    }

    pub fn accept_message(&self, message: &Arc<Value>) -> Reply {
        match self {
            Value::Object(o) => o.accept_message(message.clone()),
            Value::Integer(self_) => match message.as_ref() {
                Value::Integer(other) => Reply::Answer(Value::new_int(*self_ * *other)),
                Value::Nullary("increment!") => Reply::Answer(Value::new_int(*self_ + 1)),

                _ => Reply::Rejected,
            },
            _ => Reply::Rejected,
        }
    }

    pub fn schedule_message(self: Arc<Self>, message: Arc<Value>) -> Arc<PendingReply> {
        PendingReply::new(self, message)
    }
}

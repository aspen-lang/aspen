use crate::reply::Reply;
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct Object {
    state: Mutex<()>,
}

impl Object {
    pub fn new() -> Object {
        Object {
            state: Mutex::new(()),
        }
    }

    pub fn accept_message(&self, message: &Arc<Value>) -> Result<Arc<Value>, ()> {
        let _guard = self.state.try_lock().map_err(|_| ())?;
        unsafe {
            if libc::rand() % 1000 > 1 {
                return Err(());
            }
        }
        Ok(message.clone())
    }
}

#[derive(Debug)]
pub enum Value {
    Integer(i128),
    Float(f64),
    Object(Object),
    Nullary(&'static str),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Value::*;
        match self {
            Integer(v) => write!(f, "{}", v),
            Float(v) => write!(f, "{}", v),
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

    pub fn new_nullary(value: &'static str) -> Arc<Value> {
        Arc::new(Value::Nullary(value))
    }

    pub fn new_object() -> Arc<Value> {
        Arc::new(Value::Object(Object::new()))
    }

    pub fn accept_message(&self, message: &Arc<Value>) -> Result<Arc<Value>, ()> {
        match self {
            Value::Object(o) => o.accept_message(message),
            Value::Integer(self_) => match message.as_ref() {
                Value::Integer(other) => Ok(Value::new_int(*self_ * *other)),
                Value::Nullary("increment!") => Ok(Value::new_int(*self_ + 1)),

                _ => Ok(message.clone()),
            },
            _ => Ok(message.clone()),
        }
    }

    pub fn schedule_message(self: Arc<Self>, message: Arc<Value>) -> Arc<Reply> {
        Reply::new(self, message)
    }
}

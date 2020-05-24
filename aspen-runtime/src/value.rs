use crate::reply::Reply;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum Value {
    Integer(i128),
    Float(f64),
    Object(Mutex<()>),
}

impl Value {
    pub fn new_int(value: i128) -> Arc<Value> {
        Arc::new(Value::Integer(value))
    }

    pub fn new_float(value: f64) -> Arc<Value> {
        Arc::new(Value::Float(value))
    }

    pub fn new_object() -> Arc<Value> {
        Arc::new(Value::Object(Mutex::new(())))
    }

    pub fn accept_message(&self, message: &Arc<Value>) -> Result<Arc<Value>, ()> {
        match self {
            Value::Object(m) => {
                let _guard = m.try_lock().map_err(|_| ())?;
                println!("MUTUALLY ACCESSES OBJECT");
                Ok(message.clone())
            }
            _ => Ok(message.clone()),
        }
    }

    pub fn schedule_message(self: Arc<Self>, message: Arc<Value>) -> Arc<Reply> {
        Reply::new(self, message)
    }
}

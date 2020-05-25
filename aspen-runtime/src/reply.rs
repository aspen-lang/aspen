use crate::Value;
use std::sync::Arc;

pub enum Reply {
    Answer(Arc<Value>),
    Panic,
    Rejected,
    Pending,
}

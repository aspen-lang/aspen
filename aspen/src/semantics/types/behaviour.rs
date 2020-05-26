use crate::semantics::types::Type;

#[derive(Debug, Clone)]
pub struct Behaviour {
    pub selector: Type,
    pub reply: Type,
}

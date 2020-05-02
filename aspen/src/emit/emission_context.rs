pub struct EmissionContext {
    pub(crate) inner: inkwell::context::Context,
}

impl EmissionContext {
    pub fn new() -> EmissionContext {
        EmissionContext {
            inner: inkwell::context::Context::create(),
        }
    }
}

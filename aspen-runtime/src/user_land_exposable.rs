use crate::{Reply, Value};
use std::sync::Arc;

pub trait UserLandExposable {
    fn expose(self: Arc<Self>) -> *const Self {
        Arc::into_raw(self)
    }

    unsafe fn enclose(self: *const Self) -> Arc<Self> {
        Arc::from_raw(self)
    }
}

impl UserLandExposable for Value {}
impl UserLandExposable for Reply {}

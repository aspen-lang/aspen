use crate::Object;
use alloc::boxed::Box;
use core::fmt;
use core::ops::Deref;
use core::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
pub struct ObjectRef {
    ptr: *mut Object,
    ref_count: *mut AtomicUsize,
}

impl ObjectRef {
    pub fn new(object: Object) -> ObjectRef {
        let object = Box::new(object);
        let ref_count = Box::new(AtomicUsize::new(1));
        ObjectRef {
            ptr: Box::into_raw(object),
            ref_count: Box::into_raw(ref_count),
        }
    }
}

impl Deref for ObjectRef {
    type Target = Object;

    fn deref(&self) -> &Object {
        unsafe { self.ptr.as_ref().unwrap() }
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl Clone for ObjectRef {
    fn clone(&self) -> Self {
        unsafe {
            self.ref_count
                .as_ref()
                .unwrap()
                .fetch_add(1, Ordering::Relaxed);
        }
        ObjectRef {
            ptr: self.ptr,
            ref_count: self.ref_count,
        }
    }
}

impl Drop for ObjectRef {
    fn drop(&mut self) {
        unsafe {
            if self
                .ref_count
                .as_ref()
                .unwrap()
                .fetch_sub(1, Ordering::Relaxed)
                == 1
            {
                Box::from_raw(self.ptr);
                Box::from_raw(self.ref_count);
            }
        }
    }
}

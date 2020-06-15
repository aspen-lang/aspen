use crate::{ActorAddress, Object, Runtime};
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

    pub fn tell(&self, message: ObjectRef) {
        match self.deref() {
            Object::Noop => {}
            Object::Int(i) => {
                println!("Handle builtin tell {} -> {}", message, i);
            }
            Object::Float(f) => {
                println!("Handle builtin tell {} -> {}", message, f);
            }
            Object::Atom(a) => {
                println!("Handle builtin tell {} -> {}", message, a);
            }
            Object::Actor(a) => {
                unsafe { &*a.runtime }.enqueue(self.clone(), a.address, message, None);
            }
        }
    }

    pub fn ask(&self, message: ObjectRef, reply_to: ObjectRef) {
        match self.deref() {
            Object::Noop => {}
            Object::Int(i) => {
                println!("Handle builtin ask {} -> {}", message, i);
            }
            Object::Float(f) => {
                println!("Handle builtin ask {} -> {}", message, f);
            }
            Object::Atom(a) => {
                println!("Handle builtin ask {} -> {}", message, a);
            }
            Object::Actor(a) => {
                unsafe { &*a.runtime }.enqueue(self.clone(), a.address, message, Some(reply_to));
            }
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
        fmt::Display::fmt(self.deref(), f)
    }
}

impl fmt::Debug for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.deref(), f)
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

impl ObjectRef {
    pub fn into_weak(self) -> WeakObjectRef {
        WeakObjectRef {
            ptr: self.ptr,
            ref_count: self.ref_count,
        }
    }
}

pub struct WeakObjectRef {
    ptr: *mut Object,
    ref_count: *mut AtomicUsize,
}

impl WeakObjectRef {
    pub fn into_strong(&self) -> ObjectRef {
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

#[derive(PartialEq)]
pub struct ActorRef {
    runtime: *const Runtime,
    address: ActorAddress,
}

impl ActorRef {
    #[inline]
    pub fn new(runtime: *const Runtime, address: ActorAddress) -> ActorRef {
        ActorRef { runtime, address }
    }
}

impl Drop for ActorRef {
    fn drop(&mut self) {
        unsafe {
            self.runtime
                .as_ref()
                .unwrap()
                .schedule_deletion(self.address)
        }
    }
}

impl fmt::Debug for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "*actor{}", self.address)
    }
}

impl fmt::Display for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "*actor{}", self.address)
    }
}

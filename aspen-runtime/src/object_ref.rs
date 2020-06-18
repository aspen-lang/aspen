use crate::{ActorAddress, Envelope, Inbox, Object, Runtime};
use alloc::boxed::Box;
use core::fmt;
use core::ops::Deref;
use core::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
#[derive(PartialEq)]
pub struct ObjectRef {
    ptr: *mut Object,
    ref_count: *mut AtomicUsize,
}

unsafe impl Sync for ObjectRef {}
unsafe impl Send for ObjectRef {}

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
            Object::Noop => {
                #[cfg(debug_assertions)]
                println!("NOOP {}.", message);
            }
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
                a.enqueue(self.clone(), a.address, None, message, None);
            }
            Object::Continuation(continuation) => {
                if let Object::Actor(a) = continuation.actor.deref() {
                    a.enqueue(
                        continuation.actor.clone(),
                        a.address,
                        None,
                        message,
                        Some(self.clone()),
                    );
                }
            }
        }
    }

    pub fn ask(&self, reply_to: ObjectRef, message: ObjectRef) {
        match self.deref() {
            Object::Noop => {
                #[cfg(debug_assertions)]
                println!("NOOP {}?", message);
            }
            Object::Int(i) => match message.deref() {
                Object::Int(j) => {
                    reply_to.tell(ObjectRef::new(Object::Int(i * j)));
                }
                _ => {
                    println!("Handle builtin ask {} -> {}", message, i);
                }
            },
            Object::Float(f) => {
                println!("Handle builtin ask {} -> {}", message, f);
            }
            Object::Atom(a) => {
                println!("Handle builtin ask {} -> {}", message, a);
            }
            Object::Actor(a) => {
                a.enqueue(self.clone(), a.address, Some(reply_to), message, None);
            }
            Object::Continuation(continuation) => {
                if let Object::Actor(a) = continuation.actor.deref() {
                    a.enqueue(
                        continuation.actor.clone(),
                        a.address,
                        Some(reply_to),
                        message,
                        Some(self.clone()),
                    );
                } else {
                    panic!("Expected an actor, got {}", continuation.actor);
                }
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
    pub fn weak(&self) -> WeakObjectRef {
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
    inbox: *const Inbox,
}

impl ActorRef {
    #[inline]
    pub fn new(runtime: *const Runtime, address: ActorAddress, inbox: *const Inbox) -> ActorRef {
        ActorRef {
            runtime,
            address,
            inbox,
        }
    }

    fn enqueue(
        &self,
        self_ref: ObjectRef,
        _address: ActorAddress,
        reply_to: Option<ObjectRef>,
        message: ObjectRef,
        continuation_ref: Option<ObjectRef>,
    ) {
        unsafe { &*self.inbox }.push(Envelope {
            self_ref,
            reply_to: reply_to.unwrap_or_else(|| unsafe { &*self.runtime }.noop_object.clone()),
            message,
            continuation_ref,
        });
        unsafe { &*self.runtime }.notify();
    }
}

impl Drop for ActorRef {
    fn drop(&mut self) {
        unsafe { &*self.runtime }.schedule_deletion(self.address);
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

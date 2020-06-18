use crate::{ContFn, DropFn, Object, ObjectRef, Runtime};
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;
use core::pin::Pin;

pub struct Continuation {
    runtime: *const Runtime,
    pub actor: ObjectRef,
    pub cont_fn: ContFn,
    pub drop_fn: DropFn,
    frame: Pin<Vec<u8>>,
}

impl Continuation {
    pub fn new(
        runtime: &Runtime,
        actor: ObjectRef,
        cont_fn: ContFn,
        frame: Pin<Vec<u8>>,
        drop_fn: DropFn,
    ) -> Continuation {
        if let Object::Actor(_) = actor.deref() {
        } else {
            panic!("Can only create a continuation from an actor reference");
        }
        Continuation {
            runtime,
            actor,
            cont_fn,
            drop_fn,
            frame,
        }
    }

    pub fn frame_ptr(&self) -> *mut libc::c_void {
        self.frame.as_ptr() as *const u8 as *mut _
    }
}

impl Drop for Continuation {
    fn drop(&mut self) {
        (self.drop_fn)(self.runtime, self.frame_ptr());
    }
}

impl fmt::Display for Continuation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}[...]", self.actor)
    }
}

impl fmt::Debug for Continuation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}[...]", self.actor)
    }
}

impl PartialEq for Continuation {
    fn eq(&self, other: &Self) -> bool {
        self.actor == other.actor && self.cont_fn == other.cont_fn
    }
}

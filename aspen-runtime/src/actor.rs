use crate::{ActorRef, Object, ObjectRef, Runtime, WeakObjectRef};
use core::fmt;

#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct ActorAddress(pub usize);

impl fmt::Display for ActorAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<0.0.{:x}>", self.0)
    }
}

impl fmt::Debug for ActorAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<0.0.{:x}>", self.0)
    }
}

pub type InitFn = extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void);
pub type RecvFn =
    extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void, ObjectRef, ObjectRef);

pub struct Actor {
    runtime: *const Runtime,
    state_ptr: *mut libc::c_void,
    recv_fn: RecvFn,
    self_: WeakObjectRef,
    address: ActorAddress,
}

impl fmt::Debug for Actor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "actor{}", self.address)
    }
}

impl Actor {
    pub fn new(
        runtime: &Runtime,
        address: ActorAddress,
        state_size: usize,
        init_fn: InitFn,
        recv_fn: RecvFn,
    ) -> (ObjectRef, Actor) {
        unsafe {
            let self_ = ObjectRef::new(Object::Actor(ActorRef::new(runtime, address)));
            let actor = Actor {
                runtime,
                state_ptr: libc::malloc(state_size),
                recv_fn,
                self_: self_.clone().into_weak(),
                address,
            };
            init_fn(runtime, &actor.reference_to(), actor.state_ptr);
            (self_, actor)
        }
    }

    pub fn receive(&mut self, message: ObjectRef, reply_to: ObjectRef) {
        (self.recv_fn)(
            self.runtime,
            &self.reference_to(),
            self.state_ptr,
            reply_to,
            message,
        );
    }

    pub fn reference_to(&self) -> ObjectRef {
        self.self_.into_strong()
    }
}

impl Drop for Actor {
    fn drop(&mut self) {
        unsafe {
            libc::free(self.state_ptr);
        }
    }
}

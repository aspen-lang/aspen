use crate::{ActorRef, Object, ObjectRef, Runtime, WeakObjectRef};
use core::fmt;
use alloc::boxed::Box;
use alloc::vec::Vec;

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

pub type InitFn = extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void, ObjectRef);
pub type RecvFn =
    extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void, ObjectRef, ObjectRef);
pub type DropFn =
    extern "C" fn(*const Runtime, *mut libc::c_void);

pub struct Actor {
    runtime: *const Runtime,
    state_ptr: Vec<u8>,
    recv_fn: RecvFn,
    drop_fn: DropFn,
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
        init_msg: ObjectRef,
        init_fn: InitFn,
        recv_fn: RecvFn,
        drop_fn: DropFn,
    ) -> (ObjectRef, Actor) {
        let self_ = ObjectRef::new(Object::Actor(ActorRef::new(runtime, address)));
        let mut actor = Actor {
            runtime,
            state_ptr: Vec::with_capacity(state_size),
            recv_fn,
            drop_fn,
            self_: self_.clone().into_weak(),
            address,
        };
        init_fn(runtime, &actor.reference_to(), actor.state(), init_msg);
        (self_, actor)
    }

    #[inline]
    fn state(&mut self) -> *mut libc::c_void {
        self.state_ptr.as_mut_ptr() as *mut _
    }

    pub fn receive(&mut self, message: ObjectRef, reply_to: ObjectRef) {
        (self.recv_fn)(
            self.runtime,
            &self.reference_to(),
            self.state(),
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
        (self.drop_fn)(self.runtime, self.state());
    }
}

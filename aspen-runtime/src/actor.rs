use crate::{ActorAddress, ActorRef, Object, ObjectRef, Runtime, WeakObjectRef};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;
use core::pin::Pin;
use crossbeam_queue::SegQueue;

pub type InitFn = extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void, ObjectRef);
pub type RecvFn =
    extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void, ObjectRef, ObjectRef);
pub type DropFn = extern "C" fn(*const Runtime, *mut libc::c_void);

pub type Inbox = SegQueue<(ObjectRef, ObjectRef, ObjectRef)>;

pub struct Actor {
    runtime: *const Runtime,
    inbox: Pin<Box<Inbox>>,
    state_ptr: Vec<u8>,
    recv_fn: RecvFn,
    drop_fn: DropFn,
    self_: WeakObjectRef,
    pub address: ActorAddress,
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
        let inbox = Box::pin(Inbox::new());
        let self_ = ObjectRef::new(Object::Actor(ActorRef::new(
            runtime,
            address,
            inbox.deref(),
        )));
        let mut actor = Actor {
            runtime,
            inbox,
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
    pub fn inbox_is_empty(&self) -> bool {
        self.inbox.is_empty()
    }

    #[inline]
    fn state(&mut self) -> *mut libc::c_void {
        self.state_ptr.as_mut_ptr() as *mut _
    }

    pub fn receive(&mut self) -> bool {
        if let Ok((_self_ref, message, reply_to)) = self.inbox.pop() {
            (self.recv_fn)(
                self.runtime,
                &self.reference_to(),
                self.state(),
                reply_to,
                message,
            );
            true
        } else {
            false
        }
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

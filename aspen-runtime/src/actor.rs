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
pub type ContFn = extern "C" fn(
    *const Runtime,
    *const ObjectRef,
    *mut libc::c_void,
    *mut libc::c_void,
    ObjectRef,
    ObjectRef,
);

#[derive(Debug)]
pub struct Envelope {
    pub self_ref: ObjectRef,
    pub message: ObjectRef,
    pub reply_to: ObjectRef,
    pub continuation_ref: Option<ObjectRef>,
}

pub type Inbox = SegQueue<Envelope>;

pub struct Actor {
    runtime: *const Runtime,
    inbox: Pin<Box<Inbox>>,
    state_ptr: Pin<Vec<u8>>,
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
            state_ptr: Pin::new(Vec::with_capacity(state_size)),
            recv_fn,
            drop_fn,
            self_: self_.weak(),
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
        if let Ok(envelope) = self.inbox.pop() {
            let Envelope {
                self_ref,
                message,
                reply_to,
                continuation_ref,
            } = envelope;
            let state = self.state();
            match continuation_ref.as_ref().map(|c| c.deref()) {
                Some(Object::Continuation(cont)) => {
                    let frame = cont.frame_ptr();
                    (cont.cont_fn)(self.runtime, &self_ref, state, frame, reply_to, message)
                }

                None | Some(_) => {
                    (self.recv_fn)(self.runtime, &self_ref, state, reply_to, message);
                }
            }
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
        let state = self.state();
        (self.drop_fn)(self.runtime, state);
    }
}

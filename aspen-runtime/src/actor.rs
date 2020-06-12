use crate::{Object, ObjectRef, Runtime};
use core::fmt;

#[derive(Clone, Copy, Debug, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct ActorAddress(pub usize);

pub struct ActorRef {
    runtime: *const Runtime,
    address: ActorAddress,
}

impl ActorRef {
    #[inline]
    pub fn refer_to(runtime: &Runtime, address: ActorAddress) -> ActorRef {
        ActorRef {
            runtime: runtime as *const _,
            address,
        }
    }

    pub fn dispatch(&self, message: ObjectRef) {
        unsafe {
            (&*self.runtime).enqueue(self.address, message);
        }
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

impl fmt::Display for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "actor")
    }
}

pub type InitFn = extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void);
pub type RecvFn = extern "C" fn(*const Runtime, *const ObjectRef, *mut libc::c_void, ObjectRef);

pub struct Actor {
    runtime: *const Runtime,
    state_ptr: *mut libc::c_void,
    recv_fn: RecvFn,
    self_: ObjectRef,
}

impl Actor {
    pub fn new(
        runtime: &Runtime,
        address: ActorAddress,
        state_size: usize,
        init_fn: InitFn,
        recv_fn: RecvFn,
    ) -> Actor {
        unsafe {
            let actor = Actor {
                runtime,
                state_ptr: libc::malloc(state_size),
                recv_fn,
                self_: ObjectRef::new(Object::Actor(ActorRef { runtime, address })),
            };
            init_fn(runtime, &actor.self_, actor.state_ptr);
            actor
        }
    }

    pub fn receive(&mut self, message: ObjectRef) {
        (self.recv_fn)(self.runtime, &self.self_, self.state_ptr, message);
    }

    pub fn reference_to(&self) -> ObjectRef {
        self.self_.clone()
    }
}

impl Drop for Actor {
    fn drop(&mut self) {
        unsafe {
            libc::free(self.state_ptr);
        }
    }
}

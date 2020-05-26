use crate::{Reply, UserLandExposable, Value};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

pub type Recv = unsafe extern "C" fn(*mut c_void, *const Value) -> *const Value;

#[derive(Debug)]
pub struct Object {
    state: Mutex<*mut c_void>,
    recv: Recv,
}

unsafe impl Send for Object {}
unsafe impl Sync for Object {}

impl Object {
    pub fn new(size: usize, recv: Recv) -> Object {
        unsafe {
            Object {
                state: Mutex::new(libc::malloc(size)),
                recv,
            }
        }
    }

    pub fn accept_message(&self, message: Arc<Value>) -> Reply {
        match self.state.try_lock() {
            Err(_) => Reply::Pending,
            Ok(guard) => unsafe {
                /*
                if libc::rand() % 10 < 2 {
                    eprintln!("Controlled panic in object!");

                    return Reply::Panic;
                }
                */
                let value = (self.recv)(*guard, message.expose());

                if value == 0 as *const _ {
                    Reply::Rejected
                } else {
                    Reply::Answer(value.enclose())
                }
            },
        }
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            libc::free(*self.state.lock().unwrap());
        }
    }
}

#![feature(arbitrary_self_types)]

#[macro_use]
extern crate lazy_static;

mod job;
mod pending_reply;
mod semaphore;
mod user_land_exposable;
mod value;

use crate::pending_reply::*;
use crate::semaphore::*;
use crate::user_land_exposable::*;
use crate::value::*;

#[no_mangle]
pub extern "C" fn new_object(size: usize, recv: extern "C" fn()) -> *const Value {
    Value::new_object(size, recv).expose()
}

#[no_mangle]
pub extern "C" fn new_int(value: i128) -> *const Value {
    Value::new_int(value).expose()
}

#[no_mangle]
pub extern "C" fn new_float(value: f64) -> *const Value {
    Value::new_float(value).expose()
}

#[no_mangle]
pub unsafe extern "C" fn new_nullary(value: *mut u8) -> *const Value {
    let len = libc::strlen(value as *mut _);
    let value = std::str::from_utf8(std::slice::from_raw_parts(value, len as usize)).unwrap();
    Value::new_nullary(value).expose()
}

#[no_mangle]
pub unsafe extern "C" fn clone_reference(value: *const Value) {
    let a = value.enclose();
    let b = a.clone();

    a.expose();
    b.expose();
}

#[no_mangle]
pub unsafe extern "C" fn drop_reference(value: *const Value) {
    value.enclose();
}

#[no_mangle]
pub unsafe extern "C" fn send_message(
    receiver: *const Value,
    message: *const Value,
) -> *const PendingReply {
    let receiver = receiver.enclose();
    let message = message.enclose();

    receiver.schedule_message(message).expose()
}

#[no_mangle]
pub unsafe extern "C" fn poll_reply(pending_reply: *const PendingReply) -> *const Value {
    match (*pending_reply).poll() {
        Some(value) => {
            pending_reply.enclose();
            value.expose()
        }
        None => 0 as *const Value,
    }
}

#[no_mangle]
pub extern "C" fn print(val: *const Value) {
    unsafe {
        println!("{}", &*val);
    }
}

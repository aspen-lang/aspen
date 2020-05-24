#![no_std]
#![feature(arbitrary_self_types, lang_items)]
#![allow(unused_unsafe)]

use crate::worker::{Job, Semaphore};
use core::ffi::c_void;
use core::mem::size_of;

#[allow(unused_unsafe)]
macro_rules! print {
    ($format:expr $(, $args:expr)*) => {
        unsafe {
            let bytes = concat!($format, "\0").as_bytes();
            libc::printf(bytes as *const _ as *const i8 $(, $args)*);
        }
    }
}

macro_rules! println {
    ($format:expr $(, $args:expr)*) => {
        print!(concat!($format, "\n") $(, $args)*)
    }
}

mod standalone;
mod threads;
mod worker;

#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ValueTag {
    ObjectRef = 0xf0,
    Integer = 0xf1,
    Float = 0xf2,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Reply {
    x: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Value {
    tag: ValueTag,
    ref_count: *mut usize,
}

impl Value {
    #[inline]
    pub fn int_value(&self) -> i128 {
        debug_assert!(
            self.tag == ValueTag::Integer,
            "Trying to get value of {:x} as Integer",
            self.tag as u32
        );
        unsafe { (*(self as *const _ as *const Integer)).value }
    }

    #[inline]
    pub fn float_value(&self) -> f64 {
        debug_assert!(
            self.tag == ValueTag::Float,
            "Trying to get value of {:x} as Float",
            self.tag as u32
        );
        unsafe { (*(self as *const _ as *const Float)).value }
    }

    #[inline]
    pub fn process_message(&mut self, message: &Value) -> &Value {
        match self.tag {
            ValueTag::ObjectRef => {
                // TODO: Actual message sends!
                print(self);
                print!("-> ");
                print(message);
                self.add_reference();
                self
            }
            ValueTag::Integer => {
                match message.tag {
                    ValueTag::Integer => {
                        let product = self.int_value() * message.int_value();
                        return new_int(product);
                    }
                    _ => {}
                }

                print(self);
                print!("-> ");
                print(message);
                self.add_reference();
                self
            }
            ValueTag::Float => {
                print(self);
                print!("-> ");
                print(message);
                self.add_reference();
                self
            }
        }
    }

    pub fn add_reference(&mut self) {
        // TODO: Make this reference counter atomic

        unsafe {
            *self.ref_count += 1;
        }
    }

    pub fn drop_reference(&mut self) {
        unsafe {
            // TODO: Make this reference counter atomic

            let ref_count = &mut *self.ref_count;

            *ref_count -= 1;
            if *ref_count == 0 {
                if self.tag == ValueTag::ObjectRef {
                    libc::free((*(self as *mut _ as *mut ObjectRef)).ptr as *mut c_void);
                }

                libc::free(ref_count as *mut _ as *mut c_void);
                libc::free(self as *mut _ as *mut c_void);
            }
        }
    }
}

#[repr(C)]
pub struct Object {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ObjectRef {
    tag: ValueTag,
    ref_count: *mut usize,
    ptr: *const Object,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Integer {
    tag: ValueTag,
    ref_count: *mut usize,
    value: i128,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Float {
    tag: ValueTag,
    ref_count: *mut usize,
    value: f64,
}

#[no_mangle]
pub extern "C" fn print(val: &Value) {
    match val.tag {
        ValueTag::ObjectRef => println!("Object!"),
        ValueTag::Integer => println!("%lld", val.int_value()),
        ValueTag::Float => println!("%.15f", val.float_value()),
    };
}

#[no_mangle]
pub unsafe extern "C" fn drop_reference(val: *mut Value) {
    (&mut *val).drop_reference();
}

static mut MASTER_SEM: Option<Semaphore> = None;

#[no_mangle]
pub unsafe extern "C" fn send_message(receiver: *mut Value, message: *const Value) -> *mut Reply {
    let _receiver = &mut *receiver;
    let _message = &*message;

    if MASTER_SEM.is_none() {
        threads::spawn_threads_once();
        unsafe {
            MASTER_SEM = Some(Semaphore::new());
        }
    }

    Job::enqueue(Job::new());

    unsafe {
        MASTER_SEM.as_mut().unwrap().notify();
    }

    libc::malloc(size_of::<Reply>()) as *mut Reply

    // receiver.process_message(message) as *const _
}

#[no_mangle]
pub extern "C" fn new_object() -> &'static Value {
    unsafe {
        let ptr = libc::malloc(core::mem::size_of::<ObjectRef>()) as *mut ObjectRef;
        let object_ref = &mut *ptr;

        object_ref.tag = ValueTag::ObjectRef;
        object_ref.ref_count = libc::malloc(core::mem::size_of::<usize>()) as *mut usize;
        *object_ref.ref_count = 1;

        &*(ptr as *const Value)
    }
}

#[no_mangle]
pub extern "C" fn new_int(value: i128) -> &'static Value {
    unsafe {
        let ptr = libc::malloc(core::mem::size_of::<Integer>()) as *mut Integer;
        let integer = &mut *ptr;

        integer.tag = ValueTag::Integer;
        integer.ref_count = libc::malloc(core::mem::size_of::<usize>()) as *mut usize;
        *integer.ref_count = 1;
        integer.value = value;

        &*(ptr as *const Value)
    }
}

#[no_mangle]
pub extern "C" fn new_float(value: f64) -> &'static Value {
    unsafe {
        let ptr = libc::malloc(core::mem::size_of::<Float>()) as *mut Float;
        let float = &mut *ptr;

        float.tag = ValueTag::Float;
        float.ref_count = libc::malloc(core::mem::size_of::<usize>()) as *mut usize;
        *float.ref_count = 1;
        float.value = value;

        &*(ptr as *const Value)
    }
}

#[no_mangle]
pub extern "C" fn clone_reference(value: &mut Value) {
    value.add_reference();
}

#[no_mangle]
pub extern "C" fn poll_reply(_reply: &mut Reply) {
    loop {}
}

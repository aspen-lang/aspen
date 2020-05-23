#![no_std]
#![feature(arbitrary_self_types, lang_items)]

use core::ffi::c_void;

mod standalone;

extern "C" {
    fn printf(format: *const u8, ...) -> i32;
    fn free(ptr: *mut c_void);
    fn malloc(size: usize) -> *mut c_void;
}

macro_rules! print {
    ($format:expr $(, $args:expr)*) => {unsafe {
        let bytes = concat!($format, "\0").as_bytes();
        printf(bytes as *const _ as *const u8, $($args)*);
    }}
}

macro_rules! println {
    ($format:expr $(, $args:expr)*) => {
        print!(concat!($format, "\n") $(, $args)*)
    }
}

#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ValueTag {
    ObjectRef = 0xf0,
    Integer = 0xf1,
    Float = 0xf2,
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
                        return Value::new_int(product);
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

    pub fn new_int(value: i128) -> &'static Value {
        unsafe {
            let ptr = malloc(core::mem::size_of::<Integer>()) as *mut Integer;
            let integer = &mut *ptr;

            integer.tag = ValueTag::Integer;
            integer.ref_count = malloc(core::mem::size_of::<usize>()) as *mut usize;
            *integer.ref_count = 1;
            integer.value = value;

            &*(ptr as *const Value)
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
                    free((*(self as *mut _ as *mut ObjectRef)).ptr as *mut c_void);
                }

                free(ref_count as *mut _ as *mut c_void);
                free(self as *mut _ as *mut c_void);
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

#[no_mangle]
pub unsafe extern "C" fn send_message(receiver: *mut Value, message: *const Value) -> *const Value {
    let receiver = &mut *receiver;
    let message = &*message;

    receiver.process_message(message) as *const _
}

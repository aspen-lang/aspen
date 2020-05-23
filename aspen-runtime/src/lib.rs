#![no_std]
#![feature(arbitrary_self_types, lang_items)]

use core::ffi::c_void;

mod standalone;

extern "C" {
    fn printf(format: *const u8, ...) -> i32;
    fn free(ptr: *const c_void);
}

macro_rules! print {
    ($format:expr $(, $args:expr)*) => {{
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
    pub unsafe fn int_value(self: *const Self) -> i128 {
        debug_assert!(
            (*self).tag == ValueTag::Integer,
            "Trying to get value of {:x} as Integer",
            (*self).tag as u32
        );
        (*(self as *const Integer)).value
    }

    #[inline]
    pub unsafe fn float_value(self: *const Self) -> f64 {
        debug_assert!(
            (*self).tag == ValueTag::Float,
            "Trying to get value of {:x} as Float",
            (*self).tag as u32
        );
        (*(self as *const Float)).value
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
pub unsafe extern "C" fn print(val: *const Value) {
    match (*val).tag {
        ValueTag::ObjectRef => println!("Object!"),
        ValueTag::Integer => println!("%lld", val.int_value()),
        ValueTag::Float => println!("%.15f", val.float_value()),
    };
}

#[no_mangle]
pub unsafe extern "C" fn drop_reference(val: *mut Value) {
    // TODO: Make this reference counter atomic

    let ref_count = &mut *(*val).ref_count;

    *ref_count -= 1;
    if *ref_count == 0 {
        if (*val).tag == ValueTag::ObjectRef {
            free((*(val as *const ObjectRef)).ptr as *const c_void);
        }

        free(ref_count as *const _ as *const c_void);
        free(val as *const c_void);
    }
}

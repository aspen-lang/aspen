#![no_std]
#![feature(arbitrary_self_types)]
#![cfg_attr(feature = "standalone", feature(lang_items))]
#![cfg(feature = "standalone")]
mod standalone;

extern "C" {
    pub fn printf(format: *const u8, ...) -> i32;
}

#[repr(u32)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ValueTag {
    Integer = 0xf1,
    Float = 0xf2,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Value {
    tag: ValueTag,
}

impl Value {
    #[inline]
    pub unsafe fn int_value(self: *const Self) -> i128 {
        if cfg!(debug_assertions) {
            assert!(
                (*self).tag == ValueTag::Integer,
                "Trying to get value of {:?} as Integer",
                (*self).tag
            );
        }
        (*(self as *const Integer)).value
    }

    #[inline]
    pub unsafe fn float_value(self: *const Self) -> f64 {
        if cfg!(debug_assertions) {
            assert!(
                (*self).tag == ValueTag::Float,
                "Trying to get value of {:?} as Float",
                (*self).tag
            );
        }
        (*(self as *const Float)).value
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Integer {
    tag: ValueTag,
    value: i128,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Float {
    tag: ValueTag,
    value: f64,
}

#[no_mangle]
pub unsafe extern "C" fn print(val: *const Value) {
    match (*val).tag {
        ValueTag::Integer => printf(b"%lld\n\0" as *const u8, val.int_value()),
        ValueTag::Float => printf(b"%f\n\0" as *const u8, val.float_value()),
    };
}

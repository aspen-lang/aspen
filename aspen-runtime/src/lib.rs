#![no_std]
#![cfg_attr(feature = "standalone", feature(lang_items))]
#![cfg(feature = "standalone")]
mod standalone;

extern "C" {
    pub fn printf(format: *const u8, ...) -> i32;
}

#[no_mangle]
pub unsafe extern "C" fn print(s: *const u8) {
    printf(b"%s\n" as *const u8, s);
}

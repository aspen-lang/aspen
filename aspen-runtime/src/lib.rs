#![feature(lang_items)]
#![no_std]

#[panic_handler]
pub fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

extern "C" {
    pub fn printf(format: *const u8, ...) -> i32;
}

#[no_mangle]
pub unsafe extern "C" fn print(s: *const u8) {
    printf(b"%s\n" as *const u8, s);
}

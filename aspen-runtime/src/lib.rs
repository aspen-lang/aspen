use std::mem::size_of;
use std::os::raw::c_char;

type NativeStr = *mut c_char;

unsafe fn extern_str(mut s: NativeStr) -> &'static str {
    let ptr = s as *const u8;
    let mut length = 0;
    while *s != 0 {
        length += 1;
        s = (s as usize + size_of::<c_char>()) as *mut c_char;
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, length)).unwrap()
}

#[no_mangle]
pub unsafe extern "C" fn print(s: NativeStr) {
    println!("{}", extern_str(s));
}

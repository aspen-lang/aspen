#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), feature(lang_items))]
#![cfg_attr(not(test), feature(alloc_error_handler))]

extern crate alloc;

#[macro_use]
mod print;

#[cfg(not(test))]
mod panic {
    use core::alloc::Layout;
    use core::panic::PanicInfo;

    #[global_allocator]
    pub static ALLOCATOR: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

    #[panic_handler]
    pub fn panic(info: &PanicInfo) -> ! {
        println!("{}", info);
        unsafe {
            libc::exit(1);
        }
    }

    #[lang = "eh_personality"]
    fn eh_personality() {}

    #[alloc_error_handler]
    fn oom(_: Layout) -> ! {
        println!("Out of memory!");
        unsafe {
            libc::exit(1);
        }
    }
}

mod object;
use self::object::*;

mod cpus;

mod mutex;
use self::mutex::*;

mod semaphore;
use self::semaphore::*;

mod object_ref;
use self::object_ref::*;

mod runtime;
use self::runtime::*;

mod actor;
use self::actor::*;

use alloc::boxed::Box;

#[no_mangle]
pub unsafe extern "C" fn AspenNewRuntime() -> *mut Runtime {
    let mut rt = Runtime::new();
    for _ in 1..cpus::count() {
        rt.spawn_worker();
    }
    Box::into_raw(rt)
}

#[no_mangle]
pub unsafe extern "C" fn AspenStartRuntime(f: extern "C" fn(*const Runtime)) {
    let mut rt = Runtime::new();
    for _ in 1..cpus::count() {
        rt.spawn_worker();
    }
    let rt = Box::into_raw(rt);
    f(rt);
    let mut rt = Box::from_raw(rt);
    rt.work();
}

#[no_mangle]
pub unsafe extern "C" fn AspenDropRuntime(rt: *mut Runtime) {
    Box::from_raw(rt);
}

#[no_mangle]
pub extern "C" fn AspenNewActor(
    rt: &Runtime,
    state_size: usize,
    init_fn: InitFn,
    recv_fn: RecvFn,
) -> ObjectRef {
    rt.spawn(state_size, init_fn, recv_fn)
}

#[no_mangle]
pub extern "C" fn AspenSend(receiver: &ObjectRef, message: ObjectRef) {
    receiver.send(message);
}

#[no_mangle]
pub extern "C" fn AspenNewInt(value: i128) -> ObjectRef {
    ObjectRef::new(Object::Int(value))
}

#[no_mangle]
pub extern "C" fn AspenNewFloat(value: f64) -> ObjectRef {
    ObjectRef::new(Object::Float(value))
}

#[no_mangle]
pub unsafe extern "C" fn AspenNewAtom(value: *mut libc::c_char) -> ObjectRef {
    let len = libc::strlen(value) as usize;
    ObjectRef::new(Object::Atom(core::str::from_utf8_unchecked(
        core::slice::from_raw_parts(value as *mut _, len),
    )))
}

#[no_mangle]
pub extern "C" fn AspenDrop(object: ObjectRef) {
    drop(object);
}

#[no_mangle]
pub extern "C" fn AspenClone(object: &ObjectRef) -> ObjectRef {
    object.clone()
}

#[no_mangle]
pub extern "C" fn AspenPrint(object: &ObjectRef) {
    println!("{}", object);
}

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

mod worker;
use self::worker::*;

mod semaphore;
use self::semaphore::*;

mod object_ref;
use self::object_ref::*;

mod runtime;
use self::runtime::*;

mod scheduler;
use self::scheduler::*;

mod actor_address;
use self::actor_address::*;

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
    (&mut *rt).attach_current_thread_as_worker();
}

#[no_mangle]
pub unsafe extern "C" fn AspenExit(rt: *const Runtime) {
    Box::from_raw(rt as *mut Runtime);
}

#[no_mangle]
pub extern "C" fn AspenNewActor(
    rt: &Runtime,
    state_size: usize,
    init_msg: ObjectRef,
    init_fn: InitFn,
    recv_fn: RecvFn,
    drop_fn: DropFn,
) -> ObjectRef {
    rt.spawn(state_size, init_msg, init_fn, recv_fn, drop_fn)
}

#[no_mangle]
pub extern "C" fn AspenNewStatelessActor(rt: &Runtime, recv_fn: RecvFn) -> ObjectRef {
    AspenNewActor(rt, 0, rt.noop_object.clone(), noop_init, recv_fn, noop_drop)
}

extern "C" fn noop_init(
    _rt: *const Runtime,
    _self: *const ObjectRef,
    _state: *mut libc::c_void,
    _msg: ObjectRef,
) {
}
extern "C" fn noop_drop(_rt: *const Runtime, _state: *mut libc::c_void) {}

#[no_mangle]
pub extern "C" fn AspenTell(receiver: &ObjectRef, message: ObjectRef) {
    receiver.tell(message);
}

#[no_mangle]
pub extern "C" fn AspenAsk(receiver: &ObjectRef, reply_to: ObjectRef, message: ObjectRef) {
    receiver.ask(message, reply_to);
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

#[no_mangle]
pub extern "C" fn AspenEqInt(value: i128) -> *mut Matcher {
    Box::into_raw(Box::new(Matcher::Equal(Object::Int(value))))
}

#[no_mangle]
pub extern "C" fn AspenMatch(matcher: &Matcher, subject: &ObjectRef) -> bool {
    subject.matches(matcher)
}

#[no_mangle]
pub unsafe extern "C" fn AspenDropMatcher(matcher: *mut Matcher) {
    Box::from_raw(matcher);
}

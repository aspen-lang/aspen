use crate::worker::{Semaphore, Worker};
use core::ffi::c_void;
use core::mem::size_of;

pub fn spawn_threads_once() {
    unsafe {
        static mut DONE: bool = false;

        if !DONE {
            for i in 0..cpus::count() {
                spawn_thread(Worker::new(i + 1))
            }
        }
    }
}

fn spawn_thread(worker: Worker) {
    unsafe {
        let worker_ptr = libc::malloc(size_of::<Worker>()) as *mut Worker;
        *worker_ptr = worker;

        let thread = libc::malloc(size_of::<usize>()) as *mut usize;
        libc::pthread_create(thread, 0 as *mut _, start_thread, worker_ptr as *mut c_void);
    }
}

extern "C" fn start_thread(worker_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        let worker = &mut *(worker_ptr as *mut Worker);
        let semaphore = Semaphore::new();
        loop {
            semaphore.wait();
            worker.work();
        }
    }
}

mod cpus {
    use libc;

    #[cfg(target_os = "macos")]
    pub fn count() -> usize {
        let cpus = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        if cpus < 1 {
            1
        } else {
            cpus as usize
        }
    }
}

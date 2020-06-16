use crate::Runtime;

pub struct Worker {
    thread: libc::pthread_t,
}

impl Worker {
    pub fn new(runtime: *mut Runtime) -> Worker {
        unsafe {
            let mut thread = core::mem::zeroed();
            extern "C" fn work(runtime: *mut libc::c_void) -> *mut libc::c_void {
                let runtime = unsafe { &*(runtime as *mut Runtime) };
                runtime.work();
                0 as *mut _
            }
            libc::pthread_create(&mut thread, 0 as *mut _, work, runtime as *mut _);
            Worker { thread }
        }
    }

    pub fn from_current_thread() -> Worker {
        Worker {
            thread: unsafe { libc::pthread_self() },
        }
    }

    pub fn join(&self) {
        unsafe {
            libc::pthread_join(self.thread, 0 as *mut _);
        }
    }
}

#[cfg(target_os = "linux")]
pub struct Semaphore {
    sem: libc::sem_t,
}

#[cfg(target_os = "linux")]
impl Semaphore {
    pub fn new() -> Semaphore {
        unsafe {
            let mut sem = core::mem::zeroed();
            libc::sem_init(&mut sem, 0, 0);
            Semaphore { sem }
        }
    }

    pub fn wait(&self) {
        unsafe {
            libc::sem_wait(&self.sem as *const libc::sem_t as *mut _);
        }
    }

    pub fn notify(&self) {
        unsafe {
            libc::sem_post(&self.sem as *const libc::sem_t as *mut _);
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for Semaphore {
    fn drop(&mut self) {
        unsafe {
            libc::sem_close(&mut self.sem);
        }
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    fn dispatch_semaphore_create(value: libc::c_long) -> *mut dispatch_semaphore_t;
    fn dispatch_semaphore_signal(dsema: *mut dispatch_semaphore_t) -> libc::c_long;
    fn dispatch_semaphore_wait(
        dsema: *mut dispatch_semaphore_t,
        timeout: dispatch_time_t,
    ) -> libc::c_long;
    fn dispatch_release(object: *mut dispatch_semaphore_t);
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct dispatch_semaphore_t {
    _private: [u8; 0],
}

#[cfg(target_os = "macos")]
#[allow(non_camel_case_types)]
type dispatch_time_t = u64;

#[cfg(target_os = "macos")]
const DISPATCH_TIME_FOREVER: dispatch_time_t = !0;

#[cfg(target_os = "macos")]
pub struct Semaphore {
    sem: *mut dispatch_semaphore_t,
}

#[cfg(target_os = "macos")]
impl Semaphore {
    pub fn new() -> Semaphore {
        Semaphore {
            sem: unsafe { dispatch_semaphore_create(0) },
        }
    }

    pub fn wait(&self) {
        unsafe {
            dispatch_semaphore_wait(self.sem, DISPATCH_TIME_FOREVER);
        }
    }

    pub fn notify(&self) {
        unsafe {
            dispatch_semaphore_signal(self.sem);
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for Semaphore {
    fn drop(&mut self) {
        unsafe {
            dispatch_release(self.sem);
        }
    }
}

pub struct Semaphore {
    ptr: *mut libc::sem_t,
}

unsafe impl Send for Semaphore {}
unsafe impl Sync for Semaphore {}

impl Semaphore {
    pub fn new(mut id: String) -> Semaphore {
        unsafe {
            id.push('\0');
            Semaphore {
                ptr: libc::sem_open(id.as_ptr() as *mut _, libc::O_CREAT, 0o644, 0),
            }
        }
    }

    pub fn notify(&self) {
        unsafe {
            libc::sem_post(self.ptr);
        }
    }

    pub fn wait(&self) {
        unsafe {
            libc::sem_wait(self.ptr);
        }
    }
}

impl Drop for Semaphore {
    fn drop(&mut self) {
        unsafe {
            libc::sem_close(self.ptr);
        }
    }
}

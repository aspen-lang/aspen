mod q64;
use self::q64::Q64;

static QUEUE: Q64<Job> = Q64::new();

pub struct Worker {
    pub number: usize,
}

impl Worker {
    pub fn new(number: usize) -> Worker {
        Worker { number }
    }

    #[inline]
    pub fn work(&mut self) {
        if let Some(job) = QUEUE.dequeue() {
            println!("Worker %d doing job %d", self.number, job.name);
        }
    }
}

pub struct Semaphore {
    inner: *mut libc::sem_t,
}

impl Semaphore {
    pub fn new() -> Semaphore {
        Semaphore {
            inner: unsafe {
                libc::sem_open(
                    b"/aspenruntime\0" as *const _ as *const i8,
                    libc::O_CREAT,
                    0o644,
                    0,
                )
            },
        }
    }

    #[inline]
    pub fn wait(&self) {
        unsafe {
            libc::sem_wait(self.inner);
        }
    }

    #[inline]
    pub fn notify(&self) {
        unsafe {
            libc::sem_post(self.inner);
        }
    }
}

unsafe impl core::marker::Sync for Semaphore {}

impl Drop for Semaphore {
    fn drop(&mut self) {
        unsafe {
            libc::sem_close(self.inner);
        }
    }
}

pub struct Job {
    name: usize,
}

impl Job {
    pub fn enqueue(mut job: Job) {
        while let Err(j) = QUEUE.enqueue(job) {
            unsafe {
                libc::sched_yield();
            }
            job = j;
        }
    }

    pub fn new() -> Job {
        unsafe {
            Job {
                name: libc::rand() as usize,
            }
        }
    }
}

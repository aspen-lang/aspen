use crate::user_land_exposable::UserLandExposable;
use crate::Value;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use threadpool::ThreadPool;

pub struct Reply {
    slot: Slot,
}

impl Reply {
    pub fn new(receiver: Arc<Value>, message: Arc<Value>) -> Arc<Reply> {
        let (a, b) = Slot::new();

        Self::schedule(receiver, message, a);

        Arc::new(Reply { slot: b })
    }

    fn schedule(receiver: Arc<Value>, message: Arc<Value>, slot: Slot) {
        lazy_static! {
            static ref POOL: Arc<Mutex<ThreadPool>> =
                Arc::new(Mutex::new(ThreadPool::new(num_cpus::get())));
        }

        POOL.lock()
            .unwrap()
            .execute(move || match receiver.accept_message(&message) {
                Err(_) => Self::schedule(receiver, message, slot),
                Ok(value) => slot.fill(value),
            });
    }

    pub fn poll(&self) -> Option<Arc<Value>> {
        unsafe {
            let ptr_or_null = *self.slot.ptr;

            const NULL: *const Value = 0 as *const Value;

            match ptr_or_null {
                NULL => None,
                value => {
                    *self.slot.ptr = NULL;
                    Some(value.enclose())
                }
            }
        }
    }
}

pub struct Slot {
    is_cloned: Arc<AtomicBool>,
    ptr: *mut *const Value,
}

unsafe impl Send for Slot {}

impl Slot {
    pub fn new() -> (Slot, Slot) {
        let ptr = unsafe { libc::malloc(size_of::<*const Value>()) } as *mut *const Value;
        let is_cloned = Arc::new(AtomicBool::new(true));

        (
            Slot {
                is_cloned: is_cloned.clone(),
                ptr,
            },
            Slot { is_cloned, ptr },
        )
    }

    pub fn fill(&self, value: Arc<Value>) {
        unsafe {
            *self.ptr = value.expose();
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if !self.is_cloned.fetch_and(false, Ordering::SeqCst) {
            unsafe {
                libc::free(self.ptr as *mut _);
            }
        }
    }
}

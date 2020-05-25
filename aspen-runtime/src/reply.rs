use crate::job::JobQueue;
use crate::Value;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

pub struct Reply {
    slot: Slot<Arc<Value>>,
}

impl Reply {
    pub fn new(receiver: Arc<Value>, message: Arc<Value>) -> Arc<Reply> {
        let slot = Slot::new();

        Self::schedule(receiver, message, slot.clone());

        Arc::new(Reply { slot })
    }

    fn schedule(receiver: Arc<Value>, message: Arc<Value>, slot: Slot<Arc<Value>>) {
        lazy_static! {
            static ref QUEUE: JobQueue<(Arc<Value>, Arc<Value>, Slot<Arc<Value>>)> =
                Reply::job_queue();
        }
        QUEUE.schedule((receiver, message, slot));
    }

    fn job_queue() -> JobQueue<(Arc<Value>, Arc<Value>, Slot<Arc<Value>>)> {
        JobQueue::new(
            "/jobs".into(),
            100,
            |queue: &'static JobQueue<(Arc<Value>, Arc<Value>, Slot<Arc<Value>>)>,
             (receiver, message, slot)| {
                match receiver.accept_message(&message) {
                    Err(_) => queue.schedule((receiver, message, slot)),
                    Ok(value) => slot.fill(value),
                }
            },
        )
    }

    pub fn poll(&self) -> Option<Arc<Value>> {
        self.slot.poll()
    }
}

#[derive(Clone)]
struct Slot<T> {
    mutex: Arc<Mutex<Option<T>>>,
}

impl<T> Slot<T> {
    pub fn new() -> Slot<T> {
        Slot {
            mutex: Arc::new(Mutex::new(None)),
        }
    }

    pub fn fill(&self, value: T) {
        let mut guard = self.mutex.lock().unwrap();
        let opt: &mut Option<T> = guard.deref_mut();

        *opt = Some(value);
    }

    pub fn poll(&self) -> Option<T> {
        match self.mutex.try_lock() {
            Err(_) => None,
            Ok(mut guard) => {
                let opt = guard.deref_mut();

                if opt.is_none() {
                    None
                } else {
                    Some(std::mem::replace(opt, None).unwrap())
                }
            }
        }
    }
}

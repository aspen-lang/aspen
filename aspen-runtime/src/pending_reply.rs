use crate::job::JobQueue;
use crate::Reply;
use crate::Value;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

pub struct PendingReply {
    slot: Slot<Reply>,
}

impl PendingReply {
    pub fn new(receiver: Arc<Value>, message: Arc<Value>) -> Arc<PendingReply> {
        let slot = Slot::new();

        Self::schedule(receiver, message, slot.clone());

        Arc::new(PendingReply { slot })
    }

    fn schedule(receiver: Arc<Value>, message: Arc<Value>, slot: Slot<Reply>) {
        lazy_static! {
            static ref QUEUE: JobQueue<(Arc<Value>, Arc<Value>, Slot<Reply>)> =
                PendingReply::job_queue();
        }
        QUEUE.schedule((receiver, message, slot));
    }

    fn job_queue() -> JobQueue<(Arc<Value>, Arc<Value>, Slot<Reply>)> {
        JobQueue::new(
            "/jobs".into(),
            100,
            |queue: &'static JobQueue<(Arc<Value>, Arc<Value>, Slot<Reply>)>,
             (receiver, message, slot)| {
                match receiver.accept_message(&message) {
                    Reply::Pending => queue.schedule((receiver, message, slot)),
                    r => slot.fill(r),
                }
            },
        )
    }

    pub fn poll(&self) -> Reply {
        self.slot.poll().unwrap_or(Reply::Pending)
    }
}

struct Slot<T> {
    mutex: Arc<Mutex<Option<T>>>,
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Slot {
            mutex: self.mutex.clone(),
        }
    }
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

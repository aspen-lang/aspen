use crate::{Actor, ActorAddress, Mutex, Semaphore};
use alloc::collections::BTreeSet;
use crossbeam_queue::SegQueue;

pub struct Scheduler {
    semaphore: Semaphore,
    idle_actors: SegQueue<Actor>,
    deleted_actors: Mutex<BTreeSet<ActorAddress>>,
}

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler {
            semaphore: Semaphore::new(),
            idle_actors: SegQueue::new(),
            deleted_actors: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn add_actor(&self, actor: Actor) {
        self.idle_actors.push(actor);
    }

    #[inline]
    pub fn notify(&self) {
        self.semaphore.notify();
    }

    pub fn work(&self) -> bool {
        self.semaphore.wait();
        loop {
            if let Ok(mut actor) = self.idle_actors.pop() {
                {
                    if self.deleted_actors.lock().remove(&actor.address) {
                        break;
                    }
                }

                let received = actor.receive();
                self.idle_actors.push(actor);
                if received {
                    break;
                }
            }
        }
        true
    }

    pub fn delete(&self, address: ActorAddress) {
        let mut da = self.deleted_actors.lock();
        da.insert(address);
    }
}

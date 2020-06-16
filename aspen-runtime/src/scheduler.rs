use crate::{Actor, ActorAddress, Mutex, Semaphore};
// use alloc::collections::BTreeSet as Set;
use hashbrown::HashSet as Set;
use crossbeam_queue::SegQueue;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct Scheduler {
    semaphore: Semaphore,
    idle_actors: SegQueue<Actor>,
    deleted_actors: Mutex<Set<ActorAddress>>,
    actors_count: AtomicUsize,
}

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler {
            semaphore: Semaphore::new(),
            idle_actors: SegQueue::new(),
            deleted_actors: Mutex::new(Set::new()),
            actors_count: AtomicUsize::new(0),
        }
    }

    pub fn add_actor(&self, actor: Actor) {
        self.idle_actors.push(actor);
        self.actors_count.fetch_add(1, Ordering::Relaxed);
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
                    let mut deleted = self.deleted_actors.lock();
                    if deleted.remove(&actor.address) {
                        if actor.inbox_is_empty() {
                            if self.actors_count.fetch_sub(1, Ordering::Relaxed) == 1 {
                                println!("Just deleted the last actor");
                            }
                            break;
                        }
                        deleted.insert(actor.address);
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

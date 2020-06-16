use crate::{Actor, ActorAddress, DropFn, InitFn, Object, ObjectRef, RecvFn, Scheduler, Worker};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct Runtime {
    workers: Vec<Worker>,
    scheduler: Scheduler,
    id_gen: AtomicUsize,
    pub noop_object: ObjectRef,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        for worker in self.workers.iter() {
            worker.join();
        }
    }
}

impl Runtime {
    pub fn new() -> Box<Runtime> {
        Box::new(Runtime {
            workers: Vec::new(),
            scheduler: Scheduler::new(),
            id_gen: AtomicUsize::new(1),
            noop_object: ObjectRef::new(Object::Noop),
        })
    }

    pub fn spawn_worker(&mut self) {
        let rt = self as *mut _;
        self.workers.push(Worker::new(rt))
    }

    pub fn attach_current_thread_as_worker(&mut self) {
        let worker = Worker::from_current_thread();
        self.workers.push(worker);
        self.work();
    }

    pub fn work(&self) {
        while self.scheduler.work() {}
    }

    #[inline]
    pub fn notify(&self) {
        self.scheduler.notify();
    }

    pub fn spawn(
        &self,
        state_size: usize,
        init_msg: ObjectRef,
        init_fn: InitFn,
        recv_fn: RecvFn,
        drop_fn: DropFn,
    ) -> ObjectRef {
        let address = self.new_address();
        let (actor_ref, actor) = Actor::new(
            self, address, state_size, init_msg, init_fn, recv_fn, drop_fn,
        );
        self.scheduler.add_actor(actor);
        actor_ref
    }

    fn new_address(&self) -> ActorAddress {
        ActorAddress(self.id_gen.fetch_add(1, Ordering::Relaxed))
    }

    pub fn schedule_deletion(&self, address: ActorAddress) {
        self.scheduler.delete(address);
        self.notify();
    }

    /*

    #[inline]
    pub fn enqueue(
        &self,
        receiver_ref: ObjectRef,
        recipient: ActorAddress,
        message: ObjectRef,
        reply_to: Option<ObjectRef>,
    ) {
        self.queue
            .push((receiver_ref, recipient, message, reply_to));
        self.semaphore.notify();
    }


    pub fn next(&self) -> Option<(Guard<Actor>, ObjectRef, ObjectRef)> {
        loop {
            if self.is_dropping.load(Ordering::Relaxed) {
                return None;
            }
            self.semaphore.wait();
            let (object_ref, address, message, reply_to) = self.queue.pop().ok()?;

            let actor = {
                let actors = self.actors.lock();
                match actors.get(&address) {
                    None => {
                        println!(
                            "Undeliverable message {:?}! {:?} is no longer alive!",
                            message, address
                        );
                        continue;
                    }
                    Some(a) => a.deref() as *const Mutex<Actor>,
                }
            };
            match unsafe { &*actor }.try_lock() {
                None => {
                    self.enqueue(object_ref, address, message, reply_to);
                }
                Some(actor) => {
                    return Some((actor, message, reply_to.unwrap_or(self.noop_object.clone())));
                }
            }
        }
    }
    */
}

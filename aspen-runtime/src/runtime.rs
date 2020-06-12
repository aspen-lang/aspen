use crate::{Actor, ActorAddress, Guard, InitFn, Mutex, ObjectRef, RecvFn, Semaphore};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_queue::SegQueue;
use hashbrown::HashMap;

type MessageQueue = SegQueue<(ActorAddress, ObjectRef)>;

pub struct Runtime {
    actors: Mutex<HashMap<ActorAddress, Pin<Box<Mutex<Actor>>>>>,
    id_gen: AtomicUsize,
    queue: MessageQueue,
    semaphore: Semaphore,
    workers: Vec<Worker>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            self.semaphore.notify();
        }
        for worker in self.workers.iter() {
            worker.join();
        }
    }
}

impl Runtime {
    pub fn new() -> Box<Runtime> {
        Box::new(Runtime {
            actors: Mutex::new(HashMap::new()),
            id_gen: AtomicUsize::new(0),
            queue: MessageQueue::new(),
            semaphore: Semaphore::new(),
            workers: Vec::new(),
        })
    }

    pub fn spawn_worker(&mut self) {
        let rt = self as *mut _;
        self.workers.push(Worker::new(rt))
    }

    pub fn work(&mut self) {
        Worker::work(self);
    }

    fn new_address(&self) -> ActorAddress {
        ActorAddress(self.id_gen.fetch_add(1, Ordering::Relaxed))
    }

    pub fn enqueue(&self, recipient: ActorAddress, message: ObjectRef) {
        self.queue.push((recipient, message));
        self.semaphore.notify();
    }

    pub fn spawn(&self, state_size: usize, init_fn: InitFn, recv_fn: RecvFn) -> ObjectRef {
        let address = self.new_address();
        let actor = Actor::new(self, address, state_size, init_fn, recv_fn);
        let mut map = self.actors.lock();
        let map = map.deref_mut();
        let actor_ref = actor.reference_to();
        map.insert(address, Pin::new(Box::new(Mutex::new(actor))));
        actor_ref
    }

    pub fn next(&self) -> Option<(Guard<Actor>, ObjectRef)> {
        loop {
            self.semaphore.wait();
            let (address, message) = self.queue.pop().ok()?;

            let actors = self.actors.lock();
            if let Some(a) = actors.get(&address) {
                let actor = a.deref() as *const Mutex<Actor>;
                drop(a);
                match unsafe { &*actor }.try_lock() {
                    None => {
                        self.enqueue(address, message);
                    }
                    Some(actor) => {
                        return Some((actor, message));
                    }
                }
            } else {
                println!("Undeliverable message! {:?} is no longer alive!", address);
            }
        }
    }

    pub fn schedule_deletion(&self, address: ActorAddress) {
        self.actors.lock().remove(&address);
    }
}

struct Worker {
    thread: libc::pthread_t,
}

impl Worker {
    pub fn new(runtime: *mut Runtime) -> Worker {
        unsafe {
            let mut thread = core::mem::zeroed();
            extern "C" fn work(runtime: *mut libc::c_void) -> *mut libc::c_void {
                let runtime = unsafe { &*(runtime as *mut Runtime) };
                while let Some((mut receiver, message)) = runtime.next() {
                    receiver.receive(message);
                }
                0 as *mut _
            }
            libc::pthread_create(&mut thread, 0 as *mut _, work, runtime as *mut _);
            Worker { thread }
        }
    }

    pub fn work(runtime: &mut Runtime) {
        let worker = Worker {
            thread: unsafe { libc::pthread_self() },
        };
        runtime.workers.push(worker);
        while let Some((mut receiver, message)) = runtime.next() {
            receiver.receive(message);
        }
    }

    pub fn join(&self) {
        unsafe {
            libc::pthread_join(self.thread, 0 as *mut _);
        }
    }
}

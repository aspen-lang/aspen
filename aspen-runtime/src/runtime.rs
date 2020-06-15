use crate::{Actor, ActorAddress, Guard, InitFn, Mutex, Object, ObjectRef, RecvFn, Semaphore};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crossbeam_queue::SegQueue;
use hashbrown::HashMap;

type MessageQueue = SegQueue<(ObjectRef, ActorAddress, ObjectRef, Option<ObjectRef>)>;

pub struct Runtime {
    actors: Mutex<HashMap<ActorAddress, Pin<Box<Mutex<Actor>>>>>,
    id_gen: AtomicUsize,
    queue: MessageQueue,
    semaphore: Semaphore,
    workers: Vec<Worker>,
    noop_actor: ObjectRef,
    is_dropping: AtomicBool,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if self.is_dropping.swap(true, Ordering::SeqCst) {
            panic!("Double dropping Runtime!");
        }
        for _ in 0..self.workers.len() {
            self.semaphore.notify();
        }
        let mut main_worker = None;
        for worker in self.workers.iter() {
            if worker.is_main {
                main_worker = Some(worker);
            } else {
                worker.join();
            }
        }
        if let Some(main_worker) = main_worker {
            main_worker.join();
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
            noop_actor: ObjectRef::new(Object::Noop),
            is_dropping: AtomicBool::new(false),
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

    pub fn spawn(&self, state_size: usize, init_fn: InitFn, recv_fn: RecvFn) -> ObjectRef {
        let address = self.new_address();
        let (actor_ref, actor) = Actor::new(self, address, state_size, init_fn, recv_fn);
        let mut map = self.actors.lock();
        let map = map.deref_mut();
        map.insert(address, Pin::new(Box::new(Mutex::new(actor))));
        actor_ref
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
                    return Some((actor, message, reply_to.unwrap_or(self.noop_actor.clone())));
                }
            }
        }
    }

    pub fn schedule_deletion(&self, address: ActorAddress) {
        if !self.is_dropping.load(Ordering::SeqCst) {
            let mut actors = self.actors.lock();
            actors.remove(&address);
            if actors.len() == 0 {
                unsafe {
                    crate::AspenExit(self as *const _);
                }
            }
        }
    }
}

struct Worker {
    thread: libc::pthread_t,
    is_main: bool,
}

impl Worker {
    pub fn new(runtime: *mut Runtime) -> Worker {
        unsafe {
            let mut thread = core::mem::zeroed();
            extern "C" fn work(runtime: *mut libc::c_void) -> *mut libc::c_void {
                let runtime = unsafe { &*(runtime as *mut Runtime) };
                while let Some((mut receiver, message, reply_to)) = runtime.next() {
                    receiver.receive(message, reply_to);
                }
                0 as *mut _
            }
            libc::pthread_create(&mut thread, 0 as *mut _, work, runtime as *mut _);
            Worker {
                thread,
                is_main: false,
            }
        }
    }

    pub fn work(runtime: &mut Runtime) {
        let worker = Worker {
            thread: unsafe { libc::pthread_self() },
            is_main: true,
        };
        runtime.workers.push(worker);
        while let Some((mut receiver, message, reply_to)) = runtime.next() {
            receiver.receive(message, reply_to);
        }
    }

    pub fn join(&self) {
        unsafe {
            libc::pthread_join(self.thread, 0 as *mut _);
        }
    }
}

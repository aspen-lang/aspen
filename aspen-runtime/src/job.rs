use crate::Semaphore;
use crossbeam_queue::ArrayQueue;
use std::sync::Once;
use std::thread;

pub struct JobQueue<T: 'static + Send + Sync> {
    queue: ArrayQueue<T>,
    semaphore: Semaphore,
    procedure: Box<dyn Sync + Fn(&'static JobQueue<T>, T)>,
    once: Once,
}

impl<T: 'static + Send + Sync> JobQueue<T> {
    pub fn new<P: 'static + Send + Sync + Fn(&'static Self, T)>(
        name: String,
        size: usize,
        procedure: P,
    ) -> JobQueue<T> {
        JobQueue {
            queue: ArrayQueue::new(size),
            semaphore: Semaphore::new(name),
            procedure: Box::new(procedure),
            once: Once::new(),
        }
    }

    pub fn schedule(&'static self, job: T) {
        self.once.call_once(|| {
            for _ in 0..num_cpus::get() {
                thread::spawn(move || loop {
                    self.semaphore.wait();
                    if let Ok(job) = self.queue.pop() {
                        (self.procedure)(self, job);
                    }
                });
            }
        });

        self.queue.push(job).unwrap();
        self.semaphore.notify();
    }
}

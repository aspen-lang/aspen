use core::cell::UnsafeCell;
use core::fmt;
use core::ops::{Deref, DerefMut};

pub struct Mutex<T> {
    inner: UnsafeCell<T>,
    lock: UnsafeCell<libc::pthread_mutex_t>,
}

impl<T> Mutex<T> {
    pub fn new(inner: T) -> Mutex<T> {
        Mutex {
            inner: UnsafeCell::new(inner),
            lock: UnsafeCell::new(libc::PTHREAD_MUTEX_INITIALIZER),
        }
    }

    pub fn lock(&self) -> Guard<T> {
        unsafe {
            libc::pthread_mutex_lock(&mut *self.lock.get());
        }
        Guard { mutex: self }
    }

    /*
    pub fn try_lock(&self) -> Option<Guard<T>> {
        unsafe {
            match libc::pthread_mutex_trylock(&mut *self.lock.get()) {
                0 => Some(Guard { mutex: self }),
                _ => None,
            }
        }
    }
    */
}

impl<T: fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unsafe { &*self.inner.get() }.fmt(f)
    }
}

impl<T> Drop for Mutex<T> {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_mutex_destroy(&mut *self.lock.get());
        }
    }
}

pub struct Guard<T> {
    mutex: *const Mutex<T>,
}

impl<T> DerefMut for Guard<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *(&*self.mutex).inner.get() }
    }
}

impl<T> Deref for Guard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*(&*self.mutex).inner.get() }
    }
}

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_mutex_unlock((&*self.mutex).lock.get());
        }
    }
}

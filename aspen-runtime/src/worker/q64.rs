// STOLEN FROM `heapless` BECAUSE OF `no_std` PROBLEMS

use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

/// MPMC queue with a capacity for 64 elements
pub struct Q64<T> {
    buffer: UnsafeCell<[Cell<T>; 64]>,
    dequeue_pos: AtomicU8,
    enqueue_pos: AtomicU8,
}

impl<T> Q64<T> {
    const MASK: u8 = 64 - 1;

    /// Creates an empty queue
    pub const fn new() -> Self {
        Self {
            buffer: UnsafeCell::new([
                Cell::new(0),
                Cell::new(1),
                Cell::new(2),
                Cell::new(3),
                Cell::new(4),
                Cell::new(5),
                Cell::new(6),
                Cell::new(7),
                Cell::new(8),
                Cell::new(9),
                Cell::new(10),
                Cell::new(11),
                Cell::new(12),
                Cell::new(13),
                Cell::new(14),
                Cell::new(15),
                Cell::new(16),
                Cell::new(17),
                Cell::new(18),
                Cell::new(19),
                Cell::new(20),
                Cell::new(21),
                Cell::new(22),
                Cell::new(23),
                Cell::new(24),
                Cell::new(25),
                Cell::new(26),
                Cell::new(27),
                Cell::new(28),
                Cell::new(29),
                Cell::new(30),
                Cell::new(31),
                Cell::new(32),
                Cell::new(33),
                Cell::new(34),
                Cell::new(35),
                Cell::new(36),
                Cell::new(37),
                Cell::new(38),
                Cell::new(39),
                Cell::new(40),
                Cell::new(41),
                Cell::new(42),
                Cell::new(43),
                Cell::new(44),
                Cell::new(45),
                Cell::new(46),
                Cell::new(47),
                Cell::new(48),
                Cell::new(49),
                Cell::new(50),
                Cell::new(51),
                Cell::new(52),
                Cell::new(53),
                Cell::new(54),
                Cell::new(55),
                Cell::new(56),
                Cell::new(57),
                Cell::new(58),
                Cell::new(59),
                Cell::new(60),
                Cell::new(61),
                Cell::new(62),
                Cell::new(63),
            ]),
            dequeue_pos: AtomicU8::new(0),
            enqueue_pos: AtomicU8::new(0),
        }
    }

    /// Returns the item in the front of the queue, or `None` if the queue is empty
    pub fn dequeue(&self) -> Option<T> {
        unsafe { dequeue(self.buffer.get() as *mut _, &self.dequeue_pos, Self::MASK) }
    }

    /// Adds an `item` to the end of the queue
    ///
    /// Returns back the `item` if the queue is full
    pub fn enqueue(&self, item: T) -> Result<(), T> {
        unsafe {
            enqueue(
                self.buffer.get() as *mut _,
                &self.enqueue_pos,
                Self::MASK,
                item,
            )
        }
    }
}

unsafe impl<T> Sync for Q64<T> where T: Send {}

struct Cell<T> {
    data: MaybeUninit<T>,
    sequence: AtomicU8,
}

impl<T> Cell<T> {
    const fn new(seq: u8) -> Self {
        Self {
            data: MaybeUninit::uninit(),
            sequence: AtomicU8::new(seq),
        }
    }
}

unsafe fn dequeue<T>(buffer: *mut Cell<T>, dequeue_pos: &AtomicU8, mask: u8) -> Option<T> {
    let mut pos = dequeue_pos.load(Ordering::Relaxed);

    let mut cell;
    loop {
        cell = buffer.add(usize::from(pos & mask));
        let seq = (*cell).sequence.load(Ordering::Acquire);
        let dif = i16::from(seq) - i16::from(pos.wrapping_add(1));

        if dif == 0 {
            if dequeue_pos
                .compare_exchange_weak(
                    pos,
                    pos.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        } else if dif < 0 {
            return None;
        } else {
            if pos == 255 && dif == 255 {
                return None;
            } else {
                pos = dequeue_pos.load(Ordering::Relaxed);
            }
        }
    }

    let data = (*cell).data.as_ptr().read();
    (*cell)
        .sequence
        .store(pos.wrapping_add(mask).wrapping_add(1), Ordering::Release);
    Some(data)
}

unsafe fn enqueue<T>(
    buffer: *mut Cell<T>,
    enqueue_pos: &AtomicU8,
    mask: u8,
    item: T,
) -> Result<(), T> {
    let mut pos = enqueue_pos.load(Ordering::Relaxed);

    let mut cell;
    loop {
        cell = buffer.add(usize::from(pos & mask));
        let seq = (*cell).sequence.load(Ordering::Acquire);
        let dif = i16::from(seq) - i16::from(pos);

        if dif == 0 {
            if enqueue_pos
                .compare_exchange_weak(
                    pos,
                    pos.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        } else if dif < 0 {
            return Err(item);
        } else {
            pos = enqueue_pos.load(Ordering::Relaxed);
        }
    }

    (*cell).data.as_mut_ptr().write(item);
    (*cell)
        .sequence
        .store(pos.wrapping_add(1), Ordering::Release);
    Ok(())
}

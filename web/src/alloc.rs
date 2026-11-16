//! The allocator of the build with threads. The allocator of the standard library for WebAssembly has one global lock,
//! and the threads of rayon wait for it: the decode of the blocks makes millions of small allocations (Sweden at zoom 8:
//! step 2 took 54 s, the command line 7 s).
//!
//! Small blocks (16 to 256 bytes) go through free lists, one for each size. Each thread has its own lists, with no lock.
//! Often one thread makes a block (the decode) and another thread frees it (the writer), so the lists of the threads
//! also give batches of `BATCH` blocks to a shared pool, and take batches from it: one lock for `BATCH` blocks. Freed
//! blocks are used again by any thread, so the memory stays near the blocks in use. A thread keeps at most
//! 2 × `BATCH` blocks of a size, and the pool at most `POOL` bytes of a size; more blocks go back to the allocator of the
//! standard library. (Lists for each thread with no pool and no limit filled the 4 GiB with France at zoom 8; lists for
//! each thread with a limit and no pool made Sweden 3 times slower.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr::null_mut;
use std::sync::Mutex;

const UNIT: usize = 16;
const SIZES: usize = 16;
/// The blocks of one batch between a thread and the pool
const BATCH: usize = 256;
/// The most bytes of free blocks of one size in the pool
const POOL: usize = 64 << 20;

thread_local! {
    /// for each size: the first free block (each free block holds the address of the next one) and the number of blocks
    static LISTS: [Cell<(*mut u8, usize)>; SIZES] = const { [const { Cell::new((null_mut(), 0)) }; SIZES] };
}

/// For each size: the first batch (as an address) and the number of batches. A batch is a list of `BATCH` blocks; the
/// second word of its first block holds the address of the next batch.
static SHARED: [Mutex<(usize, usize)>; SIZES] = [const { Mutex::new((0, 0)) }; SIZES];

pub struct PerThread;

/// The list of a layout, or none for a large or aligned block.
fn list(l: &Layout) -> Option<usize> {
    (l.size() <= UNIT * SIZES && l.align() <= UNIT).then(|| l.size().max(1).div_ceil(UNIT) - 1)
}

/// The layout of the blocks of list `i`: all blocks of a list have one size, so any of them can go back to `System`.
fn layout(i: usize) -> Layout {
    unsafe { Layout::from_size_align_unchecked((i + 1) * UNIT, UNIT) }
}

unsafe fn next(p: *mut u8) -> *mut u8 {
    unsafe { (p as *mut *mut u8).read() }
}

unsafe fn set_next(p: *mut u8, n: *mut u8) {
    unsafe { (p as *mut *mut u8).write(n) }
}

// SAFETY: a small block always comes from `System` with the layout of its list, and it is on one list at a time. A free
// block holds the address of the next block of its list; the first block of a batch also holds the address of the next
// batch in its second word (a block has at least 16 bytes).
unsafe impl GlobalAlloc for PerThread {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let Some(i) = list(&l) else { return unsafe { System.alloc(l) } };
        LISTS.with(|lists| {
            let (mut p, mut n) = lists[i].get();
            if p.is_null() {
                // a batch from the pool, else one block from the system
                let mut pool = SHARED[i].lock().unwrap_or_else(|e| e.into_inner());
                if pool.1 == 0 {
                    drop(pool);
                    return unsafe { System.alloc(layout(i)) };
                }
                p = pool.0 as *mut u8;
                *pool = (unsafe { (p as *mut usize).add(1).read() }, pool.1 - 1);
                n = BATCH;
            }
            lists[i].set((unsafe { next(p) }, n - 1));
            p
        })
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let Some(i) = list(&l) else { return unsafe { System.dealloc(p, l) } };
        LISTS.with(|lists| {
            let (first, n) = lists[i].get();
            unsafe { set_next(p, first) };
            if n + 1 < 2 * BATCH {
                lists[i].set((p, n + 1));
                return;
            }
            // 2 × BATCH blocks: the first BATCH go to the pool as a batch (or to the system when the pool is full)
            let mut last = p;
            for _ in 1..BATCH {
                last = unsafe { next(last) };
            }
            let rest = unsafe { next(last) };
            unsafe { set_next(last, null_mut()) };
            lists[i].set((rest, n + 1 - BATCH));
            let mut pool = SHARED[i].lock().unwrap_or_else(|e| e.into_inner());
            if pool.1 * BATCH * (i + 1) * UNIT < POOL {
                unsafe { (p as *mut usize).add(1).write(pool.0) };
                *pool = (p as usize, pool.1 + 1);
                return;
            }
            drop(pool);
            let mut b = p;
            while !b.is_null() {
                let n = unsafe { next(b) };
                unsafe { System.dealloc(b, layout(i)) };
                b = n;
            }
        })
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let new = unsafe { Layout::from_size_align_unchecked(size, l.align()) };
        if list(&l).is_none() && list(&new).is_none() {
            return unsafe { System.realloc(p, l, size) };
        }
        let q = unsafe { self.alloc(new) };
        if !q.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(p, q, l.size().min(size));
                self.dealloc(p, l);
            }
        }
        q
    }
}

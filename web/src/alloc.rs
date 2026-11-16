//! The allocator of the build with threads. The allocator of the standard library for WebAssembly has one global lock,
//! and the threads of rayon wait for it: the decode of the blocks makes millions of small allocations (Sweden at zoom 8:
//! step 2 took 54 s, the command line 7 s). Here each thread keeps a free list for each size of small block (16 to 256
//! bytes), and takes the global lock only to get 64 blocks at a time. A freed small block goes to the list of the thread
//! that frees it.
//! ponytail: the free small blocks never go back to the system; the WebAssembly memory never gets smaller anyway.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr::null_mut;

const UNIT: usize = 16;
const SIZES: usize = 16;
const BATCH: usize = 64;

thread_local! {
    static LISTS: [Cell<*mut u8>; SIZES] = const { [const { Cell::new(null_mut()) }; SIZES] };
}

pub struct PerThread;

/// The list of a layout, or none for a large or aligned block.
fn list(l: &Layout) -> Option<usize> {
    (l.size() <= UNIT * SIZES && l.align() <= UNIT).then(|| l.size().max(1).div_ceil(UNIT) - 1)
}

// SAFETY: a small block is a block of its size of a batch from the system allocator, and it is on one list at a time;
// each free block holds the address of the next block of its list
unsafe impl GlobalAlloc for PerThread {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let Some(i) = list(&l) else { return unsafe { System.alloc(l) } };
        LISTS.with(|lists| {
            let mut p = lists[i].get();
            if p.is_null() {
                let size = (i + 1) * UNIT;
                let batch = unsafe { System.alloc(Layout::from_size_align_unchecked(size * BATCH, UNIT)) };
                if batch.is_null() {
                    return null_mut();
                }
                for k in 0..BATCH {
                    let b = unsafe { batch.add(k * size) };
                    let next = if k + 1 < BATCH { unsafe { b.add(size) } } else { null_mut() };
                    unsafe { (b as *mut *mut u8).write(next) };
                }
                p = batch;
            }
            lists[i].set(unsafe { (p as *mut *mut u8).read() });
            p
        })
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        match list(&l) {
            Some(i) => LISTS.with(|lists| {
                unsafe { (p as *mut *mut u8).write(lists[i].get()) };
                lists[i].set(p);
            }),
            None => unsafe { System.dealloc(p, l) },
        }
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

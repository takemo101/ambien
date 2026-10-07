use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Counts {
    pub(crate) allocations: usize,
    pub(crate) deallocations: usize,
    pub(crate) reallocations: usize,
}

thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

struct ObservedSystem;

fn record(update: impl FnOnce(&mut Counts)) {
    let _ = COUNTS.try_with(|cell| {
        if let Some(mut counts) = cell.get() {
            update(&mut counts);
            cell.set(Some(counts));
        }
    });
}

// Test-only: recording is local to the measured thread, needs no allocation or lock,
// and leaves all allocation ownership and alignment with the system allocator.
unsafe impl GlobalAlloc for ObservedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|counts| counts.allocations += 1);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|counts| counts.allocations += 1);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record(|counts| counts.deallocations += 1);
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(|counts| counts.reallocations += 1);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: ObservedSystem = ObservedSystem;

struct Region;
impl Drop for Region {
    fn drop(&mut self) {
        COUNTS.with(|cell| cell.set(None));
    }
}

pub(crate) fn measure<T>(operation: impl FnOnce() -> T) -> (T, Counts) {
    COUNTS.with(|cell| {
        assert!(cell.get().is_none(), "allocation observation must not nest");
        cell.set(Some(Counts::default()));
    });
    let region = Region;
    let result = operation();
    let counts = COUNTS.with(|cell| cell.get().expect("active allocation observation"));
    drop(region);
    (result, counts)
}

//! Test-binary-only requested allocation accounting, across ALL threads.
//! Not usable as RSS, usable-size, allocator-overhead, SDK/TLS or helper bounds.
//! Never reset live accounting: runtime/fixture allocations can outlive a phase.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[global_allocator]
pub(crate) static ALLOCATOR: Requested<System> = Requested::new(System);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub live: usize,
    pub peak: usize,
    pub phase_peak: usize,
}

pub(crate) struct Requested<A> {
    inner: A,
    locked: AtomicBool,
    live: AtomicUsize,
    peak: AtomicUsize,
    phase_peak: AtomicUsize,
}

impl<A> Requested<A> {
    const fn new(inner: A) -> Self {
        Self {
            inner,
            locked: AtomicBool::new(false),
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            phase_peak: AtomicUsize::new(0),
        }
    }

    // Serialize updates and phase snapshots with allocation-free atomics. Never
    // call allocator, formatting, OS locks or user code while this lock is held.
    // In particular, a peak reset cannot erase a concurrent allocation's peak.
    fn lock(&self) -> Unlock<'_> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        Unlock(&self.locked)
    }

    fn replace(&self, old: usize, new: usize) {
        let _guard = self.lock();
        let live = self.live.load(Ordering::Relaxed) - old + new;
        self.live.store(live, Ordering::Relaxed);
        self.peak.fetch_max(live, Ordering::Relaxed);
        self.phase_peak.fetch_max(live, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.measure(false)
    }

    /// Only the exactly-one-test resource gate should reset the GLOBAL phase
    /// peak. Existing live bytes seed the next phase; lifetime peak never resets.
    pub(crate) fn begin_phase(&self) -> Snapshot {
        self.measure(true)
    }

    fn measure(&self, begin_phase: bool) -> Snapshot {
        let _guard = self.lock();
        let live = self.live.load(Ordering::Relaxed);
        if begin_phase {
            self.phase_peak.store(live, Ordering::Relaxed);
        }
        Snapshot {
            live,
            peak: self.peak.load(Ordering::Relaxed),
            phase_peak: self.phase_peak.load(Ordering::Relaxed),
        }
    }
}

struct Unlock<'a>(&'a AtomicBool);

impl Drop for Unlock<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

// SAFETY: forwards each allocation unchanged to the same underlying allocator;
// counters never allocate, dereference client pointers, or change their layout.
unsafe impl<A: GlobalAlloc> GlobalAlloc for Requested<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { self.inner.alloc(layout) };
        if !pointer.is_null() {
            self.replace(0, layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { self.inner.alloc_zeroed(layout) };
        if !pointer.is_null() {
            self.replace(0, layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { self.inner.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            // Failure leaves the original allocation/live bytes untouched.
            // Success replaces its requested size, not a fictitious second copy.
            self.replace(layout.size(), new_size);
        }
        resized
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { self.inner.dealloc(pointer, layout) };
        self.replace(layout.size(), 0);
    }
}

#[test]
fn cross_thread_free_and_phase_survivors_are_accounted() {
    // Isolated counters use the EXACT global wrapper; harness allocations cannot
    // perturb exact arithmetic assertions. Also observe the installed allocator.
    let allocator = Requested::new(System);
    let layout = Layout::from_size_align(4096, 8).unwrap();
    let pointer = unsafe { allocator.alloc_zeroed(layout) };
    assert!(!pointer.is_null());
    assert_eq!(unsafe { *pointer }, 0);
    assert_eq!(allocator.begin_phase().live, 4096);
    let address = pointer as usize;
    std::thread::scope(|scope| {
        scope.spawn(|| unsafe { allocator.dealloc(address as *mut u8, layout) });
    });
    assert_eq!(
        allocator.snapshot(),
        Snapshot {
            live: 0,
            peak: 4096,
            phase_peak: 4096
        }
    );
    assert_eq!(allocator.begin_phase().phase_peak, 0);
    assert_eq!(allocator.snapshot().peak, 4096);
    assert!(ALLOCATOR.snapshot().live > 0);
}

#[test]
fn realloc_growth_shrink_and_failure_preserve_requested_sizes() {
    let allocator = Requested::new(System);
    let small = Layout::from_size_align(128, 8).unwrap();
    let large = Layout::from_size_align(8192, 8).unwrap();
    unsafe {
        let pointer = allocator.alloc(small);
        assert!(!pointer.is_null());
        let pointer = allocator.realloc(pointer, small, large.size());
        assert!(!pointer.is_null());
        assert_eq!(allocator.snapshot().live, large.size());
        let pointer = allocator.realloc(pointer, large, small.size());
        assert!(!pointer.is_null());
        assert_eq!(allocator.begin_phase().phase_peak, small.size());
        allocator.dealloc(pointer, small);
    }
    assert_eq!(allocator.snapshot().live, 0);
    assert_eq!(allocator.snapshot().peak, large.size());

    // Deterministic failure, not an enormous allocation relying on OS behavior.
    let refusing = Requested::new(RefuseGrowth);
    unsafe {
        assert!(refusing.alloc(large).is_null());
        assert!(refusing.alloc_zeroed(large).is_null());
        assert_eq!(refusing.snapshot().peak, 0);
        let pointer = refusing.alloc(small);
        assert!(!pointer.is_null());
        let before = refusing.snapshot();
        assert!(refusing.realloc(pointer, small, large.size()).is_null());
        assert_eq!(refusing.snapshot(), before);
        refusing.dealloc(pointer, small);
    }
    assert_eq!(refusing.snapshot().live, 0);
}

struct RefuseGrowth;

// SAFETY: successful allocations/deallocations use System; failures return null
// without consuming or modifying the original allocation.
unsafe impl GlobalAlloc for RefuseGrowth {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > 128 {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }
    // Required unsafe trait signature; returning null itself needs no unsafe block.
    // ast-grep-ignore: redundant-unsafe-function
    unsafe fn alloc_zeroed(&self, _layout: Layout) -> *mut u8 {
        std::ptr::null_mut()
    }
    // Required unsafe trait signature; returning null itself needs no unsafe block.
    // ast-grep-ignore: redundant-unsafe-function
    unsafe fn realloc(&self, _pointer: *mut u8, _layout: Layout, _size: usize) -> *mut u8 {
        std::ptr::null_mut()
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

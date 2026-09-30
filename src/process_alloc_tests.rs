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
    // Allocation-free evidence of an actual failed lock attempt in isolated tests.
    contended: AtomicBool,
    live: AtomicUsize,
    peak: AtomicUsize,
    phase_peak: AtomicUsize,
}

impl<A> Requested<A> {
    const fn new(inner: A) -> Self {
        Self {
            inner,
            locked: AtomicBool::new(false),
            contended: AtomicBool::new(false),
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            phase_peak: AtomicUsize::new(0),
        }
    }

    // Serialize each backend operation AND its accounting with snapshots/resets.
    // Backends must not reenter this wrapper (including via the global allocator),
    // unwind, format, or invoke callbacks. System uses its own allocator locks;
    // our lock must never be acquired while holding a backend lock. The controlled
    // test backend only adds bounded, allocation-free atomic rendezvous.
    // A snapshot/reset therefore falls wholly before or after each operation.
    fn lock(&self) -> Unlock<'_> {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.contended.store(true, Ordering::Release);
            std::hint::spin_loop();
        }
        Unlock(&self.locked)
    }

    fn replace_locked(&self, _guard: &Unlock<'_>, old: usize, new: usize) {
        // Valid GlobalAlloc calls remove exactly a live layout. Requested live
        // sizes fit the address space; wrapping arithmetic avoids unwind paths
        // in allocator code, not a relaxation of those backend/caller invariants.
        let live = self
            .live
            .load(Ordering::Relaxed)
            .wrapping_sub(old)
            .wrapping_add(new);
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
// Only non-reentrant, non-unwinding backends described above may be used. The
// lock covers the backend call through accounting, including failed operations.
unsafe impl<A: GlobalAlloc> GlobalAlloc for Requested<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let guard = self.lock();
        let pointer = unsafe { self.inner.alloc(layout) };
        if !pointer.is_null() {
            self.replace_locked(&guard, 0, layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let guard = self.lock();
        let pointer = unsafe { self.inner.alloc_zeroed(layout) };
        if !pointer.is_null() {
            self.replace_locked(&guard, 0, layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let guard = self.lock();
        let resized = unsafe { self.inner.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            // Failure leaves the original allocation/live bytes untouched.
            // Success replaces its requested size, not a fictitious second copy.
            self.replace_locked(&guard, layout.size(), new_size);
        }
        resized
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let guard = self.lock();
        unsafe { self.inner.dealloc(pointer, layout) };
        self.replace_locked(&guard, layout.size(), 0);
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

// Deadlines only bound test failures; ordering evidence is the entered/contended
// handshake, never elapsed time. No assertions or callbacks run in the backend.
const RENDEZVOUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Default)]
struct Pause {
    armed: AtomicBool,
    entered: AtomicBool,
    release: AtomicBool,
    timed_out: AtomicBool,
}

impl Pause {
    fn after_operation(&self) {
        if !self.armed.swap(false, Ordering::AcqRel) {
            return;
        }
        let start = std::time::Instant::now();
        self.entered.store(true, Ordering::Release);
        while !self.release.load(Ordering::Acquire) {
            if start.elapsed() >= RENDEZVOUS_TIMEOUT {
                self.timed_out.store(true, Ordering::Release);
                break;
            }
            std::hint::spin_loop();
        }
    }
}

struct Controlled {
    pause: Pause,
    fail_alloc: bool,
    fail_realloc: bool,
}

// SAFETY: forwards valid operations to System unchanged, or returns null without
// consuming the old allocation. The post-operation rendezvous cannot allocate,
// unwind or reenter Requested. It pauses before Requested updates its counters.
unsafe impl GlobalAlloc for Controlled {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = if self.fail_alloc {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        };
        self.pause.after_operation();
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = if self.fail_alloc {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc_zeroed(layout) }
        };
        self.pause.after_operation();
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let resized = if self.fail_realloc {
            std::ptr::null_mut()
        } else {
            unsafe { System.realloc(pointer, layout, size) }
        };
        self.pause.after_operation();
        resized
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        self.pause.after_operation();
    }
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Alloc,
    Zeroed,
    Grow,
    Shrink,
    FailAlloc,
    FailZeroed,
    FailRealloc,
    Free,
}

fn wait_for_either(first: &AtomicBool, second: &AtomicBool) {
    let start = std::time::Instant::now();
    while !first.load(Ordering::Acquire) && !second.load(Ordering::Acquire) {
        if start.elapsed() >= RENDEZVOUS_TIMEOUT {
            break;
        }
        std::thread::yield_now();
    }
}

fn check_interleaving(operation: Operation) {
    // Test each observer separately so phase-reset order cannot hide a bad peak.
    for begin_phase in [false, true] {
        let allocator = Requested::new(Controlled {
            pause: Pause::default(),
            fail_alloc: matches!(operation, Operation::FailAlloc | Operation::FailZeroed),
            fail_realloc: matches!(operation, Operation::FailRealloc),
        });
        let old = match operation {
            Operation::Grow | Operation::FailRealloc | Operation::Free => 128,
            Operation::Shrink => 8192,
            _ => 0,
        };
        let new = match operation {
            Operation::Shrink => 128,
            Operation::Free => 0,
            _ => 8192,
        };
        let failed = matches!(
            operation,
            Operation::FailAlloc | Operation::FailZeroed | Operation::FailRealloc
        );
        let live = if failed { old } else { new };
        let original_layout = Layout::from_size_align(old.max(128), 8).unwrap();
        let target_layout = Layout::from_size_align(new.max(128), 8).unwrap();
        let original = if old == 0 {
            0
        } else {
            let pointer = unsafe { allocator.alloc(original_layout) };
            assert!(!pointer.is_null());
            pointer as usize
        };
        let before = allocator.begin_phase();
        allocator.inner.pause.armed.store(true, Ordering::Release);
        let completed = AtomicBool::new(false);
        let (result, measured, entered, contended, completed_in_gap) =
            std::thread::scope(|scope| {
                // In particular, Free transfers the original pointer from this
                // thread to another; the original requested layout follows it.
                let worker = scope.spawn(|| unsafe {
                    match operation {
                        Operation::Alloc | Operation::FailAlloc => {
                            allocator.alloc(target_layout) as usize
                        }
                        Operation::Zeroed | Operation::FailZeroed => {
                            allocator.alloc_zeroed(target_layout) as usize
                        }
                        Operation::Grow | Operation::Shrink | Operation::FailRealloc => {
                            allocator.realloc(original as *mut u8, original_layout, new) as usize
                        }
                        Operation::Free => {
                            allocator.dealloc(original as *mut u8, original_layout);
                            0
                        }
                    }
                });
                let pause = &allocator.inner.pause;
                wait_for_either(&pause.entered, &pause.timed_out);
                let entered = pause.entered.load(Ordering::Acquire);
                let observer = scope.spawn(|| {
                    let measured = if begin_phase {
                        allocator.begin_phase()
                    } else {
                        allocator.snapshot()
                    };
                    completed.store(true, Ordering::Release);
                    measured
                });
                // A strong CAS failure inside lock(), not merely a thread-start
                // signal, establishes that the observer tried through the gap.
                wait_for_either(&allocator.contended, &completed);
                let contended = allocator.contended.load(Ordering::Acquire);
                let completed_in_gap = completed.load(Ordering::Acquire);
                pause.release.store(true, Ordering::Release);
                (
                    worker.join().unwrap(),
                    observer.join().unwrap(),
                    entered,
                    contended,
                    completed_in_gap,
                )
            });
        let after = allocator.snapshot();
        let zeroed = !matches!(operation, Operation::Zeroed)
            || (result != 0 && unsafe { *(result as *const u8) } == 0);
        // Release/join and clean up before assertions, even on regression paths.
        let (remaining, layout) = if result == 0 && !matches!(operation, Operation::Free) {
            // Even an unexpected System realloc failure leaves the old pointer.
            (original, original_layout)
        } else {
            (result, target_layout)
        };
        if remaining != 0 {
            unsafe { allocator.dealloc(remaining as *mut u8, layout) };
        }
        assert!(
            entered
                && contended
                && !completed_in_gap
                && !allocator.inner.pause.timed_out.load(Ordering::Acquire),
            "{operation:?}, begin_phase={begin_phase}: observer must block in the gap"
        );
        assert_eq!(result == 0, failed || matches!(operation, Operation::Free));
        assert!(zeroed);
        assert_eq!(
            before,
            Snapshot {
                live: old,
                peak: old,
                phase_peak: old
            }
        );
        let expected = Snapshot {
            live,
            peak: old.max(live),
            phase_peak: if begin_phase { live } else { old.max(live) },
        };
        assert_eq!(measured, expected);
        assert_eq!(after, expected);
        assert_eq!(
            allocator.snapshot(),
            Snapshot {
                live: 0,
                ..expected
            }
        );
        assert_eq!(
            allocator.begin_phase(),
            Snapshot {
                live: 0,
                phase_peak: 0,
                ..expected
            }
        );
    }
}

#[test]
fn alloc_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::Alloc);
}

#[test]
fn alloc_zeroed_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::Zeroed);
}

#[test]
fn realloc_growth_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::Grow);
}

#[test]
fn realloc_shrink_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::Shrink);
}

#[test]
fn failed_alloc_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::FailAlloc);
}

#[test]
fn failed_alloc_zeroed_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::FailZeroed);
}

#[test]
fn failed_realloc_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::FailRealloc);
}

#[test]
fn cross_thread_dealloc_excludes_snapshot_and_phase_reset() {
    check_interleaving(Operation::Free);
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

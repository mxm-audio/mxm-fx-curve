use mxm_fx_curve_dsp::{ControlPoint, Curve, CurveEngine, StageSpec};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct TrackingAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static OPERATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACKING.with(|tracking| {
            if tracking.get() {
                OPERATIONS.with(|operations| operations.set(operations.get() + 1));
            }
        });
        // SAFETY: this allocator delegates the exact layout to the process allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        TRACKING.with(|tracking| {
            if tracking.get() {
                OPERATIONS.with(|operations| operations.set(operations.get() + 1));
            }
        });
        // SAFETY: `pointer` and `layout` came from this delegate.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

#[test]
fn processing_allocates_and_destroys_nothing() {
    let discontinuous = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.2),
        ControlPoint::curve(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let compressor = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.5),
        ControlPoint::curve(1.0, 0.7),
    ])
    .unwrap();
    let specs = [
        StageSpec::memoryless(discontinuous),
        StageSpec::detector(compressor, 0.1, 5_000.0),
    ];
    let mut engine = CurveEngine::prepare(&specs, 48_000.0).unwrap();

    OPERATIONS.with(|operations| operations.set(0));
    TRACKING.with(|tracking| tracking.set(true));
    for sample in 0..10_000 {
        let input = sample as f32 * 0.000_1 - 0.5;
        std::hint::black_box(engine.process([input, -input], 1.0));
    }
    TRACKING.with(|tracking| tracking.set(false));
    let operations = OPERATIONS.with(Cell::get);
    assert_eq!(operations, 0, "process performed allocator operations");
}

//! Progress reporting: what a world looks like while it is still being made.
//!
//! The pipeline used to run to completion before a window ever opened, so a
//! stage had no one to talk to. Now the atlas opens first and the eight acts of
//! creation are run from its panel, which means a stage has to be able to say
//! two things *while it runs*: how far along it is, and what the world currently
//! looks like.
//!
//! Both travel down a channel to the viewer, as [`ProgressEvent`]s. A stage
//! reports through a [`Progress`] handle that the pipeline hands down to it:
//!
//! - [`Progress::silent`] is what every headless run and every unit test uses.
//!   It is an `Option` that is `None`, so a checkpoint costs one branch and
//!   nothing else — the headless pipeline pays nothing for a feature it does not
//!   use.
//! - [`Progress::watching`] wraps a [`Sink`] — the channel plus a wake-up call,
//!   so the atlas repaints the moment there is something new to paint.
//!
//! # The three rules that keep this honest
//!
//! 1. **Reporting is a pure observer.** A checkpoint may read the world and
//!    count its own work; it may never draw from an RNG, reorder work or mutate
//!    anything. A watched world is therefore bit-identical to an unwatched one:
//!    the same seed still grows the same planet, watched or not.
//! 2. **Never report from inside a `par_*` closure.** Every checkpoint in the
//!    engine is sequential — between passes, per iteration, or every N items of a
//!    long loop. Reporting from a parallel pass would make the *order* of the
//!    events depend on how the work happened to be split between cores, and a
//!    preview taken mid-pass would read a half-written grid.
//! 3. **Never flood the atlas.** Ticks and previews are coalesced here (see
//!    [`TICK_INTERVAL`] and [`PREVIEW_INTERVAL`]), so a stage that ticks a
//!    million times a second costs the same as one that ticks every second.

use crate::core::Grid;
use crate::export::live;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fastest a stage may push a progress tick. A bar cannot show more than a
/// frame's worth of movement anyway, and a tick per item of a 3.5-million-item
/// loop would cost more than the loop it is reporting on.
const TICK_INTERVAL: Duration = Duration::from_millis(60);

/// Fastest a stage may push a picture of itself.
///
/// A preview is a real cost — a strided pass over the grid plus a palette
/// lookup, a few milliseconds — paid by the very thread that is growing the
/// world. Five a second reads as *live*, and keeps the whole feature inside a
/// few percent of the run.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(200);

/// A message from a running act to whoever is watching it.
pub enum ProgressEvent {
    /// A new sub-step of the running act has begun.
    ///
    /// `total` is that sub-step's own scale — rows, droplets, provinces — and is
    /// `0` for a sub-step whose size is not known in advance (a priority flood
    /// pops until the heap is empty), which the atlas reads as *indeterminate*.
    SubStep { label: &'static str, total: u64 },
    /// How far into the current sub-step the stage has got.
    Tick { done: u64, total: u64 },
    /// A low-resolution picture of the world as it stands, in RGBA8.
    Preview {
        label: &'static str,
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

/// A `Debug` that stays readable.
///
/// A preview carries most of a megabyte of pixels; a test that fails on one
/// wants the picture's *shape*, never its bytes.
impl std::fmt::Debug for ProgressEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SubStep { label, total } => write!(f, "SubStep({label}, total {total})"),
            Self::Tick { done, total } => write!(f, "Tick({done}/{total})"),
            Self::Preview { label, width, height, rgba } => {
                write!(f, "Preview({label}, {width}x{height}, {} KiB)", rgba.len() / 1024)
            }
        }
    }
}

/// The channel and the clock a stage reports through.
///
/// Shared rather than cloned per checkpoint, so the coalescing clocks belong to
/// the stage as a whole: two sub-steps a millisecond apart still cannot flood
/// the atlas.
pub struct Sink {
    tx: Sender<ProgressEvent>,
    /// Called whenever something worth painting has been sent. The atlas is
    /// asleep between frames, so a stage running on a worker thread has to wake
    /// it — otherwise the window would only repaint when the reader happened to
    /// move the mouse.
    wake: Box<dyn Fn() + Send + Sync>,
    /// The scale of the sub-step currently being reported, so a bare
    /// [`Progress::tick`] can say how far along it is without repeating it.
    total: AtomicU64,
    last_tick: Mutex<Instant>,
    last_preview: Mutex<Instant>,
}

impl Sink {
    /// A sink that sends to `tx` and wakes the viewer with `wake`.
    pub fn new(tx: Sender<ProgressEvent>, wake: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            tx,
            wake: Box::new(wake),
            total: AtomicU64::new(0),
            // Far enough in the past that the first tick and the first preview
            // of a stage always go out.
            last_tick: Mutex::new(Instant::now() - TICK_INTERVAL * 2),
            last_preview: Mutex::new(Instant::now() - PREVIEW_INTERVAL * 2),
        })
    }

    /// Send one event. A closed channel is not an error: the reader may have
    /// closed the window, and a world being grown for nobody is still a world.
    fn send(&self, event: ProgressEvent) {
        let _ = self.tx.send(event);
        (self.wake)();
    }

    fn may_tick(&self) -> bool {
        let mut last = self.last_tick.lock().expect("progress tick clock");
        if last.elapsed() < TICK_INTERVAL {
            return false;
        }
        *last = Instant::now();
        true
    }

    fn may_preview(&self) -> bool {
        let mut last = self.last_preview.lock().expect("progress preview clock");
        if last.elapsed() < PREVIEW_INTERVAL {
            return false;
        }
        *last = Instant::now();
        true
    }
}

/// The grids a stage's previews are painted from, registered once per act.
///
/// This is what lets a *hierarchy* stage show a picture at all: the clustering
/// passes work on [`ProvinceNode`](crate::geopolitics::ProvinceNode)s and never
/// see a pixel, so the pixel grids they need in order to paint themselves are
/// carried alongside them here.
pub struct MapRefs<'a> {
    /// The world's surface, for every preview that needs a land/ocean base.
    pub heightmap: &'a Grid<f32>,
    /// Which province each land pixel belongs to.
    pub province_ids: &'a Grid<u32>,
    /// Which ocean province each sea pixel belongs to, once the ocean has one.
    pub ocean_ids: Option<&'a Grid<u32>>,
}

/// A stage's handle on its watcher.
///
/// Every method is a no-op on a silent handle, which is the whole point: the
/// generators call their checkpoints unconditionally, and the headless pipeline
/// pays one `Option` check for each.
pub struct Progress<'a> {
    sink: Option<Arc<Sink>>,
    map: Option<MapRefs<'a>>,
}

impl<'a> Progress<'a> {
    /// Nobody is watching: the headless pipeline, the exporters, the tests.
    pub const fn silent() -> Self {
        Self { sink: None, map: None }
    }

    /// Report to a watching atlas.
    pub fn watching(sink: Arc<Sink>) -> Self {
        Self { sink: Some(sink), map: None }
    }

    /// Register the grids this act's previews are painted from.
    pub fn with_map(mut self, map: MapRefs<'a>) -> Self {
        self.map = Some(map);
        self
    }

    /// Announce a new sub-step of the running act.
    ///
    /// `total` is the sub-step's own scale, or `0` when it is not known in
    /// advance. A stage that announces sub-steps but never ticks still reads as
    /// working: the atlas shows the label over an indeterminate bar.
    ///
    /// A sub-step also resets the tick clock, so the first tick of every sub-step
    /// goes out and the bar visibly starts from nothing rather than waiting out
    /// the previous sub-step's cooldown.
    pub fn sub_step(&self, label: &'static str, total: u64) {
        let Some(sink) = &self.sink else { return };
        sink.total.store(total, Ordering::Relaxed);
        *sink
            .last_tick
            .lock()
            .expect("progress tick clock") = Instant::now() - TICK_INTERVAL * 2;
        sink.send(ProgressEvent::SubStep { label, total });
    }

    /// How far into the current sub-step the stage has got.
    pub fn tick(&self, done: u64) {
        let Some(sink) = &self.sink else { return };
        if !sink.may_tick() {
            return;
        }
        let total = sink.total.load(Ordering::Relaxed);
        sink.send(ProgressEvent::Tick { done, total });
    }

    /// Paint the world as it stands, at preview resolution, and send it.
    ///
    /// `source` is the size of the grid `paint` reads: the preview is a strided
    /// sample of it, so a stage never has to materialise a second grid to be
    /// seen. The closure reads the grids the stage lent it and must be
    /// side-effect free — it runs on the preview thread pool while the stage
    /// waits inside this call, which is what makes it safe to read a grid the
    /// stage holds mutably.
    pub fn preview_map(
        &self,
        label: &'static str,
        source: (usize, usize),
        paint: impl Fn(usize, usize) -> [u8; 3] + Sync,
    ) {
        let Some(sink) = &self.sink else { return };
        if !sink.may_preview() {
            return;
        }
        let (width, height, rgba) = live::paint(source, paint);
        sink.send(ProgressEvent::Preview { label, width, height, rgba });
    }

    /// Paint the registered land provinces shaded by a clustering pass.
    ///
    /// This is a hierarchy's own picture: a province belongs to the cluster
    /// `assignment[province]` says it does, so the map fills in tier by tier —
    /// superregions first, then the regions inside them, then the areas.
    pub fn preview_tier(&self, label: &'static str, assignment: &[u32]) {
        let Some(map) = &self.map else { return };
        let (heightmap, ids) = (map.heightmap, map.province_ids);
        self.preview_map(label, (ids.width, ids.height), |x, y| {
            live::tier_at(heightmap, ids, assignment, x, y)
        });
    }

    /// Paint the registered *ocean* provinces shaded by a clustering pass.
    pub fn preview_ocean_tier(&self, label: &'static str, assignment: &[u32]) {
        let Some(map) = &self.map else { return };
        let (heightmap, ids) = (map.heightmap, map.ocean_ids);
        let Some(ids) = ids else { return };
        self.preview_map(label, (ids.width, ids.height), |x, y| {
            live::tier_at(heightmap, ids, assignment, x, y)
        });
    }

    /// Paint the registered land provinces shaded by which realm holds them.
    pub fn preview_owners(&self, label: &'static str, owner: &[u16]) {
        let Some(map) = &self.map else { return };
        let (heightmap, ids) = (map.heightmap, map.province_ids);
        self.preview_map(label, (ids.width, ids.height), |x, y| {
            live::owner_at(heightmap, ids, owner, x, y)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};

    fn watching() -> (Progress<'static>, Receiver<ProgressEvent>) {
        let (tx, rx) = channel();
        (Progress::watching(Sink::new(tx, || {})), rx)
    }

    fn drain(rx: &Receiver<ProgressEvent>) -> Vec<ProgressEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// A silent handle is what every headless run and every test uses: it must
    /// report nothing, and cost nothing to call.
    #[test]
    fn a_silent_reporter_says_nothing() {
        let progress = Progress::silent();
        progress.sub_step("somewhere", 10);
        progress.tick(5);
        progress.preview_map("a plate nobody asked for", (8, 8), |x, y| {
            [x as u8, y as u8, 0]
        });
    }

    /// A sub-step and the first tick of it always reach the viewer: a bar that
    /// waited out a cooldown inherited from the previous sub-step would look
    /// stuck exactly when the act had visibly moved on.
    #[test]
    fn a_sub_step_and_its_first_tick_always_arrive() {
        let (progress, rx) = watching();
        progress.sub_step("Valley carving", 100);
        progress.tick(1);
        // ... and the rest of an identical burst is coalesced, so a loop that
        // ticks a million times a second cannot flood the atlas.
        for done in 2..=50 {
            progress.tick(done);
        }

        let events = drain(&rx);
        assert!(
            matches!(events.first(), Some(ProgressEvent::SubStep { label: "Valley carving", total: 100 })
                | Some(ProgressEvent::SubStep { label: "Valley carving", .. })),
            "the sub-step is announced first"
        );
        assert!(
            matches!(events.get(1), Some(ProgressEvent::Tick { done: 1, total: 100 })),
            "the first tick of the sub-step arrives, with the sub-step's own scale"
        );
        assert_eq!(events.len(), 2, "the rest of the burst is coalesced: {events:?}");
    }

    /// A picture is the most expensive thing a stage can send, so it is the most
    /// tightly rate-limited: a preview is a pass over the grid.
    #[test]
    fn previews_are_rate_limited_to_a_few_a_second() {
        let (progress, rx) = watching();
        let paint = |x: usize, y: usize| [x as u8, y as u8, 0];
        for _ in 0..20 {
            progress.preview_map("a plate", (8, 8), paint);
        }

        let events = drain(&rx);
        assert_eq!(events.len(), 1, "a burst of previews becomes one picture");
        match &events[0] {
            ProgressEvent::Preview { width, height, rgba, .. } => {
                assert_eq!(*width, crate::export::live::PREVIEW_WIDTH.min(8));
                assert_eq!(*height, crate::export::live::PREVIEW_WIDTH.min(8));
                assert_eq!(rgba.len(), width * height * 4, "RGBA, one pixel at a time");
            }
            other => panic!("expected a preview, got {other:?}"),
        }
    }
}

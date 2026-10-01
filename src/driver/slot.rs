use crate::dsp::filters::FilterDsp;
use crate::dsp::vp8l::Vp8lDsp;
use crate::dsp::yuv::YuvDsp;
use crate::error::{Error, Result};
use crate::image::FILE_PADDING;
use crate::input::Input;
use crate::picture::{Buffer, Frame};
use crate::vp8l::Output as Lossless;

use super::{empty_view, lossless_view, lossy_view, Source};

/// Everything outside a frame that decoding it reads, and nothing it writes.
/// A slot is handed one of these rather than reaching back into the decoder,
/// which is what lets several slots be filled at once.
#[derive(Clone, Copy)]
pub(crate) struct FrameEnv<'e, 'i> {
    pub(crate) input: &'e Input<'i>,
    pub(crate) ldsp: &'e Vp8lDsp,
    pub(crate) fdsp: &'e FilterDsp,
    pub(crate) ydsp: &'e YuvDsp,
    pub(crate) settings: FrameSettings,
    pub(crate) threads: usize,
}

impl std::ops::Deref for FrameEnv<'_, '_> {
    type Target = FrameSettings;

    fn deref(&self) -> &FrameSettings {
        &self.settings
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrameSettings {
    pub(crate) bypass_filtering: bool,
    pub(crate) no_fancy_upsampling: bool,
    /// The output format alone decides the frame must become ARGB, whatever
    /// the frames before it did, so the conversion can happen off the walk.
    pub(crate) to_argb: bool,
    /// libwebp premultiplies a frame before compositing it.
    pub(crate) premultiply: bool,
}

/// The frames an animation may have decoded ahead of the walk at once.
///
/// A batch costs about as long as its slowest frame, so the fewer batches an
/// animation takes the better, up to the point where the frames no longer
/// each find a core: on 18 threads, 16 decoded a 42-frame animation 1.14x
/// faster than 12, and 18 did it 0.85x as fast as 16 while other work was
/// running.
pub(crate) const MAX_SLOTS: usize = 16;

/// Everything one frame's decode produces and nothing that outlives it.
///
/// A still uses a single slot. An animation's frames depend on nothing but
/// their own bytes, so they can be decoded into a slot each, which is what
/// makes them separable; compositing, which depends on every frame before it,
/// still walks them in order.
#[derive(Default)]
pub(crate) struct FrameSlot {
    pub(crate) vp8: Vec<crate::vp8::Decoder>,
    pub(crate) vp8l: crate::vp8l::Decoder,

    pub(crate) has_alpha: bool,
    pub(crate) alpha_compression: i32,
    pub(crate) alpha_filter: i32,
    pub(crate) alpha_data_offset: usize,
    pub(crate) alpha_data_size: usize,
    pub(crate) alpha_plane: Vec<u8>,

    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) lossless_has_alpha: bool,
    pub(crate) lossless_out: Option<Lossless>,

    /// Where a sub-frame was converted to ARGB, when it had to be.
    pub(crate) converted: Buffer,

    /// Set once the image has been put in the form compositing wants, so a
    /// frame prepared in a batch is not converted or premultiplied twice.
    prepared: Option<Source>,
}

impl FrameSlot {
    pub(crate) fn view(&self, which: Source) -> Frame<'_> {
        match which {
            Source::Lossy => lossy_view(
                self.vp8.first(),
                &self.alpha_plane,
                self.has_alpha,
                self.width,
                self.height,
            ),
            Source::Lossless => lossless_view(&self.vp8l, self.lossless_out),
            Source::Converted => self.converted.frame(),
            Source::Canvas | Source::None => empty_view(),
        }
    }

    /// The view a still is exported from, beside the buffer it is converted
    /// into. They are different fields, which is the only reason both can be
    /// held at once; `which` is never Converted here, since that buffer is the
    /// destination rather than a source.
    pub(crate) fn split_converted(
        &mut self,
        which: Source,
    ) -> (Frame<'_>, &mut Buffer) {
        let Self {
            vp8,
            vp8l,
            alpha_plane,
            has_alpha,
            width,
            height,
            lossless_out,
            converted,
            ..
        } = self;
        let img = match which {
            Source::Lossy => {
                lossy_view(vp8.first(), alpha_plane, *has_alpha, *width, *height)
            }
            Source::Lossless => lossless_view(vp8l, *lossless_out),
            Source::Converted | Source::Canvas | Source::None => empty_view(),
        };

        (img, converted)
    }

    /// Puts the decoded image in the form compositing wants: ARGB where the
    /// caller says it must be, premultiplied where the output format is.
    /// Runs at most once per frame, whichever thread gets there first.
    pub(crate) fn prepare(
        &mut self,
        env: &FrameEnv<'_, '_>,
        which: Source,
        to_argb: bool,
    ) -> Result<Source> {
        if let Some(out) = self.prepared {
            return Ok(out);
        }
        let mut out = which;

        if to_argb {
            let (src, converted) = self.split_converted(which);

            if src.format != crate::image::Format::Argb {
                crate::driver::convert::convert_to_argb(
                    env.ydsp,
                    converted,
                    &src,
                    env.no_fancy_upsampling,
                    env.threads,
                )?;
                out = Source::Converted;
            }
        }

        if env.premultiply {
            let Self {
                converted,
                vp8l,
                lossless_out,
                ..
            } = self;
            let view = match out {
                Source::Converted => Some(converted.frame_mut()),
                Source::Lossless => lossless_out.and_then(|w| vp8l.view_mut(w)),
                Source::Lossy | Source::Canvas | Source::None => None,
            };

            if let Some(mut view) = view {
                for y in 0..view.height {
                    (env.ydsp.premultiply_row)(view.row(0, y), true);
                }
            }
        }
        self.prepared = Some(out);
        Ok(out)
    }

    pub(crate) fn reset(&mut self) {
        self.prepared = None;
        self.vp8l.reset();
        self.width = 0;
        self.height = 0;
        self.has_alpha = false;
        self.lossless_has_alpha = false;
        self.lossless_out = None;
    }

    pub(crate) fn release(&mut self) {
        self.vp8l.release();
        self.converted.release();
    }

    /// Records the size the image turned out to be, warning where it is not
    /// the size the frame before it was.
    pub(crate) fn set_size(&mut self, w: i32, h: i32) {
        if self.width != 0 && self.width != w {
            crate::log::warning_args(format_args!(
                "Width mismatch. {} != {w}",
                self.width
            ));
        }
        self.width = w;
        if self.height != 0 && self.height != h {
            crate::log::warning_args(format_args!(
                "Height mismatch. {} != {h}",
                self.height
            ));
        }
        self.height = h;
    }

    pub(crate) fn lossless_canvas_in(&mut self) {
        self.vp8l.set_canvas(self.width, self.height);
    }

    pub(crate) fn lossless_canvas_out(&mut self) {
        self.width = self.vp8l.width;
        self.height = self.vp8l.height;
        self.lossless_has_alpha = self.vp8l.has_alpha;
    }

    pub(crate) fn lossless_decode(
        &mut self,
        env: &FrameEnv<'_, '_>,
        offset: usize,
        size: usize,
    ) -> Result<()> {
        /* The canvas a lossless image is decoded against is whatever this
         * slot has been told to expect, which is nothing for a still and the
         * sub-frame's declared size inside an ANMF. */
        self.lossless_canvas_in();
        self.vp8l.threads = env.threads;

        let ret = self.vp8l.decode_frame(
            crate::vp8l::Target::Argb,
            env.input.chunk(offset, size),
            false,
            None,
        );

        self.lossless_canvas_out();
        ret?;
        self.lossless_out = Some(Lossless::Argb);
        Ok(())
    }

    pub(crate) fn set_alpha_chunk(
        &mut self,
        header: i32,
        offset: usize,
        size: usize,
    ) -> Result<()> {
        if header >> 4 & 3 > super::ALPHA_PREPROCESSED_LEVELS || header >> 6 != 0 {
            crate::log::error_args(format_args!(
                "invalid ALPHA chunk header 0x{header:02x}"
            ));
            return Err(Error::InvalidData);
        }
        self.alpha_data_offset = offset;
        self.alpha_data_size = size;

        let compression = header & 3;

        if compression > super::ALPHA_COMPRESSION_VP8L {
            crate::log::error("unsupported ALPHA compression");
            return Err(Error::Unsupported);
        }
        self.has_alpha = true;
        self.alpha_compression = compression;
        self.alpha_filter = header >> 2 & 3;
        Ok(())
    }

    /// Walks the sub-chunks of one ANMF and decodes the image it carries,
    /// with its alpha channel if it has one. It reads nothing but its own
    /// bytes, which is what lets several frames be decoded at once.
    pub(crate) fn decode_anmf_image(
        &mut self,
        env: &FrameEnv<'_, '_>,
        base: usize,
        size: usize,
    ) -> Result<Source> {
        self.has_alpha = false;
        self.width = 0;
        self.height = 0;
        self.prepared = None;

        let mut sub: Option<Source> = None;
        let mut at = base + 16;
        let end = base + size;

        if size < 16 {
            crate::log::error("ANMF chunk too short for a frame header");
            return Err(Error::InvalidData);
        }

        while end - at >= 8 {
            let (chunk_type, payload_size) = {
                let head = env.input.chunk(at, 8);

                if head.len() < 8 {
                    break;
                }
                (crate::bits::rl32(head), crate::bits::rl32(&head[4..]))
            };

            if payload_size == u32::MAX {
                return Err(Error::InvalidData);
            }
            let payload_size = payload_size as usize;
            let padded_size = payload_size + (payload_size & 1);

            at += 8;
            if end - at < padded_size {
                break;
            }

            match chunk_type {
                crate::container::TAG_ALPH => {
                    if payload_size == 0 {
                        crate::log::error("invalid ALPHA chunk size");
                        return Err(Error::InvalidData);
                    }
                    if sub.is_some() || self.has_alpha {
                        crate::log::error("ALPHA chunk after the image it belongs to");
                        return Err(Error::InvalidData);
                    }
                    let header = env.input.chunk(at, 1)[0] as i32;

                    self.set_alpha_chunk(header, at + 1, payload_size - 1)?;
                }
                crate::container::TAG_VP8 if sub.is_none() => {
                    self.lossy_decode_frame(env, at, payload_size)?;
                    sub = Some(Source::Lossy);
                }
                crate::container::TAG_VP8L if sub.is_none() && !self.has_alpha => {
                    self.lossless_decode(env, at, payload_size)?;
                    sub = Some(Source::Lossless);
                }
                crate::container::TAG_VP8 | crate::container::TAG_VP8L => {
                    return Err(Error::InvalidData);
                }
                _ => {}
            }
            at += padded_size;
        }

        let which = sub.ok_or_else(|| {
            crate::log::error("image data not found");
            Error::InvalidData
        })?;

        /* Only where the output format decides it on its own; otherwise it
         * depends on the canvas, which only the walk knows about. */
        if env.to_argb {
            return self.prepare(env, which, true);
        }
        Ok(which)
    }

    /// Whether the image this slot holds carries an alpha channel.
    pub(crate) fn frame_has_alpha(&self, which: Source) -> bool {
        match which {
            Source::Lossless => self.lossless_has_alpha,
            _ => self.has_alpha,
        }
    }

    pub(crate) fn size(&self) -> (i32, i32) {
        self.vp8
            .first()
            .map_or((0, 0), |vp8| (vp8.width, vp8.height))
    }

    pub(crate) fn vp8_decoder(&mut self) -> Result<&mut crate::vp8::Decoder> {
        if self.vp8.is_empty() {
            self.vp8
                .try_reserve_exact(1)
                .map_err(|_| crate::error::Error::NoMemory)?;
            self.vp8.push(crate::vp8::Decoder::new());
        }
        Ok(&mut self.vp8[0])
    }
}

/// One frame of a batch decoded ahead of the walk that composites it: where
/// its bytes are, and what decoding them produced.
#[derive(Clone, Copy)]
pub(crate) struct AheadEntry {
    pub(crate) base: usize,
    pub(crate) size: usize,
    pub(crate) out: Result<Source>,
    /// Handed to the pool and not collected yet, so `out` means nothing and
    /// the entry's slot is a stand-in.
    pub(crate) pending: bool,
}

/// Frames decoded ahead of the one being handed out, and the slots holding
/// them. Empty whenever a decode cannot run ahead: a streamed animation, a
/// canvas too large to hold several frames, or one thread.
///
/// A run of frames goes through the slots in turn: frame `j` of the run is
/// entry and slot `j % entries.len()`, and each slot is given the next frame
/// as soon as the walk takes the one it held.
#[derive(Default)]
pub(crate) struct Ahead {
    pub(crate) slots: Vec<FrameSlot>,
    pub(crate) entries: Vec<AheadEntry>,
    /// The next frame of the run to hand out.
    pub(crate) pos: usize,
    /// One past the last frame of the run handed to a slot.
    pub(crate) end: usize,
    /// Where the chunk after that frame starts, while the run may go on.
    pub(crate) next: Option<usize>,
    pub(crate) settings: FrameSettings,
    /// The threads the decode could use when the run started, which sized
    /// the pool.
    pub(crate) threads: usize,
    /// Each slot's copy of the payload it is decoding, kept to be reused.
    inputs: Vec<Input<'static>>,
    /// The most bytes a slot's copy may hold, padding included: its share of
    /// what the run may spend beside the decoded frames. A frame with a
    /// larger payload ends the run.
    payload_cap: usize,
    pool: Option<Pool>,
}

impl Ahead {
    pub(crate) fn clear(&mut self) {
        if self.entries.iter().any(|e| e.pending) {
            if let Some(pool) = &self.pool {
                let n = self.entries.len();

                for finished in pool.drain() {
                    self.slots[finished.index % n] = finished.slot;
                    self.inputs[finished.index % n] = finished.input;
                }
            }
        }
        self.entries.clear();
        self.pos = 0;
        self.end = 0;
        self.stop();
    }

    pub(crate) fn release(&mut self) {
        self.clear();
        for slot in &mut self.slots {
            slot.release();
        }
    }

    /// True once every frame decoded ahead has been handed over.
    pub(crate) fn spent(&self) -> bool {
        self.pos >= self.end
    }

    /// Starts a run with `entries`, handing every one after the first to a
    /// pool of up to `workers` threads, at most `beside` of which decode
    /// while the calling thread is not waiting on them. `next` is where the
    /// chunk after the last entry starts, if the run may go on past it.
    /// `payload_cap` bounds each slot's copy of a payload; an entry whose
    /// payload is larger, or cannot be copied, ends the run.
    pub(crate) fn start(
        &mut self,
        env: &FrameEnv<'_, '_>,
        entries: Vec<AheadEntry>,
        next: Option<usize>,
        workers: usize,
        beside: usize,
        payload_cap: usize,
    ) {
        if self.pool.as_ref().is_none_or(|pool| {
            (pool.shared.size, pool.shared.beside) != (workers, beside)
        }) {
            /* The old threads have nothing left to do: a run is only
             * started once the one before it has been collected. */
            self.pool = Some(Pool::new(workers, beside, env));
        }
        self.inputs.truncate(entries.len());
        self.inputs.resize_with(entries.len(), Input::default);
        /* More slots, or larger decoded frames, can lower each copy's share. */
        for input in &mut self.inputs {
            if input.capacity() > payload_cap {
                *input = Input::default();
            }
        }
        self.payload_cap = payload_cap;
        self.entries = entries;
        self.pos = 0;
        self.end = self.entries.len();
        self.next = next;
        self.settings = env.settings;

        let mut jobs = Vec::new();

        for j in 1..self.end {
            if jobs.try_reserve(1).is_err() {
                self.cut(j);
                break;
            }
            match self.job(env.input, j) {
                Some(job) => jobs.push(job),
                None => {
                    self.cut(j);
                    break;
                }
            }
        }
        if let Some(pool) = &mut self.pool {
            pool.submit(jobs, self.next.is_none());
        }
    }

    /// Hands `entry`, the frame after the last one handed out, to the slot
    /// the walk has just emptied, if the run is still going. `next` is where
    /// the chunk after it starts.
    pub(crate) fn follow(&mut self, input: &Input<'_>, entry: AheadEntry, next: usize) {
        let n = self.entries.len();

        if self.next.is_none() || self.end - self.pos >= n {
            return;
        }

        let j = self.end;

        self.entries[j % n] = entry;
        match self.job(input, j) {
            Some(job) => {
                self.end += 1;
                self.next = Some(next);
                if let Some(pool) = &mut self.pool {
                    pool.submit(vec![job], false);
                }
            }
            None => self.stop(),
        }
    }

    /// The threads the walk may spread its own work over, out of the
    /// `threads` the decode may use: those the pool is not decoding frames
    /// on, or about to. Only the walk hands the pool frames, so the pool can
    /// only get less busy while that work runs.
    pub(crate) fn walk_threads(&self, threads: usize) -> usize {
        let Some(pool) = &self.pool else {
            return threads;
        };
        let queue = pool.shared.lock();
        let busy = (queue.running + queue.waiting.len()).min(pool.shared.beside);

        threads.saturating_sub(busy).max(1)
    }

    /// Ends the run at the frames already handed out.
    pub(crate) fn stop(&mut self) {
        self.next = None;
        if let Some(pool) = &self.pool {
            pool.end();
        }
    }

    /// Ends the first batch of a run before frame `j`, which is not queued.
    fn cut(&mut self, j: usize) {
        self.entries.truncate(j);
        self.end = j;
        self.next = None;
    }

    /// Packs frame `j` of the run up for the pool, with its slot and a copy
    /// of its payload, unless that payload is over the slot's share.
    fn job(&mut self, input: &Input<'_>, j: usize) -> Option<Job> {
        let k = j % self.entries.len();
        let entry = &mut self.entries[k];

        if entry.size > self.payload_cap.saturating_sub(FILE_PADDING) {
            return None;
        }

        let mut copy = std::mem::take(&mut self.inputs[k]);

        if copy.own_exact(input.chunk(entry.base, entry.size)).is_err() {
            self.inputs[k] = copy;
            return None;
        }
        entry.pending = true;
        Some(Job {
            index: j,
            slot: std::mem::take(&mut self.slots[k]),
            input: copy,
            size: entry.size,
            settings: self.settings,
        })
    }

    /// Brings frame `j` of the run back from the pool, waiting for it to be
    /// decoded or decoding it here if no thread has started on it, and
    /// returns the slot it is in.
    pub(crate) fn collect(&mut self, j: usize) -> usize {
        let k = j % self.entries.len();
        let (Some(pool), true) = (&self.pool, self.entries[k].pending) else {
            return k;
        };
        let finished = pool.collect(j);

        finished.logs.replay();
        self.slots[k] = finished.slot;
        self.inputs[k] = finished.input;
        self.entries[k].pending = false;
        match finished.out {
            Ok(out) => self.entries[k].out = out,
            Err(panic) => std::panic::resume_unwind(panic),
        }
        k
    }

    /// Waits for every frame handed to a slot to be decoded.
    #[cfg(all(test, feature = "threads"))]
    pub(crate) fn settle(&mut self) {
        for j in self.pos..self.end {
            self.collect(j);
        }
    }

    /// Counts what holds the pool's state, which stays above zero while any
    /// of its threads has yet to leave.
    #[cfg(all(test, feature = "threads"))]
    pub(crate) fn pool_holders(&self) -> impl Fn() -> usize {
        let shared = self
            .pool
            .as_ref()
            .map(|pool| std::sync::Arc::downgrade(&pool.shared));

        move || shared.as_ref().map_or(0, std::sync::Weak::strong_count)
    }

    /// The most threads that have decoded at once, the calling one counted
    /// unless it was waiting on the pool.
    #[cfg(all(test, feature = "threads"))]
    pub(crate) fn peak(&self) -> usize {
        self.pool.as_ref().map_or(0, |pool| pool.shared.lock().peak)
    }
}

/// Threads that decode an animation's frames ahead of the walk, kept from
/// one frame of a run to the next.
///
/// A batch decoded inside a scope has to be finished before the call that
/// started it returns, so the walk could composite nothing until the last
/// frame of the batch was in, and every batch paid again for starting its
/// threads. These outlive the call instead: the frames of a batch are handed
/// over and the walk collects each as it reaches it, compositing the ones
/// before while the rest are still being decoded. A job owns everything it
/// touches, the slot it decodes into and a copy of its ANMF payload, so
/// nothing it holds is borrowed from the decoder. Idle threads wait on a
/// condition variable while the run may go on, and leave once it cannot,
/// while the walk composites the last of its frames.
///
/// The calling thread counts against the threads a decode may use: it
/// decodes a run's first frame and any frame no thread has started on when
/// the walk reaches it, and composites every frame. So while it works the
/// pool decodes a frame fewer at once than the budget allows, and its last
/// thread decodes only while the calling thread waits on the pool. That is
/// not left to the calling thread, which would then still be decoding when
/// the frame it waited on came in, and hold up compositing it: at four
/// threads a 60-frame 480x360 lossy animation took 10.7ms that way, and
/// 10.5ms with a pool a thread smaller and the calling thread idle while it
/// waits, against 8.9ms this way.
struct Pool {
    shared: std::sync::Arc<Shared>,
    /// The threads started from here, each of which joins the ones it
    /// started in turn.
    starters: Vec<std::thread::JoinHandle<()>>,
}

struct Shared {
    /// The most threads the pool may start, and the most frames it decodes
    /// at once while the calling thread waits on one.
    size: usize,
    /// The most frames it decodes at once otherwise.
    beside: usize,
    queue: std::sync::Mutex<Queue>,
    /// Signalled when a job is queued, or the run is ending.
    work: std::sync::Condvar,
    /// Signalled when a job has finished.
    done: std::sync::Condvar,
    ldsp: Vp8lDsp,
    fdsp: FilterDsp,
    ydsp: YuvDsp,
}

#[derive(Default)]
struct Queue {
    waiting: std::collections::VecDeque<Job>,
    finished: Vec<Finished>,
    running: usize,
    /// Threads started and not leaving yet.
    threads: usize,
    /// No job will be queued before another run starts, so a thread that
    /// finds none waiting leaves.
    ending: bool,
    /// The job the calling thread waits on, which lends it its turn.
    lent: Option<usize>,
    /// The most threads that have decoded at once, the calling one counted
    /// unless it was waiting on the pool.
    #[cfg(test)]
    peak: usize,
}

struct Job {
    index: usize,
    slot: FrameSlot,
    input: Input<'static>,
    size: usize,
    settings: FrameSettings,
}

struct Finished {
    index: usize,
    slot: FrameSlot,
    input: Input<'static>,
    out: std::thread::Result<Result<Source>>,
    /// What the decode logged, passed on when the walk collects the frame:
    /// by then the call that started the decode may have returned, and the
    /// caller may have changed its log callback or freed what it points to.
    /// A frame dropped uncollected takes its messages with it.
    logs: crate::log::Held,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Forgets `n` threads counted as started that could not be.
    fn lost(&self, n: usize) {
        self.lock().threads -= n;
    }

    fn wait<'q>(
        &self,
        on: &std::sync::Condvar,
        queue: std::sync::MutexGuard<'q, Queue>,
    ) -> std::sync::MutexGuard<'q, Queue> {
        on.wait(queue)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn run(&self, job: Job) -> Finished {
        let Job {
            index,
            mut slot,
            input,
            size,
            settings,
        } = job;
        let env = FrameEnv {
            input: &input,
            ldsp: &self.ldsp,
            fdsp: &self.fdsp,
            ydsp: &self.ydsp,
            settings,
            threads: 1,
        };
        /* The copy starts where the ANMF payload did. A panic is carried
         * back to be raised on the thread that collects the frame. */
        let (out, logs) = crate::log::hold(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                slot.decode_anmf_image(&env, 0, size)
            }))
        });

        Finished {
            index,
            slot,
            input,
            out,
            logs,
        }
    }

    fn work(&self) {
        let mut queue = self.lock();

        loop {
            let turns = match queue.lent {
                Some(_) => self.size,
                None => self.beside,
            };
            let job = if queue.running < turns {
                queue.waiting.pop_front()
            } else {
                None
            };
            let Some(job) = job else {
                if queue.ending && queue.waiting.is_empty() {
                    break;
                }
                queue = self.wait(&self.work, queue);
                continue;
            };

            if queue.ending && queue.waiting.is_empty() {
                /* Held back for want of a turn, the others would otherwise
                 * wait for the drop to tell them to leave. */
                self.work.notify_all();
            }
            queue.running += 1;
            #[cfg(test)]
            {
                let calling = usize::from(queue.lent.is_none());

                queue.peak = queue.peak.max(queue.running + calling);
            }
            drop(queue);

            let index = job.index;
            let finished = self.run(job);

            queue = self.lock();
            queue.running -= 1;
            if queue.lent == Some(index) {
                /* The calling thread takes its turn back, and this thread's
                 * ends with it. */
                queue.lent = None;
            }
            queue.finished.push(finished);
            self.done.notify_all();
        }
        queue.threads -= 1;
    }
}

impl Pool {
    fn new(size: usize, beside: usize, env: &FrameEnv<'_, '_>) -> Self {
        let shared = std::sync::Arc::new(Shared {
            size,
            beside,
            queue: std::sync::Mutex::default(),
            work: std::sync::Condvar::new(),
            done: std::sync::Condvar::new(),
            ldsp: env.ldsp.clone(),
            fdsp: env.fdsp.clone(),
            ydsp: env.ydsp.clone(),
        });

        Self {
            shared,
            starters: Vec::new(),
        }
    }

    /// Queues `jobs`, then starts as many more threads as they could use;
    /// `last` if no job will follow them in this run. The jobs go first so
    /// that each thread finds one the moment it runs. One thread is started
    /// here and it starts the rest, since a spawn costs the spawning thread
    /// several microseconds that this one would rather spend decoding the
    /// first frame.
    fn submit(&mut self, jobs: Vec<Job>, last: bool) {
        let single = jobs.len() == 1;
        let more = {
            let mut queue = self.shared.lock();

            queue.waiting.extend(jobs);
            queue.ending = last;

            let more = (queue.waiting.len() + queue.running)
                .min(self.shared.size)
                .saturating_sub(queue.threads);

            queue.threads += more;
            more
        };

        if single {
            self.shared.work.notify_one();
        } else {
            self.shared.work.notify_all();
        }
        if more == 0 {
            return;
        }

        let mut at = 0;

        while at < self.starters.len() {
            if self.starters[at].is_finished() {
                let _ = self.starters.swap_remove(at).join();
            } else {
                at += 1;
            }
        }

        /* Fewer threads than asked for, or none, still gets every job done:
         * collect() runs whatever no thread has taken. */
        let shared = std::sync::Arc::clone(&self.shared);
        let starter = std::thread::Builder::new().spawn(move || {
            let mut started = Vec::with_capacity(more - 1);

            for _ in 1..more {
                let worker = std::sync::Arc::clone(&shared);

                match std::thread::Builder::new().spawn(move || worker.work()) {
                    Ok(thread) => started.push(thread),
                    Err(_) => {
                        shared.lost(more - 1 - started.len());
                        break;
                    }
                }
            }
            shared.work();
            for thread in started {
                let _ = thread.join();
            }
        });

        match starter {
            Ok(starter) => self.starters.push(starter),
            Err(_) => self.shared.lost(more),
        }
    }

    /// Ends the run: once the jobs queued are done, the threads leave.
    fn end(&self) {
        self.shared.lock().ending = true;
        self.shared.work.notify_all();
    }

    fn collect(&self, index: usize) -> Finished {
        let mut queue = self.shared.lock();

        loop {
            if let Some(at) = queue.finished.iter().position(|f| f.index == index) {
                return queue.finished.swap_remove(at);
            }

            let waiting = queue.waiting.iter().position(|j| j.index == index);

            if let Some(job) = waiting.and_then(|at| queue.waiting.remove(at)) {
                drop(queue);
                return self.shared.run(job);
            }
            if queue.lent.is_none() {
                queue.lent = Some(index);
                if !queue.waiting.is_empty() {
                    self.shared.work.notify_one();
                }
            }
            queue = self.shared.wait(&self.shared.done, queue);
        }
    }

    /// Takes back every job not collected, once none is still running. What
    /// they decoded is dropped, and so is any panic, which has already been
    /// reported where it happened.
    fn drain(&self) -> Vec<Finished> {
        let mut queue = self.shared.lock();
        let mut back: Vec<Finished> = queue
            .waiting
            .drain(..)
            .map(|job| Finished {
                index: job.index,
                slot: job.slot,
                input: job.input,
                out: Ok(Err(Error::InvalidData)),
                logs: crate::log::Held::default(),
            })
            .collect();

        while queue.running != 0 {
            queue = self.shared.wait(&self.shared.done, queue);
        }
        back.append(&mut queue.finished);
        back
    }
}

impl Drop for Pool {
    /// Waits for every thread to leave, as dav1d_close joins its workers:
    /// nothing may still be running the library's code once the decoder is
    /// gone, since a C caller is free to unload the library next. A job still
    /// running is one frame's decode, which finishes.
    fn drop(&mut self) {
        {
            let mut queue = self.shared.lock();

            queue.ending = true;
            queue.waiting.clear();
        }
        self.shared.work.notify_all();
        for starter in self.starters.drain(..) {
            let _ = starter.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_slots_release_copies_over_the_new_share() {
        const BUDGET: usize = 96 << 10;
        const FRAME_BYTES: usize = 6 << 10;
        const SLOTS: usize = 9;
        const CAP: usize = BUDGET / SLOTS - FRAME_BYTES;

        let payload = vec![7; 40 << 10];
        let mut input = Input::new();
        input.borrow(&payload);
        let ldsp = Vp8lDsp::default();
        let fdsp = FilterDsp::default();
        let ydsp = YuvDsp::default();
        let env = FrameEnv {
            input: &input,
            ldsp: &ldsp,
            fdsp: &fdsp,
            ydsp: &ydsp,
            settings: FrameSettings::default(),
            threads: 1,
        };
        let entries = |n, size| {
            vec![
                AheadEntry {
                    base: 0,
                    size,
                    out: Ok(Source::None),
                    pending: false,
                };
                n
            ]
        };

        /* Scale the MiB reproducer down to KiB. Check the unused first
         * slot too, and retain buffers exactly at the new allowance. */
        for first_size in [payload.len(), CAP - FILE_PADDING] {
            let mut ahead = Ahead::default();
            ahead.slots.resize_with(2, FrameSlot::default);
            /* Keep jobs queued: this test concerns their copies, not decode. */
            ahead.start(
                &env,
                entries(2, payload.len()),
                None,
                0,
                0,
                BUDGET / 2 - FRAME_BYTES,
            );
            ahead.clear();
            ahead.inputs[0].own_exact(&payload[..first_size]).unwrap();
            let first = ahead.inputs[0].bytes().as_ptr();
            assert!(ahead.inputs[1].capacity() > CAP);

            ahead.slots.resize_with(SLOTS, FrameSlot::default);
            ahead.start(&env, entries(SLOTS, 64), None, 0, 0, CAP);
            if first_size + FILE_PADDING == CAP {
                assert_eq!(ahead.inputs[0].bytes().as_ptr(), first);
                assert_eq!(ahead.inputs[0].capacity(), CAP);
            } else {
                assert_eq!(ahead.inputs[0].capacity(), 0);
            }
            ahead.clear();

            assert_eq!(ahead.inputs.len(), SLOTS);
            assert!(ahead.inputs.iter().all(|input| input.capacity() <= CAP));
            let held: usize = ahead.inputs.iter().map(Input::capacity).sum();
            assert!(held + SLOTS * FRAME_BYTES <= BUDGET);
        }
    }

    #[test]
    fn a_smaller_share_releases_retained_capacity_and_reuses_copies_that_fit() {
        let cap = 128 + FILE_PADDING;
        let mut ahead = Ahead {
            payload_cap: cap * 2,
            ..Ahead::default()
        };

        for capacity in [cap + 1, cap, cap - 1, cap + 32] {
            let mut input = Input::new();

            input.own_exact(&vec![0; capacity - FILE_PADDING]).unwrap();
            /* The current payload fits, but its retained allocation may not. */
            input.own_exact(&[1; 16]).unwrap();
            assert_eq!(input.capacity(), capacity);
            ahead.inputs.push(input);
            ahead.slots.push(FrameSlot::default());
        }
        let kept = [
            ahead.inputs[1].bytes().as_ptr(),
            ahead.inputs[2].bytes().as_ptr(),
        ];
        let mut input = Input::new();

        input.borrow(&[9; 16]);
        let env = FrameEnv {
            input: &input,
            ldsp: &Vp8lDsp::new(),
            fdsp: &FilterDsp::new(),
            ydsp: &YuvDsp::new(),
            settings: FrameSettings::default(),
            threads: 1,
        };
        let entries = vec![
            AheadEntry {
                base: 0,
                size: 16,
                out: Err(Error::InvalidData),
                pending: false,
            };
            4
        ];

        /* Leave jobs queued so clear() returns their copies without decoding. */
        ahead.start(&env, entries, None, 0, 0, cap);
        assert_eq!(ahead.inputs[0].capacity(), 0);
        ahead.clear();

        assert!(ahead.inputs.iter().all(|input| input.capacity() <= cap));
        for (j, capacity) in [(1, cap), (2, cap - 1)] {
            assert_eq!(ahead.inputs[j].capacity(), capacity);
            assert_eq!(ahead.inputs[j].bytes().as_ptr(), kept[j - 1]);
        }
        assert_eq!(ahead.inputs[3].capacity(), 16 + FILE_PADDING);
        for input in &ahead.inputs[1..] {
            assert_eq!(input.bytes(), &[9; 16]);
        }
    }
}

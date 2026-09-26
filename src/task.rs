//! Where the decoder hands work to another thread.
//!
//! Everything here is safe: parallelism is `std::thread::scope`, which joins
//! before it returns, so a job may borrow the caller's state and disjointness
//! is a matter of splitting owned data rather than of asserting anything. The
//! price is that threads are spawned per region instead of pooled, so a region
//! has to be worth roughly 20us before it earns the handoff; `pieces` is where
//! that judgement is written down.
//!
//! Every entry point runs the work on the calling thread when there are too
//! few threads to spread it over, so a caller needs no second path for
//! `n_threads == 1`, for a build without the `threads` feature, or for work too
//! small to split.

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};

/// More than this many threads buys nothing and costs a spawn each.
const MAX_THREADS: usize = 64;

/// Resolves `options.n_threads`, which counts the calling thread: 0 asks for
/// the number of processors this thread is allowed to run on, and anything
/// below 1 after that is 1.
pub fn resolve(requested: i32) -> usize {
    if !cfg!(feature = "threads") {
        return 1;
    }

    let n = if requested > 0 {
        requested as usize
    } else {
        available()
    };

    n.clamp(1, MAX_THREADS)
}

/// The processors this thread may run on. Asked once: every decoder created
/// with the default thread count would otherwise pay for the query.
fn available() -> usize {
    static COUNT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

    /* available_parallelism() already honours an affinity mask and a
     * container's cpu quota, which the machine-wide counts do not. */
    *COUNT.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// How many pieces to cut `total` units of work into: never so many that a
/// piece falls below `min` units, never more than there are threads. One means
/// the caller should not split at all.
pub fn pieces(total: usize, min: usize, threads: usize) -> usize {
    if threads < 2 || min == 0 {
        return 1;
    }
    (total / min).clamp(1, threads)
}

/// Runs `side` on another thread and `main` here, then joins. Both always run,
/// whatever either returns, so the caller decides which failure it reports.
/// `main` is told whether `side` runs beside it: with one thread, or when the
/// spawn fails, `side` runs after it instead, so it must not wait on `side`.
pub fn join<A: Send, B>(
    threads: usize,
    side: impl FnOnce() -> A + Send,
    main: impl FnOnce(bool) -> B,
) -> (A, B) {
    if !cfg!(feature = "threads") || threads < 2 {
        /* The order the serial path runs them in is the order the code had
         * before it was split, so a log reads the same at one thread. */
        let b = main(false);

        return (side(), b);
    }

    let side = std::sync::Mutex::new(Some(side));

    std::thread::scope(|s| {
        let job = &side;
        let handle = std::thread::Builder::new()
            .spawn_scoped(s, move || job.lock().unwrap().take().unwrap()());
        let b = main(handle.is_ok());

        let a = match handle {
            Ok(handle) => unwrap_joined(handle.join()),
            Err(_) => side.lock().unwrap().take().unwrap()(),
        };
        (a, b)
    })
}

/// Runs `f` once per element, on up to `threads` threads.
pub fn for_each<T: Send>(threads: usize, items: &mut [T], f: impl Fn(&mut T) + Sync) {
    if !cfg!(feature = "threads") || threads < 2 || items.len() < 2 {
        return items.iter_mut().for_each(&f);
    }

    let n = threads.min(items.len());
    let per = items.len().div_ceil(n);
    let f = &f;

    {
        let mut chunks = items.chunks_mut(per);
        /* The last chunk stays here: the caller has to wait for the others
         * anyway, and running one of them costs no spawn. */
        let last = chunks.next_back();

        let jobs: Vec<_> = chunks
            .map(|chunk| std::sync::Mutex::new(Some(chunk)))
            .collect();

        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(jobs.len());
            for job in &jobs {
                let borrowed = job;
                // A failed spawn leaves the job here, so resource limits only
                // reduce parallelism.
                match std::thread::Builder::new().spawn_scoped(s, move || {
                    borrowed
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .iter_mut()
                        .for_each(f);
                }) {
                    Ok(handle) => handles.push(handle),
                    Err(_) => {
                        job.lock().unwrap().take().unwrap().iter_mut().for_each(f)
                    }
                }
            }
            if let Some(chunk) = last {
                chunk.iter_mut().for_each(f);
            }
            for handle in handles {
                unwrap_joined(handle.join());
            }
        });
    }
}

/// One step of a relay after the producer: it works on a buffer and says
/// whether it wants another. A step that says no has still done its work, and
/// the buffer goes on to the steps after it, but the relay then winds down.
pub type Step<'s, T> = &'s mut (dyn FnMut(&mut T) -> bool + Send);

/// The producing end of `relay`: where empty buffers come from and full ones
/// go.
pub struct Relay<'c, 's, T> {
    route: Route<'c, 's, T>,
}

enum Route<'c, 's, T> {
    /// One thread: a buffer goes through every step as soon as it is passed
    /// on. The flag is whether they all still want more.
    Here(VecDeque<T>, &'c mut [Step<'s, T>], bool),
    Across(Receiver<T>, Option<Sender<T>>),
}

impl<T> Relay<'_, '_, T> {
    /// A buffer to fill, waiting for the steps to finish with one if they
    /// are all in their hands. Buffers come back in the order they were
    /// passed, after any never passed. None once the steps have gone, which
    /// they do after `finish`, after one said no, or after a panic.
    pub fn take(&mut self) -> Option<T> {
        match &mut self.route {
            Route::Here(idle, ..) => idle.pop_front(),
            Route::Across(idle, _) => wait(idle),
        }
    }

    /// Hands a filled buffer on. False once a step has said it wants no more,
    /// which may be a buffer or two after it said so; the steps never see
    /// the buffer then, and it is not given back.
    pub fn pass(&mut self, mut item: T) -> bool {
        match &mut self.route {
            Route::Here(idle, steps, more) => {
                if !*more {
                    return false;
                }
                for step in steps.iter_mut() {
                    *more &= step(&mut item);
                }
                idle.push_back(item);
                *more
            }
            Route::Across(_, full) => {
                full.as_ref().is_some_and(|full| full.send(item).is_ok())
            }
        }
    }

    /// Passes nothing more, so that `take` can drain the buffers still with
    /// the steps and then return None.
    pub fn finish(&mut self) {
        match &mut self.route {
            Route::Here(_, _, more) => *more = false,
            Route::Across(_, full) => *full = None,
        }
    }
}

/// Runs `produce` here and each of `steps` on a thread of its own, with
/// `items` as the buffers passing through them in turn: the producer fills
/// one, passes it on, and gets it back once the last step is done with it, so
/// it can run as many ahead as there are buffers. Every step sees the buffers
/// in the order they were passed. Returns what `produce` did and the buffers,
/// for another time; a step that said no may have kept some.
///
/// It takes a thread per step on top of this one, so with fewer than that all
/// of it runs here, a buffer at a time.
pub fn relay<'s, T: Send, R>(
    threads: usize,
    items: Vec<T>,
    produce: impl FnOnce(&mut Relay<'_, 's, T>) -> R,
    steps: &mut [Step<'s, T>],
) -> (R, Vec<T>) {
    fn here<'s, T, R>(
        items: Vec<T>,
        produce: impl FnOnce(&mut Relay<'_, 's, T>) -> R,
        steps: &mut [Step<'s, T>],
    ) -> (R, Vec<T>) {
        let mut relay = Relay {
            route: Route::Here(items.into(), steps, true),
        };
        let r = produce(&mut relay);

        match relay.route {
            Route::Here(items, ..) => (r, items.into()),
            Route::Across(..) => unreachable!(),
        }
    }

    if !cfg!(feature = "threads") || threads <= steps.len() || steps.is_empty() {
        return here(items, produce, steps);
    }

    let (idle_tx, idle_rx) = channel();

    for item in items {
        let _ = idle_tx.send(item);
    }

    /* Step i takes from receivers[i] and gives to senders[i]: the next
     * step's receiver, or for the last one back to the producer. */
    let (mut senders, mut receivers) = (Vec::new(), Vec::new());

    for _ in 0..steps.len() {
        let (tx, rx) = channel();

        senders.push(tx);
        receivers.push(rx);
    }
    let first_tx = senders.remove(0);

    senders.push(idle_tx);

    let n = steps.len();
    let mut produce = Some(produce);
    let ran = std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(n);

        for (step, (rx, tx)) in steps.iter_mut().zip(receivers.into_iter().zip(senders))
        {
            let spawned = std::thread::Builder::new().spawn_scoped(s, move || {
                while let Some(mut item) = wait(&rx) {
                    let more = step(&mut item);

                    if tx.send(item).is_err() || !more {
                        break;
                    }
                }
            });

            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => break,
            }
        }

        let complete = handles.len() == n;
        let mut relay = Relay {
            route: Route::Across(idle_rx, Some(first_tx)),
        };
        /* A failed spawn dropped the ends of the channels it was given, so
         * the steps already running stop once the producer's end goes too,
         * and the buffers stay queued for the producer to run here. */
        let r = complete.then(|| produce.take().unwrap()(&mut relay));
        let Route::Across(idle_rx, first_tx) = relay.route else {
            unreachable!()
        };

        drop(first_tx);
        handles
            .into_iter()
            .for_each(|handle| unwrap_joined(handle.join()));
        (r, idle_rx.try_iter().collect::<Vec<_>>())
    });

    match ran {
        (Some(r), items) => (r, items),
        (None, items) => here(items, produce.take().unwrap(), steps),
    }
}

/// How long a relay step spins for its next buffer before it sleeps. A step
/// mostly waits on the one before it for about as long as a row takes, tens
/// of microseconds, and a sleeping one has to be woken by a system call on
/// the other side of every handoff. Spinning 100us rather than not at all
/// made lossy 1024x1024 and 3072x3072 stills decode 1-5% faster on 3 threads
/// (Apple M-series, min of 21); 1ms did no better than 20us.
const SPIN: std::time::Duration = std::time::Duration::from_micros(100);

/// Receives, spinning for a while before sleeping.
fn wait<T>(rx: &Receiver<T>) -> Option<T> {
    use std::sync::mpsc::TryRecvError;

    let start = std::time::Instant::now();

    loop {
        match rx.try_recv() {
            Ok(item) => return Some(item),
            Err(TryRecvError::Disconnected) => return None,
            Err(TryRecvError::Empty) => {}
        }
        if start.elapsed() >= SPIN {
            return rx.recv().ok();
        }
        for _ in 0..32 {
            std::hint::spin_loop();
        }
    }
}

fn unwrap_joined<T>(joined: std::thread::Result<T>) -> T {
    match joined {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_count_of_zero_asks_the_machine_and_one_is_taken_at_its_word() {
        assert_eq!(resolve(1), 1);
        assert_eq!(resolve(3), if cfg!(feature = "threads") { 3 } else { 1 });
        assert_eq!(resolve(-4), resolve(0));
        assert!(resolve(0) >= 1);
        assert_eq!(
            resolve(i32::MAX),
            if cfg!(feature = "threads") {
                MAX_THREADS
            } else {
                1
            }
        );
    }

    #[test]
    fn work_is_only_split_where_every_piece_is_worth_a_thread() {
        assert_eq!(pieces(1024, 16, 1), 1);
        assert_eq!(pieces(15, 16, 8), 1);
        assert_eq!(pieces(48, 16, 8), 3);
        assert_eq!(pieces(1024, 16, 8), 8);
        assert_eq!(pieces(1024, 0, 8), 1);
    }

    #[test]
    fn both_sides_of_a_join_run_and_their_answers_come_back_in_order() {
        for threads in [1, 2, 8] {
            let (a, b) = join(
                threads,
                || 1u32,
                |beside| {
                    assert!(!beside || threads > 1);
                    2u32
                },
            );

            assert_eq!((a, b), (1, 2));
        }
    }

    #[test]
    fn a_relay_consumes_every_buffer_in_the_order_it_was_passed() {
        for threads in [1, 2, 3, 8] {
            let mut doubled = Vec::new();
            let mut seen = Vec::new();
            let items = vec![Vec::new(); 3];
            let mut double = |item: &mut Vec<u32>| {
                item.push(2 * item[0]);
                doubled.push(item[0]);
                true
            };
            let mut record = |item: &mut Vec<u32>| {
                seen.push(item[1]);
                true
            };
            let (produced, items) = relay(
                threads,
                items,
                |relay| {
                    for i in 0..50u32 {
                        let mut item = relay.take().unwrap();

                        item.clear();
                        item.push(i);
                        assert!(relay.pass(item));
                    }
                    50
                },
                &mut [&mut double, &mut record],
            );

            assert_eq!(produced, 50);
            assert_eq!(items.len(), 3);
            assert!(doubled.iter().copied().eq(0..50));
            assert!(seen.iter().copied().eq((0..50).map(|i| 2 * i)));
        }
    }

    #[test]
    fn a_finished_relay_gives_back_what_it_was_passed_in_order() {
        for threads in [1, 2] {
            let mut add = |item: &mut (u32, bool)| {
                item.0 += 100;
                true
            };
            let mut back = Vec::new();
            let (_, items) = relay(
                threads,
                vec![(0, false); 4],
                |relay| {
                    for i in 0..10u32 {
                        let item = relay.take().unwrap();

                        if item.1 {
                            back.push(item.0);
                        }
                        assert!(relay.pass((i, true)));
                    }
                    relay.finish();
                    assert!(!relay.pass((99, true)));
                    while let Some(item) = relay.take() {
                        if item.1 {
                            back.push(item.0);
                        }
                    }
                },
                &mut [&mut add],
            );

            assert!(back.iter().copied().eq(100..110));
            assert!(items.is_empty());
        }
    }

    #[test]
    fn a_step_that_wants_no_more_still_passes_its_buffer_on() {
        for threads in [1, 3] {
            let mut seen = Vec::new();
            let mut stop = |item: &mut u32| *item != 7;
            let mut record = |item: &mut u32| {
                seen.push(*item);
                true
            };
            let (passed, _) = relay(
                threads,
                vec![0; 2],
                |relay| {
                    let mut passed = 0;

                    while relay.take().is_some() {
                        passed += 1;
                        if !relay.pass(passed - 1) {
                            break;
                        }
                    }
                    passed
                },
                &mut [&mut stop, &mut record],
            );

            assert!(passed > 7 && passed < 20);
            assert!(seen.iter().copied().eq(0..8));
        }
    }

    #[test]
    fn every_element_is_visited_at_any_thread_count() {
        for threads in [1, 2, 3, 5, 8] {
            let mut items: Vec<usize> = (0..17).collect();

            for_each(threads, &mut items, |item| *item *= 2);
            assert!(items.iter().enumerate().all(|(i, &v)| v == 2 * i));
        }
    }
}

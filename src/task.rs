//! Where the decoder hands work to another thread.
//!
//! Everything here is safe: parallelism is `std::thread::scope`, which joins
//! before it returns, so a job may borrow the caller's state and disjointness
//! is a matter of splitting owned data rather than of asserting anything. The
//! price is that threads are spawned per region instead of pooled, so a region
//! has to be worth roughly 20us before it earns the handoff; `pieces` is where
//! that judgement is written down.
//!
//! Both entry points run the work on the calling thread when there is only one
//! thread to run it on, so a caller needs no second path for `n_threads == 1`,
//! for a build without the `threads` feature, or for work too small to split.

use crate::error::Result;

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
pub fn join<A: Send, B>(
    threads: usize,
    side: impl FnOnce() -> A + Send,
    main: impl FnOnce() -> B,
) -> (A, B) {
    if !cfg!(feature = "threads") || threads < 2 {
        /* The order the serial path runs them in is the order the code had
         * before it was split, so a log reads the same at one thread. */
        let b = main();

        return (side(), b);
    }

    std::thread::scope(|s| {
        let handle = s.spawn(side);
        let b = main();

        (unwrap_joined(handle.join()), b)
    })
}

/// Runs `f` once per element, on up to `threads` threads. Every element is
/// visited whatever the ones before it returned, and the first error in
/// element order is the one reported, so a failure does not depend on which
/// thread got there first.
pub fn for_each<T: Send>(
    threads: usize,
    items: &mut [T],
    f: impl Fn(&mut T) -> Result<()> + Sync,
) -> Result<()> {
    if !cfg!(feature = "threads") || threads < 2 || items.len() < 2 {
        return run(items, &f);
    }

    let n = threads.min(items.len());
    let per = items.len().div_ceil(n);
    let f = &f;

    std::thread::scope(|s| {
        let mut chunks = items.chunks_mut(per);
        /* The last chunk stays here: the caller has to wait for the others
         * anyway, and running one of them costs no spawn. */
        let last = chunks.next_back();
        let mut handles = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            handles.push(s.spawn(move || run(chunk, f)));
        }

        let mut first = match last {
            Some(chunk) => run(chunk, f),
            None => Ok(()),
        };

        for handle in handles.into_iter().rev() {
            let ret = unwrap_joined(handle.join());

            if ret.is_err() {
                first = ret;
            }
        }
        first
    })
}

fn run<T>(items: &mut [T], f: &(impl Fn(&mut T) -> Result<()> + ?Sized)) -> Result<()> {
    let mut first = Ok(());

    for item in items {
        let ret = f(item);

        if first.is_ok() {
            first = ret;
        }
    }
    first
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
    use crate::error::Error;

    #[test]
    fn a_thread_count_of_zero_asks_the_machine_and_one_is_taken_at_its_word() {
        assert_eq!(resolve(1), 1);
        assert_eq!(resolve(3), 3);
        assert_eq!(resolve(-4), resolve(0));
        assert!(resolve(0) >= 1);
        assert_eq!(resolve(i32::MAX), MAX_THREADS.min(resolve(i32::MAX)));
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
            let (a, b) = join(threads, || 1u32, || 2u32);

            assert_eq!((a, b), (1, 2));
        }
    }

    #[test]
    fn every_element_is_visited_at_any_thread_count() {
        for threads in [1, 2, 3, 5, 8] {
            let mut items: Vec<usize> = (0..17).collect();

            for_each(threads, &mut items, |item| {
                *item *= 2;
                Ok(())
            })
            .unwrap();
            assert!(items.iter().enumerate().all(|(i, &v)| v == 2 * i));
        }
    }

    #[test]
    fn the_first_failure_in_element_order_is_the_one_reported() {
        for threads in [1, 2, 3, 5, 8] {
            let mut items: Vec<usize> = (0..17).collect();
            let seen = std::sync::atomic::AtomicUsize::new(0);

            let ret = for_each(threads, &mut items, |item| {
                seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                match *item {
                    5 => Err(Error::InvalidData),
                    9 => Err(Error::NoMemory),
                    _ => Ok(()),
                }
            });

            assert_eq!(ret, Err(Error::InvalidData));
            assert_eq!(seen.load(std::sync::atomic::Ordering::Relaxed), 17);
        }
    }
}

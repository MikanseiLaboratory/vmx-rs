//! Rayon-backed CPU pool for slice-parallel encode/decode.
//!
//! This is **not** an async runtime. Tokio remains appropriate for I/O; use
//! `tokio::task::spawn_blocking` (or a dedicated thread) to keep codec work off
//! the async executor. Slice parallelism itself uses [rayon].

use rayon::ThreadPool as RayonPool;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::sync::{Arc, Mutex, OnceLock};

/// Process-wide pool that every subsequently created
/// [`Codec`](crate::Codec) draws its slice workers from when installed.
///
/// A codec normally owns a pool sized by the bitrate table. Deployments that
/// run many codecs at once (one per feed or replay angle) can instead install
/// one shared pool.
///
/// **When to prefer which** (measured, 1080p SQ, 8P+16E core host):
/// dedicated pools win *latency* — each frame has its own worker set, so one
/// codec's slices never wait behind another's (8 concurrent codecs: p99
/// 4.6 ms with per-codec pools vs 9-14 ms shared; 16 codecs: p99 16-18 ms
/// dedicated vs 36-72 ms shared). The shared pool wins *aggregate* rate only
/// once the machine is over-subscribed (16 codecs: p50 7.3 → 5.0 ms, at the
/// cost of that tail). Keep the default for interactive work; install a
/// shared pool only for batch-style throughput where tails do not matter.
static SHARED_POOL: OnceLock<Mutex<Option<Arc<ThreadPool>>>> = OnceLock::new();

fn shared_pool_slot() -> &'static Mutex<Option<Arc<ThreadPool>>> {
    SHARED_POOL.get_or_init(|| Mutex::new(None))
}

/// Make every subsequently created [`Codec`](crate::Codec) draw slice workers
/// from one process-wide pool instead of building its own.
///
/// `threads = None` sizes the pool to the machine. Calling this again replaces
/// the pool; codecs created earlier keep the pool they were built with, and
/// [`Codec::set_threads`](crate::Codec::set_threads) still switches one
/// instance to a private pool. Clear with [`clear_shared_thread_pool`].
pub fn use_shared_thread_pool(threads: Option<usize>) {
    let thread_count = threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
    });
    let pool = ThreadPool::new(thread_count);
    *shared_pool_slot().lock().expect("shared pool lock") = Some(Arc::new(pool));
}

/// Drop a pool installed by [`use_shared_thread_pool`]; codecs created later
/// go back to dedicated pools.
pub fn clear_shared_thread_pool() {
    *shared_pool_slot().lock().expect("shared pool lock") = None;
}

/// The installed shared pool, if any (used by `Codec::new`).
pub(crate) fn shared_thread_pool() -> Option<Arc<ThreadPool>> {
    shared_pool_slot().lock().expect("shared pool lock").clone()
}

/// Slice-parallelism helper sized to the bitrate table's thread count.
pub struct ThreadPool {
    inner: RayonPool,
    thread_count: usize,
}

impl ThreadPool {
    pub fn new(threads: usize) -> Self {
        let thread_count = threads.max(1);
        let inner = ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .thread_name(|i| format!("vmx-slice-{i}"))
            .build()
            .expect("rayon thread pool");
        Self {
            inner,
            thread_count,
        }
    }

    pub fn thread_count(&self) -> usize {
        self.thread_count
    }

    /// Apply `f` to every item of `items` on this pool.
    ///
    /// One task per item (each a single-element slice): slices finish
    /// independently, so a long tail slice no longer blocks a whole static
    /// chunk's worth of work.
    pub fn parallel_chunks_mut<T, F>(&self, items: &mut [T], f: F)
    where
        T: Send,
        F: Fn(&mut [T]) + Sync + Send,
    {
        let n = self.thread_count;
        if n <= 1 || items.len() <= 1 {
            f(items);
            return;
        }
        self.inner.install(|| {
            items
                .par_iter_mut()
                .for_each(|item| f(std::slice::from_mut(item)));
        });
    }

    /// Apply `f(index, item)` to every item of `items` on this pool.
    ///
    /// One task per item; the sequential fallback covers single-thread pools
    /// and trivial inputs. Used by the wgpu coefficient paths, which need the
    /// slice index to address disjoint windows of the shared pack buffer.
    #[cfg(feature = "wgpu")]
    pub fn parallel_enumerate_mut<T, F>(&self, items: &mut [T], f: F)
    where
        T: Send,
        F: Fn(usize, &mut T) + Sync + Send,
    {
        let n = self.thread_count;
        if n <= 1 || items.len() <= 1 {
            for (i, item) in items.iter_mut().enumerate() {
                f(i, item);
            }
            return;
        }
        self.inner.install(|| {
            items
                .par_iter_mut()
                .enumerate()
                .for_each(|(i, item)| f(i, item));
        });
    }
}

//! Rayon-backed CPU pool for slice-parallel encode/decode.
//!
//! This is **not** an async runtime. Tokio remains appropriate for I/O; use
//! `tokio::task::spawn_blocking` (or a dedicated thread) to keep codec work off
//! the async executor. Slice parallelism itself uses [rayon].

use rayon::ThreadPool as RayonPool;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;

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

    /// Apply `f` to disjoint mutable chunks of `items` on this pool.
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
        let chunk_size = items.len().div_ceil(n);
        self.inner.install(|| {
            items.par_chunks_mut(chunk_size).for_each(f);
        });
    }
}

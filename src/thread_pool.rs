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

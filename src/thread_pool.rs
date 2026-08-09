//! Persistent worker thread pool for slice-parallel encode/decode.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct ThreadPool {
    tx: Option<Sender<Job>>,
    workers: Vec<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    thread_count: usize,
}

impl ThreadPool {
    pub fn new(threads: usize) -> Self {
        let threads = threads.max(1);
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        let running = Arc::new(AtomicBool::new(true));
        let mut workers = Vec::with_capacity(threads);
        for _ in 0..threads {
            let rx = Arc::clone(&rx);
            let running = Arc::clone(&running);
            workers.push(thread::spawn(move || worker_loop(rx, running)));
        }
        Self {
            tx: Some(tx),
            workers,
            running,
            thread_count: threads,
        }
    }

    pub fn thread_count(&self) -> usize {
        self.thread_count
    }

    pub fn execute<F>(&self, job: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Box::new(job));
        }
    }
}

fn worker_loop(rx: Arc<Mutex<Receiver<Job>>>, running: Arc<AtomicBool>) {
    while running.load(Ordering::SeqCst) {
        let job = {
            let Ok(guard) = rx.lock() else { break };
            guard.recv()
        };
        match job {
            Ok(job) => job(),
            Err(_) => break,
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.tx.take();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

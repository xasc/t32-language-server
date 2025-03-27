// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Task system with one queue per worker and task stealing. Based on
//! this [talk](https://youtu.be/zULU6Hhp42w) from Sean Parent.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::{JoinHandle, available_parallelism, spawn},
};

use tree_sitter::Tree;
use url::Url;

use crate::{
    ReturnCode,
    config::Workspace,
    ls::{textdoc::TextDoc, workspace::WorkspaceMembers},
    protocol::{
        LocationLink, NumberOrString, Position, TextDocumentContentChangeEvent, TextDocumentItem,
        Uri,
    },
};

pub struct TaskSystem {
    pub rx: Receiver<TaskDone>,
    queues: Arc<Vec<JobQueue>>,
    threads: Vec<Option<JoinHandle<Result<(), ReturnCode>>>>,
    work: Arc<(AtomicU32, AtomicBool)>,
    signal: Arc<(Condvar, Mutex<usize>)>,
    slot: usize,
}

pub struct JobQueue {
    queue: Mutex<VecDeque<Task>>,
}

#[derive(Debug, Clone)]
pub enum Task {
    GoToDefinitionExtMeta(
        NumberOrString,
        TextDoc,
        Tree,
        Position,
        fn(TextDoc, Tree, Position) -> Option<LocationLink>,
    ),
    TextDocNew(TextDocumentItem, fn(TextDocumentItem) -> (TextDoc, Tree)),
    TextDocEdit(
        TextDoc,
        Tree,
        Vec<TextDocumentContentChangeEvent>,
        fn(TextDoc, Tree, Vec<TextDocumentContentChangeEvent>) -> (TextDoc, Tree),
    ),
    WorkspaceIndexScan(
        Workspace,
        &'static [&'static str],
        fn(&Workspace, &[&str]) -> WorkspaceMembers,
    ),
    WorkspaceFileScan(Url, fn(Url) -> Result<(TextDoc, Tree), Uri>),
}

#[derive(Debug)]
pub enum TaskDone {
    GoToDefinitionExtMeta(NumberOrString, Option<LocationLink>),
    TextDocNew(TextDoc, Tree),
    TextDocEdit(TextDoc, Tree),
    WorkspaceIndexScan(WorkspaceMembers),
    WorkspaceFileScan(Result<(TextDoc, Tree), Uri>),
}

#[derive(Debug)]
pub enum OngoingTask {
    TextDocUpdate { uri: String },
}

impl TaskSystem {
    pub fn build() -> Self {
        let num_workers = match available_parallelism() {
            Ok(num) => usize::from(num),
            Err(_) => 4,
        };

        let mut queues: Vec<JobQueue> = Vec::with_capacity(num_workers);
        for _ in 0..num_workers {
            queues.push(JobQueue::build());
        }

        let queues: Arc<Vec<JobQueue>> = Arc::new(queues);
        let mut threads: Vec<Option<JoinHandle<Result<(), ReturnCode>>>> =
            Vec::with_capacity(num_workers);

        let signal: Arc<(Condvar, Mutex<usize>)> = Arc::new((Condvar::new(), Mutex::new(0)));
        let work: Arc<(AtomicU32, AtomicBool)> =
            Arc::new((AtomicU32::new(0), AtomicBool::new(true)));

        let (tx, rx) = channel::<TaskDone>();
        let tx = Arc::new(tx);

        for ii in 0..num_workers {
            let s = signal.clone();
            let w = work.clone();
            let q = queues.clone();
            let ch = tx.clone();

            threads.push(Some(spawn(move || Self::run(ii, s, w, q, ch))));
        }

        TaskSystem {
            rx,
            queues,
            threads,
            work,
            signal,
            slot: 0,
        }
    }

    pub fn schedule(&mut self, job: &Task) -> Result<(), ReturnCode> {
        let num_queues = self.queues.len();
        loop {
            for ii in 0..num_queues {
                if self.queues[(self.slot + ii) % num_queues].try_push(job)? {
                    let (num_jobs, ..) = &*self.work;
                    let (enqueued, ..) = &*self.signal;

                    num_jobs.fetch_add(1, Ordering::Relaxed);
                    enqueued.notify_one();

                    self.slot += 1;
                    return Ok(());
                }
            }
            self.slot += 1;
        }
    }

    fn run(
        idx: usize,
        signal: Arc<(Condvar, Mutex<usize>)>,
        work: Arc<(AtomicU32, AtomicBool)>,
        queues: Arc<Vec<JobQueue>>,
        tx: Arc<Sender<TaskDone>>,
    ) -> Result<(), ReturnCode> {
        let (enqueued, lock) = &*signal;
        let (num_jobs, running) = &*work;

        while running.load(Ordering::Relaxed) {
            let _guard = enqueued
                .wait_while(lock.lock().expect("One lock per thread."), |_| {
                    num_jobs.load(Ordering::Relaxed) <= 0 && running.load(Ordering::Relaxed)
                })
                .unwrap();
            drop(_guard);

            loop {
                let num_queues = queues.len();
                for ii in 0..num_queues {
                    if let Some(job) = queues[(idx + ii) % num_queues].try_pop()? {
                        num_jobs.fetch_sub(1, Ordering::Relaxed);

                        let job = Self::execute(job);
                        let _ = tx.send(job);
                        break;
                    }
                }

                if num_jobs.load(Ordering::Relaxed) <= 0 || !running.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
        Ok(())
    }

    fn execute(job: Task) -> TaskDone {
        match job {
            Task::GoToDefinitionExtMeta(id, doc, tree, loc, find) => {
                TaskDone::GoToDefinitionExtMeta(id, find(doc, tree, loc))
            }
            Task::TextDocNew(doc, transform) => {
                let (doc, tree) = transform(doc);
                TaskDone::TextDocNew(doc, tree)
            }
            Task::TextDocEdit(doc, tree, changes, update) => {
                let (doc, tree) = update(doc, tree, changes);
                TaskDone::TextDocEdit(doc, tree)
            }
            Task::WorkspaceIndexScan(workspace, suffixes, locate) => {
                TaskDone::WorkspaceIndexScan(locate(&workspace, suffixes))
            }
            Task::WorkspaceFileScan(uri, scan) => TaskDone::WorkspaceFileScan(scan(uri)),
        }
    }
}

impl Drop for TaskSystem {
    fn drop(&mut self) {
        let (.., running) = &*self.work;
        let (enqueued, ..) = &*self.signal;

        running.store(false, Ordering::Relaxed);
        enqueued.notify_all();

        for (ii, t) in self.threads.iter_mut().enumerate() {
            let status = t
                .take()
                .expect("Worker must exist.")
                .join()
                .expect("Stopping the task queue must not fail.");

            if let Err(rc) = status {
                eprintln!(
                    "Error: Task queue #{ii} exited with error code {}.",
                    rc as i32
                );
            }
        }
    }
}

impl JobQueue {
    pub fn build() -> Self {
        JobQueue {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn try_push(&self, job: &Task) -> Result<bool, ReturnCode> {
        let mut queue = match self.queue.try_lock() {
            Ok(q) => q,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => return Err(ReturnCode::UnavailableErr),
        };
        queue.push_back(job.clone());
        Ok(true)
    }

    pub fn try_pop(&self) -> Result<Option<Task>, ReturnCode> {
        let mut queue = match self.queue.try_lock() {
            Ok(q) => q,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err(ReturnCode::UnavailableErr),
        };

        if queue.is_empty() {
            Ok(None)
        } else {
            Ok(queue.pop_front())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::*;

    #[test]
    fn does_not_deadlock() {
        let ts = TaskSystem::build();
        thread::sleep(Duration::from_millis(250));

        drop(ts);
    }
}

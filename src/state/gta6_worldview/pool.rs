//! Workers that serve the nearest request first: the queue is what is wanted now, set anew as the
//! camera moves, so a request no longer wanted is never served.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};

/// The soonest each key of `asks` is needed by, where a key is asked for more than once.
pub(super) fn soonest<K: Eq + Hash>(asks: impl Iterator<Item = (K, f32)>) -> HashMap<K, f32> {
    let mut out = HashMap::new();
    for (key, soon) in asks {
        let at = out.entry(key).or_insert(soon);
        *at = at.min(soon);
    }
    out
}

struct Queue<Q> {
    /// What is wanted, each with how far it is from the camera.
    wanted: Vec<(f32, Q)>,
    /// What a worker holds, or has answered and the owner has not taken yet.
    held: HashSet<Q>,
    /// What the answers not taken yet weigh.
    waiting: u64,
    stopped: bool,
}

struct Shared<Q> {
    queue: Mutex<Queue<Q>>,
    woken: Condvar,
    /// The most the answers not taken may weigh before the workers wait for the owner to take some.
    most: u64,
}

/// A pool of workers, each answering one request at a time.
pub(super) struct Pool<Q, A> {
    shared: Arc<Shared<Q>>,
    answers: Receiver<(Q, A, u64)>,
    /// Answers drawn from the workers that [`Pool::take_soonest`] has not taken yet, each with its weight.
    arrived: Mutex<Vec<(Q, A, u64)>>,
}

impl<Q: Copy + Eq + Hash + Send + 'static, A: Send + 'static> Pool<Q, A> {
    /// `workers` threads, each with a state of its own made by `state`, answering with `answer`.
    pub(super) fn new<S>(
        workers: usize,
        state: impl Fn() -> S + Send + Sync + 'static,
        answer: impl Fn(&mut S, Q) -> A + Send + Sync + 'static,
    ) -> Self {
        Self::bounded(workers, (u64::MAX, |_| 0), state, answer)
    }

    /// As [`Self::new`], but no worker starts another request while the answers the owner has not taken weigh `most` or more, each weighed by `weigh`: what is read waits on the device's uploads rather than piling up in memory.
    pub(super) fn bounded<S>(
        workers: usize,
        (most, weigh): (u64, fn(&A) -> u64),
        state: impl Fn() -> S + Send + Sync + 'static,
        answer: impl Fn(&mut S, Q) -> A + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                wanted: Vec::new(),
                held: HashSet::new(),
                waiting: 0,
                stopped: false,
            }),
            woken: Condvar::new(),
            most,
        });
        let (send, answers) = mpsc::channel();
        let (state, answer) = (Arc::new(state), Arc::new(answer));
        for _ in 0..workers.max(1) {
            let (shared, send) = (Arc::clone(&shared), send.clone());
            let (state, answer) = (Arc::clone(&state), Arc::clone(&answer));
            std::thread::spawn(move || {
                below_normal();
                let mut own = state();
                loop {
                    let request = {
                        let mut queue = shared.queue.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            if queue.stopped {
                                return;
                            }
                            let nearest = (queue.wanted.iter().enumerate())
                                .filter(|(_, (_, q))| !queue.held.contains(q))
                                .min_by(|a, b| a.1 .0.total_cmp(&b.1 .0))
                                .map(|(at, _)| at);
                            let room = queue.waiting < shared.most;
                            if let (Some(at), true) = (nearest, room) {
                                let (_, request) = queue.wanted.swap_remove(at);
                                queue.held.insert(request);
                                break request;
                            }
                            queue = (shared.woken.wait(queue)).unwrap_or_else(|p| p.into_inner());
                        }
                    };
                    // A panic answers nothing: the request stays held and is not asked again.
                    let answered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        answer(&mut own, request)
                    }));
                    if let Ok(answered) = answered {
                        let weight = weigh(&answered);
                        let mut queue = shared.queue.lock().unwrap_or_else(|p| p.into_inner());
                        queue.waiting = queue.waiting.saturating_add(weight);
                        drop(queue);
                        if send.send((request, answered, weight)).is_err() {
                            return;
                        }
                    }
                }
            });
        }
        Self {
            shared,
            answers,
            arrived: Mutex::new(Vec::new()),
        }
    }

    /// The answer that arrived whose request `soon` says is needed first, the least soonest.
    // ponytail: every answer waiting is looked at on each take; a heap if a backlog of thousands shows in the frame.
    pub(super) fn take_soonest(&self, soon: impl Fn(&Q) -> f32) -> Option<(Q, A)> {
        let mut arrived = self.arrived.lock().unwrap_or_else(|p| p.into_inner());
        arrived.extend(self.answers.try_iter());
        let first = (arrived.iter().enumerate())
            .min_by(|a, b| soon(&a.1 .0).total_cmp(&soon(&b.1 .0)))
            .map(|(at, _)| at)?;
        let (request, answer, weight) = arrived.swap_remove(first);
        drop(arrived);
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.held.remove(&request);
        queue.waiting = queue.waiting.saturating_sub(weight);
        drop(queue);
        self.shared.woken.notify_all();
        Some((request, answer))
    }

    /// Ask for `wanted`, each with its distance, in place of what was asked before.
    pub(super) fn want(&self, wanted: Vec<(f32, Q)>) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.wanted = wanted;
        drop(queue);
        self.shared.woken.notify_all();
    }

    /// The answers that arrived, while `more` says to take another.
    pub(super) fn take(&self, mut more: impl FnMut() -> bool) -> Vec<(Q, A)> {
        let mut taken = Vec::new();
        while more() {
            match self.answers.try_recv() {
                Ok(answer) => taken.push(answer),
                Err(_) => break,
            }
        }
        if taken.is_empty() {
            return Vec::new();
        }
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        for (request, _, weight) in &taken {
            queue.held.remove(request);
            queue.waiting = queue.waiting.saturating_sub(*weight);
        }
        drop(queue);
        self.shared.woken.notify_all();
        taken
            .into_iter()
            .map(|(request, answer, _)| (request, answer))
            .collect()
    }
}

/// Run the calling thread below normal priority: a worker reading while the camera moves then waits for the frame's own thread and the layout's, not they for it.
fn below_normal() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(thread: isize, priority: i32) -> i32;
        }
        const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
        // SAFETY: the handle of the calling thread is a pseudo handle, valid while the thread runs and never closed, and the call reads nothing else.
        let set = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
        if set == 0 {
            tracing::debug!("a world reader's priority was not lowered");
        }
    }
}

impl<Q, A> Drop for Pool<Q, A> {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.stopped = true;
        drop(queue);
        self.shared.woken.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn what_arrived_is_taken_by_how_soon_it_is_needed_now_and_not_as_it_arrived() {
        let pool = Pool::new(1, || (), |_, request: u32| request * 10);
        // Read nearest first: 5 then 7.
        pool.want(vec![(1.0, 5), (2.0, 7)]);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut arrived = pool.arrived.lock().unwrap_or_else(|p| p.into_inner());
            arrived.extend(pool.answers.try_iter());
            if arrived.len() == 2 || std::time::Instant::now() > until {
                break;
            }
            drop(arrived);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // The camera turned: 7 is needed first now.
        let soon = |request: &u32| if *request == 7 { 0.0 } else { 1.0 };
        assert_eq!(pool.take_soonest(soon), Some((7, 70)));
        assert_eq!(pool.take_soonest(soon), Some((5, 50)));
        assert_eq!(pool.take_soonest(soon), None);
        assert_eq!(soonest([(1, 3.0), (1, 2.0), (4, 9.0)].into_iter())[&1], 2.0);
    }

    /// Take answers from `pool` until `count` have come, or a few seconds pass.
    fn wait_for(pool: &Pool<u32, u64>, count: usize, got: &mut Vec<u32>) {
        let started = Instant::now();
        while got.len() < count && started.elapsed() < Duration::from_secs(5) {
            got.extend(pool.take(|| true).into_iter().map(|(q, _)| q));
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_bounded_pool_starts_nothing_while_what_waits_to_be_taken_weighs_its_most() {
        let pool: Pool<u32, u64> = Pool::bounded(1, (10, |weight| *weight), || (), |_, _| 6);
        pool.want(vec![(1.0, 1), (2.0, 2), (3.0, 3)]);
        // Two answers weigh 12, past the most of 10: the third waits until they are taken.
        let started = Instant::now();
        while pool.shared.queue.lock().map_or(0, |q| q.waiting) < 12
            && started.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(50));
        let queue = pool.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(queue.waiting, 12);
        assert_eq!(queue.wanted.len(), 1, "the third is not started");
        drop(queue);
        let mut got = Vec::new();
        wait_for(&pool, 3, &mut got);
        assert_eq!(got, [1, 2, 3]);
    }
}

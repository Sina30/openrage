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
    stopped: bool,
}

struct Shared<Q> {
    queue: Mutex<Queue<Q>>,
    woken: Condvar,
}

/// A pool of workers, each answering one request at a time.
pub(super) struct Pool<Q, A> {
    shared: Arc<Shared<Q>>,
    answers: Receiver<(Q, A)>,
    /// Answers drawn from the workers that [`Pool::take_soonest`] has not taken yet.
    arrived: Mutex<Vec<(Q, A)>>,
}

impl<Q: Copy + Eq + Hash + Send + 'static, A: Send + 'static> Pool<Q, A> {
    /// `workers` threads, each with a state of its own made by `state`, answering with `answer`.
    pub(super) fn new<S>(
        workers: usize,
        state: impl Fn() -> S + Send + Sync + 'static,
        answer: impl Fn(&mut S, Q) -> A + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                wanted: Vec::new(),
                held: HashSet::new(),
                stopped: false,
            }),
            woken: Condvar::new(),
        });
        let (send, answers) = mpsc::channel();
        let (state, answer) = (Arc::new(state), Arc::new(answer));
        for _ in 0..workers.max(1) {
            let (shared, send) = (Arc::clone(&shared), send.clone());
            let (state, answer) = (Arc::clone(&state), Arc::clone(&answer));
            std::thread::spawn(move || {
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
                            if let Some(at) = nearest {
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
                        if send.send((request, answered)).is_err() {
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
        let (request, answer) = arrived.swap_remove(first);
        drop(arrived);
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.held.remove(&request);
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
        if !taken.is_empty() {
            let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
            for (request, _) in &taken {
                queue.held.remove(request);
            }
        }
        taken
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
}

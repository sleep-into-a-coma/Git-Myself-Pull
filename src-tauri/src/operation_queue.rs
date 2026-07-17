use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
};

#[derive(Clone)]
pub struct OperationQueue {
    inner: Arc<(Mutex<QueueState>, Condvar)>,
    limit: Arc<AtomicUsize>,
}

struct QueueState {
    next_ticket: u64,
    waiting: VecDeque<QueueEntry>,
    active: HashSet<String>,
}

struct QueueEntry {
    ticket: u64,
    repository_id: String,
    label: String,
}

pub struct OperationPermit {
    queue: OperationQueue,
    repository_id: String,
}

pub struct OperationQueueSnapshot {
    pub active: HashSet<String>,
    pub waiting: Vec<(String, String)>,
    pub limit: usize,
}

impl OperationQueue {
    pub fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(QueueState {
                    next_ticket: 1,
                    waiting: VecDeque::new(),
                    active: HashSet::new(),
                }),
                Condvar::new(),
            )),
            limit: Arc::new(AtomicUsize::new(limit.clamp(1, 8))),
        }
    }

    pub fn enqueue(&self, repository_id: String, label: String) -> Result<(u64, usize), String> {
        let (lock, notify) = &*self.inner;
        let mut state = lock.lock().map_err(|_| "任务队列不可用")?;
        if state.active.contains(&repository_id) {
            return Err("该项目正在执行 Git 任务".into());
        }
        if state.waiting.iter().any(|entry| entry.repository_id == repository_id) {
            return Err("该项目已在 Git 任务队列中".into());
        }
        let ticket = state.next_ticket;
        state.next_ticket = state.next_ticket.wrapping_add(1).max(1);
        state.waiting.push_back(QueueEntry { ticket, repository_id, label });
        let position = state.waiting.len();
        notify.notify_all();
        Ok((ticket, position))
    }

    pub fn wait(&self, ticket: u64) -> Result<OperationPermit, String> {
        let (lock, notify) = &*self.inner;
        let mut state = lock.lock().map_err(|_| "任务队列不可用")?;
        loop {
            let is_next = state.waiting.front().is_some_and(|entry| entry.ticket == ticket);
            if is_next && state.active.len() < self.limit.load(Ordering::SeqCst) {
                let entry = state.waiting.pop_front().ok_or("任务已从队列中移除")?;
                state.active.insert(entry.repository_id.clone());
                notify.notify_all();
                return Ok(OperationPermit { queue: self.clone(), repository_id: entry.repository_id });
            }
            if !state.waiting.iter().any(|entry| entry.ticket == ticket) {
                return Err("任务已从队列中移除".into());
            }
            state = notify.wait(state).map_err(|_| "任务队列不可用")?;
        }
    }

    pub fn contains(&self, repository_id: &str) -> bool {
        let Ok(state) = self.inner.0.lock() else { return true };
        state.active.contains(repository_id) || state.waiting.iter().any(|entry| entry.repository_id == repository_id)
    }

    pub fn set_limit(&self, limit: usize) {
        self.limit.store(limit.clamp(1, 8), Ordering::SeqCst);
        self.inner.1.notify_all();
    }

    pub fn snapshot(&self) -> OperationQueueSnapshot {
        let state = self.inner.0.lock().unwrap();
        OperationQueueSnapshot {
            active: state.active.clone(),
            waiting: state.waiting.iter().map(|entry| (entry.repository_id.clone(), entry.label.clone())).collect(),
            limit: self.limit.load(Ordering::SeqCst),
        }
    }
}

impl Drop for OperationPermit {
    fn drop(&mut self) {
        let (lock, notify) = &*self.queue.inner;
        if let Ok(mut state) = lock.lock() {
            state.active.remove(&self.repository_id);
            notify.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OperationQueue;
    use std::{sync::mpsc, thread, time::Duration};

    #[test]
    fn rejects_duplicate_repository_tasks() {
        let queue = OperationQueue::new(2);
        queue.enqueue("one".into(), "等待更新".into()).unwrap();
        assert!(queue.enqueue("one".into(), "等待推送".into()).is_err());
    }

    #[test]
    fn queues_tasks_when_concurrency_is_full() {
        let queue = OperationQueue::new(1);
        let (first, _) = queue.enqueue("one".into(), "等待更新".into()).unwrap();
        let (second, _) = queue.enqueue("two".into(), "等待更新".into()).unwrap();
        let first_permit = queue.wait(first).unwrap();
        let waiting_queue = queue.clone();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let permit = waiting_queue.wait(second).unwrap();
            sender.send(()).unwrap();
            drop(permit);
        });
        assert!(receiver.recv_timeout(Duration::from_millis(80)).is_err());
        drop(first_permit);
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn increasing_concurrency_releases_waiting_tasks() {
        let queue = OperationQueue::new(1);
        let (first, _) = queue.enqueue("one".into(), "等待更新".into()).unwrap();
        let (second, _) = queue.enqueue("two".into(), "等待更新".into()).unwrap();
        let first_permit = queue.wait(first).unwrap();
        let waiting_queue = queue.clone();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let permit = waiting_queue.wait(second).unwrap();
            sender.send(()).unwrap();
            drop(permit);
        });
        queue.set_limit(2);
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(first_permit);
        handle.join().unwrap();
    }
}

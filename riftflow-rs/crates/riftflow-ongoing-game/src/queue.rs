//! `shared/utils/queue-keeper.ts` on tokio primitives.
//!
//! Same contract as the `PQueue` + `AbortController` pair it replaces: named
//! queues with a mutable `concurrency`, per-task `priority` (higher runs first,
//! `p-queue`'s `PriorityQueue`), a task id that makes `hasTask` cheap, and tags so
//! `cancelByTags([puuid, 'match-history'], 'and')` drops exactly the pending work
//! of one player.
//!
//! Two deliberate differences. A task result is a `serde_json::Value`, since every
//! queued call here is an HTTP read; and cancelling a task that is *already
//! running* resolves its waiter as aborted immediately while the request itself
//! runs to completion and is discarded — there is no abort signal to hand `reqwest`.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

/// `priority: Infinity` — the game summaries a match history is waiting on.
pub const PRIORITY_NOW: i64 = i64::MAX;

type TaskFuture = Pin<Box<dyn Future<Output = Result<Value, TaskFailure>> + Send + 'static>>;

/// Both failure kinds a queued read can have: the task was cancelled (`aborted`,
/// what `isAbortError` answered for) or it failed with an HTTP or transport error,
/// whose status code the callers still need — `additional-info-controller.ts` treats
/// a 404 as "no such game" rather than as an error.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskFailure {
    pub aborted: bool,
    pub status: Option<u16>,
    pub message: String,
}

impl TaskFailure {
    pub fn aborted() -> Self {
        Self { aborted: true, status: None, message: "AbortError".to_string() }
    }

    pub fn new(message: impl Into<String>) -> Self {
        Self { aborted: false, status: None, message: message.into() }
    }

    /// `isAbortError`.
    pub fn is_abort(&self) -> bool {
        self.aborted
    }

    /// `isAxiosError(error) && error.response.status === code`
    pub fn is_status(&self, code: u16) -> bool {
        self.status == Some(code)
    }
}

impl From<crate::fetch::FetchError> for TaskFailure {
    fn from(error: crate::fetch::FetchError) -> Self {
        Self { aborted: false, status: error.status, message: error.message }
    }
}

#[derive(Clone, Debug)]
pub struct TaskOptions {
    pub priority: i64,
    pub tags: Vec<String>,
}

impl TaskOptions {
    pub fn new() -> Self {
        Self { priority: 0, tags: Vec::new() }
    }

    pub fn priority(mut self, priority: i64) -> Self {
        self.priority = priority;
        self
    }

    pub fn tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|tag| tag.to_string()).collect::<Vec<_>>();
        self
    }
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagMode {
    And,
    /// `cancelByTags` defaults to this
    Or,
}

struct TaskRecord {
    tags: Vec<String>,
    /// dropped by a cancel *and* when the task settles; the waiter only listens to
    /// it while the reply is not ready, so a normal finish cannot read as an abort
    abort: Option<oneshot::Sender<()>>,
    /// taken when the task starts running; dropping it aborts the waiter
    reply: Option<oneshot::Sender<Result<Value, TaskFailure>>>,
    /// taken when the task starts running
    task: Option<TaskFuture>,
}

struct Pending {
    priority: i64,
    /// the enqueue sequence, so equal priorities stay first-in-first-out
    seq: u64,
    task_id: String,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}

impl Eq for Pending {}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority).then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct QueueState {
    pending: Mutex<BinaryHeap<Pending>>,
    wake: mpsc::UnboundedSender<()>,
    active: Arc<AtomicUsize>,
    concurrency: Arc<AtomicUsize>,
}

#[derive(Default)]
struct Table {
    tasks: HashMap<String, TaskRecord>,
    tags: HashMap<String, Vec<String>>,
    next_seq: u64,
}

pub struct QueueKeeper {
    queues: HashMap<String, Arc<QueueState>>,
    table: Arc<Mutex<Table>>,
}

impl QueueKeeper {
    /// `new QueueKeeper([{ id: 'match-history' }, { id: 'misc' }])`
    pub fn new(queue_ids: &[&str]) -> Self {
        let table: Arc<Mutex<Table>> = Arc::new(Mutex::new(Table::default()));
        let mut queues = HashMap::new();

        for id in queue_ids {
            let (wake, receiver) = mpsc::unbounded_channel();
            let state = Arc::new(QueueState {
                pending: Mutex::new(BinaryHeap::new()),
                wake,
                active: Arc::new(AtomicUsize::new(0)),
                // an unstarted queue accepts work before `set_concurrency` runs
                concurrency: Arc::new(AtomicUsize::new(usize::MAX)),
            });

            queues.insert((*id).to_string(), state.clone());
            spawn_worker(state, table.clone(), receiver);
        }

        Self { queues, table }
    }

    pub fn has_queue(&self, id: &str) -> bool {
        self.queues.contains_key(id)
    }

    /// `setConcurrency(id, concurrency)`.
    pub fn set_concurrency(&self, id: &str, concurrency: usize) -> anyhow::Result<()> {
        let queue = self
            .queues
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Queue {id} does not exist"))?;

        queue.concurrency.store(concurrency.max(1), AtomicOrdering::SeqCst);
        let _ = queue.wake.send(());
        Ok(())
    }

    /// `hasTask(taskId)` — true from the moment it is added until it settles,
    /// which is exactly the JS `_tasks` map's lifetime.
    pub fn has_task(&self, task_id: &str) -> bool {
        self.table.lock().unwrap().tasks.contains_key(task_id)
    }

    pub fn task_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.table.lock().unwrap().tasks.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// `add(queueId, taskId, task, options)`.
    pub async fn add<F>(
        &self,
        queue_id: &str,
        task_id: &str,
        task: F,
        options: TaskOptions,
    ) -> Result<Value, TaskFailure>
    where
        F: Future<Output = Result<Value, TaskFailure>> + Send + 'static,
    {
        let queue = self
            .queues
            .get(queue_id)
            .ok_or_else(|| TaskFailure::new(format!("Queue {queue_id} does not exist")))?;

        let (reply_tx, reply_rx) = oneshot::channel();
        let (abort_tx, abort_rx) = oneshot::channel::<()>();

        let seq = {
            let mut table = self.table.lock().unwrap();

            // `Task already exists in queue` — callers guard with `hasTask`, so
            // reaching this means two paths raced on one id
            if table.tasks.contains_key(task_id) {
                return Err(TaskFailure::new(format!(
                    "Task {task_id} already exists in queue {queue_id}"
                )));
            }

            for tag in &options.tags {
                let ids = table.tags.entry(tag.clone()).or_default();
                if !ids.iter().any(|existing| existing == task_id) {
                    ids.push(task_id.to_string());
                }
            }

            table.tasks.insert(
                task_id.to_string(),
                TaskRecord {
                    tags: options.tags.clone(),
                    abort: Some(abort_tx),
                    reply: Some(reply_tx),
                    task: Some(Box::pin(task)),
                },
            );

            table.next_seq += 1;
            table.next_seq
        };

        queue.pending.lock().unwrap().push(Pending { priority: options.priority, seq, task_id: task_id.to_string() });
        let _ = queue.wake.send(());

        // `biased` matters: a settling task drops the record, which closes the abort
        // channel too, and the reply must win that race
        tokio::select! {
            biased;
            reply = reply_rx => reply.unwrap_or_else(|_| Err(TaskFailure::aborted())),
            _ = abort_rx => Err(TaskFailure::aborted()),
        }
    }

    /// `cancelById(taskId)` — whether anything was waiting on that id.
    pub fn cancel_by_id(&self, task_id: &str) -> bool {
        let record = {
            let mut table = self.table.lock().unwrap();
            let record = table.tasks.remove(task_id);

            if let Some(record) = &record {
                for tag in &record.tags {
                    if let Some(ids) = table.tags.get_mut(tag) {
                        ids.retain(|existing| existing.as_str() != task_id);
                        if ids.is_empty() {
                            table.tags.remove(tag);
                        }
                    }
                }
            }

            record
        };

        let Some(record) = record else {
            return false;
        };

        // an unstarted task goes with the record; a running one keeps its future
        // alive but both senders closing means the waiter resolves as aborted
        drop(record.reply);
        drop(record.abort);
        true
    }

    /// `cancelByTags(tags, mode)` — how many tasks were waiting on them.
    pub fn cancel_by_tags(&self, tags: &[&str], mode: TagMode) -> usize {
        if tags.is_empty() {
            return 0;
        }

        let ids = {
            let table = self.table.lock().unwrap();

            match mode {
                TagMode::Or => {
                    let mut merged: Vec<String> = Vec::new();
                    for tag in tags {
                        for id in table.tags.get(*tag).cloned().unwrap_or_default() {
                            if !merged.iter().any(|existing| existing == &id) {
                                merged.push(id);
                            }
                        }
                    }
                    merged
                }
                TagMode::And => {
                    let first = table.tags.get(tags[0]).cloned().unwrap_or_default();
                    first
                        .into_iter()
                        .filter(|id| {
                            tags[1..]
                                .iter()
                                .all(|tag| table.tags.get(*tag).is_some_and(|ids| ids.contains(id)))
                        })
                        .collect()
                }
            }
        };

        ids.iter().filter(|id| self.cancel_by_id(id)).count()
    }

    /// `cancelAll()`.
    pub fn cancel_all(&self) {
        let ids: Vec<String> = self.table.lock().unwrap().tasks.keys().cloned().collect();
        for id in ids {
            self.cancel_by_id(&id);
        }
    }

    pub fn pending_count(&self, queue_id: &str) -> usize {
        self.queues
            .get(queue_id)
            .map(|queue| queue.pending.lock().unwrap().len())
            .unwrap_or_default()
    }

    pub fn active_count(&self, queue_id: &str) -> usize {
        self.queues
            .get(queue_id)
            .map(|queue| queue.active.load(AtomicOrdering::SeqCst))
            .unwrap_or_default()
    }
}

fn spawn_worker(
    queue: Arc<QueueState>,
    table: Arc<Mutex<Table>>,
    mut wake: mpsc::UnboundedReceiver<()>,
) {
    tokio::spawn(async move {
        while wake.recv().await.is_some() {
            drain(&queue, &table);

            // keep waking until every running task is done, so a concurrency bump
            // while busy still picks the backlog up
            while queue.active.load(AtomicOrdering::SeqCst) > 0 {
                if wake.recv().await.is_none() {
                    return;
                }
                drain(&queue, &table);
            }
        }
    });
}

fn drain(queue: &Arc<QueueState>, table: &Arc<Mutex<Table>>) {
    loop {
        let concurrency = queue.concurrency.load(AtomicOrdering::SeqCst);
        if queue.active.load(AtomicOrdering::SeqCst) >= concurrency {
            return;
        }

        let Some(job) = queue.pending.lock().unwrap().pop() else {
            return;
        };

        let started = {
            let mut table = table.lock().unwrap();
            match table.tasks.get_mut(&job.task_id) {
                Some(record) if record.task.is_some() && record.reply.is_some() => {
                    (record.reply.take(), record.task.take())
                }
                // cancelled while it sat in the backlog
                _ => continue,
            }
        };

        let (Some(reply), Some(task)) = started else {
            continue;
        };

        queue.active.fetch_add(1, AtomicOrdering::SeqCst);
        let queue = queue.clone();
        let table = table.clone();
        let task_id = job.task_id.clone();

        tokio::spawn(async move {
            let result = task.await;

            // `_tasks.delete(taskId)` + `_removeTags` in the JS `finally`
            {
                let mut table = table.lock().unwrap();
                if let Some(record) = table.tasks.remove(&task_id) {
                    for tag in &record.tags {
                        if let Some(ids) = table.tags.get_mut(tag) {
                            ids.retain(|existing| existing.as_str() != task_id);
                            if ids.is_empty() {
                                table.tags.remove(tag);
                            }
                        }
                    }
                }
            }

            let _ = reply.send(result);
            queue.active.fetch_sub(1, AtomicOrdering::SeqCst);
            let _ = queue.wake.send(());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use tokio::sync::Barrier;

    fn keeper() -> Arc<QueueKeeper> {
        Arc::new(QueueKeeper::new(&["match-history", "misc"]))
    }

    #[tokio::test]
    async fn a_task_runs_and_reports_its_value() {
        let keeper = keeper();
        let value = keeper
            .add("misc", "one", async { Ok(json!({ "ok": true })) }, TaskOptions::new())
            .await
            .expect("no failure");

        assert_eq!(value, json!({ "ok": true }));
        assert!(!keeper.has_task("one"), "the record goes when the task settles");
    }

    #[tokio::test]
    async fn an_unknown_queue_is_refused() {
        let error = keeper()
            .add("nope", "one", async { Ok(json!(1)) }, TaskOptions::new())
            .await
            .unwrap_err();

        assert_eq!(error.message, "Queue nope does not exist");
        assert!(!error.aborted);
    }

    #[tokio::test]
    async fn a_duplicate_task_id_is_refused() {
        let keeper = keeper();
        let barrier = Arc::new(Barrier::new(2));

        let first = {
            let keeper = keeper.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                keeper
                    .add("misc", "same", async move {
                        barrier.wait().await;
                        Ok(json!("first"))
                    }, TaskOptions::new())
                    .await
            })
        };

        while !keeper.has_task("same") {
            tokio::task::yield_now().await;
        }

        let duplicate = keeper
            .add("misc", "same", async { Ok(json!("second")) }, TaskOptions::new())
            .await
            .unwrap_err();

        assert_eq!(duplicate.message, "Task same already exists in queue misc");

        barrier.wait().await;
        assert_eq!(first.await.unwrap().unwrap(), json!("first"));
    }

    #[tokio::test]
    async fn the_backlog_is_popped_in_priority_order() {
        let keeper = keeper();
        keeper.set_concurrency("misc", 1).unwrap();
        let gate = Arc::new(Barrier::new(2));

        // hold the single slot so the rest pile up in the heap
        let blocker = {
            let keeper = keeper.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                keeper
                    .add("misc", "blocker", async move {
                        gate.wait().await;
                        Ok(json!("blocker"))
                    }, TaskOptions::new().priority(0))
                    .await
            })
        };

        while keeper.active_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut handles = Vec::new();

        for (id, priority) in [("low", 1i64), ("high", 50i64), ("now", PRIORITY_NOW)] {
            let keeper = keeper.clone();
            let order = order.clone();
            handles.push(tokio::spawn(async move {
                let value = keeper
                    .add("misc", id, async move {
                        order.lock().unwrap().push(id.to_string());
                        Ok(json!(id))
                    }, TaskOptions::new().priority(priority))
                    .await;
                value
            }));
        }

        // let the three reach the heap before releasing the blocker
        while keeper.pending_count("misc") < 3 {
            tokio::task::yield_now().await;
        }

        gate.wait().await;
        blocker.await.unwrap().unwrap();
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        assert_eq!(*order.lock().unwrap(), vec!["now", "high", "low"]);
    }

    #[tokio::test]
    async fn concurrency_caps_how_many_run_at_once() {
        let keeper = keeper();
        keeper.set_concurrency("match-history", 2).unwrap();

        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();

        for index in 0..6 {
            let keeper = keeper.clone();
            let running = running.clone();
            let peak = peak.clone();
            handles.push(tokio::spawn(async move {
                keeper
                    .add(
                        "match-history",
                        &format!("task-{index}"),
                        async move {
                            let now = running.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                            peak.fetch_max(now, AtomicOrdering::SeqCst);
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            running.fetch_sub(1, AtomicOrdering::SeqCst);
                            Ok(json!(index))
                        },
                        TaskOptions::new(),
                    )
                    .await
            }));
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        assert_eq!(peak.load(AtomicOrdering::SeqCst), 2);
    }

    #[tokio::test]
    async fn raising_concurrency_admits_the_backlog_without_restarting() {
        let keeper = keeper();
        keeper.set_concurrency("misc", 1).unwrap();
        let gate = Arc::new(Barrier::new(2));

        let blocker = {
            let keeper = keeper.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                keeper
                    .add("misc", "blocker", async move {
                        gate.wait().await;
                        Ok(json!(0))
                    }, TaskOptions::new())
                    .await
            })
        };

        while keeper.active_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        let waiting = {
            let keeper = keeper.clone();
            tokio::spawn(async move { keeper.add("misc", "waiting", async { Ok(json!(1)) }, TaskOptions::new()).await })
        };

        while keeper.pending_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        keeper.set_concurrency("misc", 2).unwrap();
        assert_eq!(waiting.await.unwrap().unwrap(), json!(1), "it started while the blocker ran");

        gate.wait().await;
        blocker.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_cancelled_pending_task_never_runs() {
        let keeper = keeper();
        keeper.set_concurrency("misc", 1).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let ran = Arc::new(AtomicBool::new(false));

        let blocker = {
            let keeper = keeper.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                keeper
                    .add("misc", "blocker", async move {
                        gate.wait().await;
                        Ok(json!(0))
                    }, TaskOptions::new())
                    .await
            })
        };

        while keeper.active_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        let cancelled = {
            let keeper = keeper.clone();
            let ran = ran.clone();
            tokio::spawn(async move {
                keeper
                    .add(
                        "misc",
                        "doomed",
                        async move {
                            ran.store(true, AtomicOrdering::SeqCst);
                            Ok(json!(1))
                        },
                        TaskOptions::new().tags(&["puuid-1", "match-history"]),
                    )
                    .await
            })
        };

        while keeper.pending_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        assert_eq!(keeper.cancel_by_tags(&["puuid-1", "match-history"], TagMode::And), 1);
        let failure = cancelled.await.unwrap().unwrap_err();
        assert!(failure.is_abort());

        gate.wait().await;
        blocker.await.unwrap().unwrap();
        tokio::task::yield_now().await;
        assert!(!ran.load(AtomicOrdering::SeqCst));
    }

    #[tokio::test]
    async fn and_mode_needs_every_tag_and_or_mode_needs_any() {
        let keeper = keeper();
        keeper.set_concurrency("misc", 0).unwrap();
        keeper.set_concurrency("misc", 1).unwrap();

        let idle = |keeper: &Arc<QueueKeeper>| {
            let keeper = keeper.clone();
            tokio::spawn(async move {
                keeper
                    .add("misc", "never", async { Ok(json!(0)) }, TaskOptions::new().tags(&["p1", "summoner"]))
                    .await
            })
        };

        // hold the queue open without consuming it
        let gate = Arc::new(Barrier::new(2));
        let blocker = {
            let keeper = keeper.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                keeper
                    .add("misc", "blocker", async move {
                        gate.wait().await;
                        Ok(json!(0))
                    }, TaskOptions::new().tags(&["blocker"]))
                    .await
            })
        };

        while keeper.active_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        let waiter = idle(&keeper);
        while keeper.pending_count("misc") == 0 {
            tokio::task::yield_now().await;
        }

        assert_eq!(keeper.cancel_by_tags(&["p1", "other"], TagMode::And), 0, "both tags are required");
        assert_eq!(keeper.cancel_by_tags(&["summoner"], TagMode::Or), 1);
        waiter.await.unwrap().unwrap_err();

        gate.wait().await;
        blocker.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancelling_all_clears_everything_waiting() {
        let keeper = keeper();
        let gate = Arc::new(Barrier::new(10));
        let mut waiters = Vec::new();

        for index in 0..9 {
            let keeper = keeper.clone();
            let gate = gate.clone();
            waiters.push(tokio::spawn(async move {
                keeper
                    .add(
                        "misc",
                        &format!("task-{index}"),
                        async move {
                            gate.wait().await;
                            Ok(json!(index))
                        },
                        TaskOptions::new().tags(&["shared"]),
                    )
                    .await
            }));
        }

        while keeper.task_ids().len() < 9 {
            tokio::task::yield_now().await;
        }

        keeper.cancel_all();

        for waiter in waiters {
            assert!(waiter.await.unwrap().unwrap_err().is_abort());
        }

        assert!(keeper.task_ids().is_empty());
    }

    #[tokio::test]
    async fn a_task_that_fails_reports_the_message_without_the_abort_flag() {
        let failure = keeper()
            .add("misc", "boom", async { Err(TaskFailure::new("404")) }, TaskOptions::new())
            .await
            .unwrap_err();

        assert_eq!(failure.message, "404");
        assert!(!failure.aborted);
    }
}

//! Storage layer.
//!
//! All state mutations go through [`Store::mutate`], which acquires a
//! per-namespace lock, reads the current state, runs a synchronous closure
//! (the engine logic), writes the result, and releases the lock. This makes
//! concurrent events from many agents safe even across multiple backend
//! replicas, and works with a multiplexed Redis connection because the lock is
//! itself an ordinary atomic Redis command rather than connection state.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use redis::aio::ConnectionManager;

use crate::error::StoreError;
use crate::model::State;

pub type StoreResult<T> = Result<T, StoreError>;

const LOCK_TTL_MS: i64 = 10_000;
const LOCK_RETRY: Duration = Duration::from_millis(25);
const LOCK_MAX_ATTEMPTS: u32 = 200; // ~5s budget before giving up

/// Atomically delete the lock key only if we still own it.
const RELEASE_SCRIPT: &str = r"
if redis.call('get', KEYS[1]) == ARGV[1] then
  return redis.call('del', KEYS[1])
else
  return 0
end";

/// Storage backend. `Redis` is used in production; `Memory` backs tests.
pub enum Store {
    Redis(RedisStore),
    Memory(MemoryStore),
}

impl Store {
    /// Check backend liveness (used by the health endpoint).
    pub async fn ping(&self) -> StoreResult<()> {
        match self {
            Store::Redis(r) => r.ping().await,
            Store::Memory(_) => Ok(()),
        }
    }

    /// Run `f` against the namespace's state inside a locked critical section,
    /// persisting the result. Returns the final state and the closure's output.
    pub async fn mutate<F, T>(&self, ns: &str, f: F) -> StoreResult<(State, T)>
    where
        F: FnOnce(&mut State) -> T,
    {
        let token = self.acquire_lock(ns).await?;
        let result = self.run_locked(ns, f).await;
        // Best-effort release; the TTL is the backstop if this fails.
        self.release_lock(ns, &token).await;
        result
    }

    async fn run_locked<F, T>(&self, ns: &str, f: F) -> StoreResult<(State, T)>
    where
        F: FnOnce(&mut State) -> T,
    {
        let mut state = self.get_state(ns).await?;
        let out = f(&mut state);
        self.put_state(ns, &state).await?;
        Ok((state, out))
    }

    async fn get_state(&self, ns: &str) -> StoreResult<State> {
        match self {
            Store::Redis(r) => r.get_state(ns).await,
            Store::Memory(m) => Ok(m.get_state(ns)),
        }
    }

    async fn put_state(&self, ns: &str, state: &State) -> StoreResult<()> {
        match self {
            Store::Redis(r) => r.put_state(ns, state).await,
            Store::Memory(m) => {
                m.put_state(ns, state);
                Ok(())
            }
        }
    }

    async fn acquire_lock(&self, ns: &str) -> StoreResult<String> {
        match self {
            Store::Redis(r) => r.acquire_lock(ns).await,
            Store::Memory(_) => Ok(String::new()),
        }
    }

    async fn release_lock(&self, ns: &str, token: &str) {
        if let Store::Redis(r) = self {
            r.release_lock(ns, token).await;
        }
    }
}

/// Redis-backed store using a multiplexed [`ConnectionManager`].
pub struct RedisStore {
    conn: ConnectionManager,
}

impl RedisStore {
    pub fn new(conn: ConnectionManager) -> Self {
        RedisStore { conn }
    }

    fn state_key(ns: &str) -> String {
        format!("garlic:state:{ns}")
    }

    fn lock_key(ns: &str) -> String {
        format!("garlic:lock:{ns}")
    }

    async fn ping(&self) -> StoreResult<()> {
        let mut conn = self.conn.clone();
        let _: String = redis::cmd("PING").query_async(&mut conn).await?;
        Ok(())
    }

    async fn get_state(&self, ns: &str) -> StoreResult<State> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = redis::cmd("GET")
            .arg(Self::state_key(ns))
            .query_async(&mut conn)
            .await?;
        match raw {
            Some(json) => match serde_json::from_str::<State>(&json) {
                Ok(state) => Ok(state),
                Err(e) => {
                    // Corrupt or out-of-date payload: reset to defaults rather
                    // than wedging the user's hooks (mirrors garlic's behavior).
                    tracing::warn!(error = %e, namespace = %ns, "resetting corrupt state");
                    Ok(State::default())
                }
            },
            None => Ok(State::default()),
        }
    }

    async fn put_state(&self, ns: &str, state: &State) -> StoreResult<()> {
        let json = serde_json::to_string(state)?;
        let mut conn = self.conn.clone();
        let _: () = redis::cmd("SET")
            .arg(Self::state_key(ns))
            .arg(json)
            .query_async(&mut conn)
            .await?;
        Ok(())
    }

    async fn acquire_lock(&self, ns: &str) -> StoreResult<String> {
        let token = uuid::Uuid::new_v4().to_string();
        let key = Self::lock_key(ns);
        for _ in 0..LOCK_MAX_ATTEMPTS {
            let mut conn = self.conn.clone();
            let ok: Option<String> = redis::cmd("SET")
                .arg(&key)
                .arg(&token)
                .arg("NX")
                .arg("PX")
                .arg(LOCK_TTL_MS)
                .query_async(&mut conn)
                .await?;
            if ok.is_some() {
                return Ok(token);
            }
            tokio::time::sleep(LOCK_RETRY).await;
        }
        Err(StoreError::LockTimeout)
    }

    async fn release_lock(&self, ns: &str, token: &str) {
        let key = Self::lock_key(ns);
        let mut conn = self.conn.clone();
        let res: Result<i64, _> = redis::cmd("EVAL")
            .arg(RELEASE_SCRIPT)
            .arg(1)
            .arg(&key)
            .arg(token)
            .query_async(&mut conn)
            .await;
        if let Err(e) = res {
            tracing::warn!(error = %e, namespace = %ns, "failed to release lock; TTL will reclaim it");
        }
    }
}

/// In-memory store for tests. Locking is a no-op: tests drive each namespace
/// sequentially, so there is no real contention to serialize.
#[derive(Default)]
pub struct MemoryStore {
    data: StdMutex<HashMap<String, State>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore::default()
    }

    fn get_state(&self, ns: &str) -> State {
        self.data
            .lock()
            .unwrap()
            .get(ns)
            .cloned()
            .unwrap_or_default()
    }

    fn put_state(&self, ns: &str, state: &State) {
        self.data
            .lock()
            .unwrap()
            .insert(ns.to_string(), state.clone());
    }
}

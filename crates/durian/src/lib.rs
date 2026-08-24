#![cfg_attr(not(test), deny(clippy::unwrap_used))]
//! Bounded in-RAM sliding-window failure store + exponential soft-lock
//! backoff.
//!
//! One [`ThrottleStore<K>`] is one throttle *bucket*: a mutex-wrapped LRU
//! cache from key to failure timestamps. The structural properties — all
//! load-bearing against attacker-supplied keys — live in this type:
//!
//! - **Bounded entry count.** The LRU capacity caps total entries; an
//!   attacker spraying distinct keys (usernames, IPs) churns the LRU
//!   instead of growing memory without bound.
//! - **Bounded per-key history.** Each key's timestamp Vec is capped
//!   (`history_per_key`); the soft-lock delay saturates anyway, so extra
//!   timestamps add memory pressure without signal.
//! - **Sliding-window counting.** [`current_delay`](ThrottleStore::current_delay)
//!   counts only timestamps fresher than the caller's window, pruning
//!   stale ones in place. Reads use `peek_mut` (no recency bump) so
//!   entries without fresh writes age out via LRU eviction; writes use
//!   `get_or_insert_mut` (recency bump) so active attackers keep their
//!   own entry hot until the window expires.
//! - **The soft-lock backoff curve.** Over-budget counts map to delays of
//!   1, 2, 4, 8, 16, then a 30 s cap. Soft-lock (sleep, then proceed)
//!   rather than hard rejection is the semantic this curve encodes:
//!   legitimate users get in eventually; attackers can't lock them out
//!   by spamming their key.
//!
//! What stays with the caller: bucket *topology* (which axes exist and
//! their isolation from each other), the per-axis windows and budgets,
//! and the scheduling of [`sweep`](ThrottleStore::sweep) (needed so an
//! idle process doesn't carry ancient entries indefinitely — read-time
//! pruning only fires when a key is read again).

use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lru::LruCache;

/// One throttle bucket. See crate docs for the structural guarantees.
pub struct ThrottleStore<K: Eq + Hash> {
    cache: Mutex<LruCache<K, Vec<Instant>>>,
    history_per_key: usize,
}

impl<K: Eq + Hash> ThrottleStore<K> {
    /// `capacity` bounds the entry count (LRU eviction past it);
    /// `history_per_key` caps each key's timestamp Vec.
    ///
    /// Panics if `capacity == 0` — construction is static-init /
    /// startup-time and a zero-capacity throttle must be loud.
    pub fn new(capacity: usize, history_per_key: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).expect("ThrottleStore capacity must be > 0");
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            history_per_key,
        }
    }

    /// Read the count of fresh timestamps for `key`, pruning stale entries
    /// in place. Uses `peek_mut` so the read does NOT bump recency —
    /// entries without fresh writes naturally age out via LRU eviction.
    ///
    /// If the Vec becomes empty after pruning, the entry is popped to free
    /// the slot for a hotter key.
    fn count_fresh(&self, key: &K, window: Duration) -> u32 {
        let now = Instant::now();
        let mut guard = self.cache.lock().expect("throttle mutex");
        if let Some(v) = guard.peek_mut(key) {
            v.retain(|t| now.duration_since(*t) < window);
            let count = v.len() as u32;
            if v.is_empty() {
                guard.pop(key);
            }
            count
        } else {
            0
        }
    }

    /// Soft-lock delay for the NEXT attempt against `key`: counts fresh
    /// failures in `window`, and once `(count + 1)` exceeds `budget`, maps
    /// the overage onto the backoff curve (1 s, 2 s, 4 s, 8 s, 16 s, then
    /// capped at 30 s). `None` within budget.
    pub fn current_delay(&self, key: &K, window: Duration, budget: u32) -> Option<Duration> {
        let fresh = self.count_fresh(key, window);
        let over = (fresh + 1).saturating_sub(budget);
        delay_from_over(over)
    }

    /// Append now to `key`'s history, truncating to `history_per_key`.
    /// Uses `get_or_insert_mut` which bumps recency — active attackers
    /// keep their entry hot in the LRU, ensuring the throttle bucket
    /// survives until the sliding window expires (the caller's sweep
    /// schedule handles the idle case).
    pub fn record(&self, key: K) {
        self.record_instant(key, Instant::now());
    }

    fn record_instant(&self, key: K, at: Instant) {
        let mut guard = self.cache.lock().expect("throttle mutex");
        let v = guard.get_or_insert_mut(key, Vec::new);
        v.push(at);
        if v.len() > self.history_per_key {
            let drop_count = v.len() - self.history_per_key;
                                                                    
            v.drain(0..drop_count);
        }
    }

    /// Test-only: record a timestamp other than now (fake-stale injection
    /// for sweep tests). Deliberately not pub — production records now.
    #[cfg(test)]
    fn record_at(&self, key: K, at: Instant) {
        self.record_instant(key, at);
    }

    /// Full sweep: iterate all entries, prune timestamps older than
    /// `window`, pop entries left empty. Schedule periodically — read-time
    /// pruning alone never fires for keys that are never read again.
    pub fn sweep(&self, window: Duration)
    where
        K: Clone,
    {
        let now = Instant::now();
        let mut guard = self.cache.lock().expect("throttle mutex");
                                                                           
                                                                    
                                 
        let keys: Vec<K> = guard.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            if let Some(v) = guard.peek_mut(&k) {
                v.retain(|t| now.duration_since(*t) < window);
                if v.is_empty() {
                    guard.pop(&k);
                }
            }
        }
    }

    /// Entry count (LRU-bound introspection for tests/ops).
    pub fn len(&self) -> usize {
        self.cache.lock().expect("throttle mutex").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of timestamps currently stored for `key` — no pruning, no
    /// recency bump (peek). Test/ops introspection.
    pub fn recorded_len(&self, key: &K) -> usize {
        self.cache
            .lock()
            .expect("throttle mutex")
            .peek(key)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Drop every entry (test isolation).
    pub fn clear(&self) {
        self.cache.lock().expect("throttle mutex").clear();
    }
}

/// Convert an over-budget count into a soft-lock delay. 1 s, 2 s, 4 s,
/// 8 s, 16 s, then capped at 30 s. `over == 0` means no delay.
fn delay_from_over(over: u32) -> Option<Duration> {
    if over == 0 {
        return None;
    }
    let shift = (over - 1).min(5);
    let secs = (1u64 << shift).min(30);
    Some(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        let s: ThrottleStore<String> = ThrottleStore::new(50, 32);
        let window = Duration::from_secs(300);
        let budget = 5u32;
        let key = "k".to_string();
                                   
        assert!(s.current_delay(&key, window, budget).is_none());
        for _ in 0..budget {
            s.record(key.clone());
        }
                              
        let d = s
            .current_delay(&key, window, budget)
            .expect("first overage");
        assert_eq!(d.as_secs(), 1);
        s.record(key.clone());
                               
        let d = s
            .current_delay(&key, window, budget)
            .expect("second overage");
        assert_eq!(d.as_secs(), 2);
                                 
        for _ in 0..20 {
            s.record(key.clone());
        }
        let d = s.current_delay(&key, window, budget).expect("capped");
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn per_key_history_vec_caps_at_threshold() {
                                                                     
                                                                          
                                                     
        let s: ThrottleStore<String> = ThrottleStore::new(50, 32);
        let key = "k".to_string();
        for _ in 0..(32 + 50) {
            s.record(key.clone());
        }
        assert_eq!(
            s.recorded_len(&key),
            32,
            "per-key Vec must cap at history_per_key"
        );
                                                                      
        let d = s
            .current_delay(&key, Duration::from_secs(300), 5)
            .expect("at-cap should delay");
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn lru_eviction_bounds_attacker_supplied_key_growth() {
                                                                         
                                                                           
                                                                        
                                                                           
        let s: ThrottleStore<String> = ThrottleStore::new(50, 32);
        for i in 0..100 {
            s.record(format!("key-{i}"));
        }
        assert_eq!(s.len(), 50, "entry count must cap at LRU capacity");
        assert_eq!(
            s.recorded_len(&"key-0".to_string()),
            0,
            "earliest key must be evicted"
        );
        assert_eq!(
            s.recorded_len(&"key-99".to_string()),
            1,
            "most recent key must be present"
        );
    }

    #[test]
    fn sweep_prunes_stale_timestamps() {
                                                                          
                                                             
        let s: ThrottleStore<String> = ThrottleStore::new(50, 32);
        let window = Duration::from_secs(300);
        let stale = Instant::now()
            .checked_sub(window + Duration::from_secs(1))
            .expect("Instant::now() can step back across the window");
        s.record_at("stale-key".to_string(), stale);
        assert_eq!(s.recorded_len(&"stale-key".to_string()), 1);
        s.sweep(window);
        assert_eq!(
            s.recorded_len(&"stale-key".to_string()),
            0,
            "stale timestamps must be pruned"
        );
        assert!(s.is_empty(), "emptied entry must be popped");
    }

    #[test]
    fn concurrent_record_preserves_count() {
                                                                           
                                                            
        let s: ThrottleStore<String> = ThrottleStore::new(1000, 32);
        std::thread::scope(|scope| {
            for i in 0..50 {
                let s = &s;
                scope.spawn(move || s.record(format!("ip-{i}")));
            }
        });
        for i in 0..50 {
            let key = format!("ip-{i}");
            assert_eq!(
                s.recorded_len(&key),
                1,
                "expected exactly 1 record for {key} (lost write?)"
            );
        }
    }
}

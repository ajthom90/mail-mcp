use crate::types::{AccountId, MessageId};
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Stub(String);

    fn id(s: &str) -> (AccountId, MessageId) {
        (AccountId::new(), MessageId::from(s))
    }

    #[test]
    fn put_and_get_returns_value() {
        let cache: MessageCache<Stub> =
            MessageCache::new(NonZeroUsize::new(4).unwrap(), Duration::from_secs(60));
        let key = id("a");
        cache.put(key.0, key.1.clone(), Stub("v".into()));
        assert_eq!(cache.get(key.0, &key.1), Some(Stub("v".into())));
    }

    #[test]
    fn miss_returns_none() {
        let cache: MessageCache<Stub> =
            MessageCache::new(NonZeroUsize::new(4).unwrap(), Duration::from_secs(60));
        let (a, m) = id("z");
        assert_eq!(cache.get(a, &m), None);
    }

    #[test]
    fn lru_eviction() {
        let cache: MessageCache<Stub> =
            MessageCache::new(NonZeroUsize::new(2).unwrap(), Duration::from_secs(60));
        let a = AccountId::new();
        cache.put(a, MessageId::from("1"), Stub("x".into()));
        cache.put(a, MessageId::from("2"), Stub("x".into()));
        cache.put(a, MessageId::from("3"), Stub("x".into())); // evicts "1"
        assert_eq!(cache.get(a, &MessageId::from("1")), None);
        assert_eq!(cache.get(a, &MessageId::from("2")), Some(Stub("x".into())));
        assert_eq!(cache.get(a, &MessageId::from("3")), Some(Stub("x".into())));
    }

    #[test]
    fn ttl_expiry() {
        let cache: MessageCache<Stub> =
            MessageCache::new(NonZeroUsize::new(4).unwrap(), Duration::from_millis(50));
        let (a, m) = id("a");
        cache.put(a, m.clone(), Stub("v".into()));
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(cache.get(a, &m), None);
    }

    #[test]
    fn invalidate_account_clears_only_that_account() {
        let cache: MessageCache<Stub> =
            MessageCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_secs(60));
        let a = AccountId::new();
        let b = AccountId::new();
        cache.put(a, MessageId::from("1"), Stub("a1".into()));
        cache.put(b, MessageId::from("1"), Stub("b1".into()));
        cache.invalidate_account(a);
        assert_eq!(cache.get(a, &MessageId::from("1")), None);
        assert_eq!(cache.get(b, &MessageId::from("1")), Some(Stub("b1".into())));
    }
}

/// Bounded LRU cache keyed by `(AccountId, MessageId)`, with per-entry TTL.
pub struct MessageCache<V: Clone + Send + 'static> {
    inner: Mutex<lru::LruCache<(AccountId, MessageId), CacheEntry<V>>>,
    ttl: Duration,
}

struct CacheEntry<V> {
    value: V,
    inserted: Instant,
}

impl<V: Clone + Send + 'static> MessageCache<V> {
    pub fn new(capacity: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(lru::LruCache::new(capacity)),
            ttl,
        }
    }

    pub fn put(&self, account: AccountId, msg: MessageId, value: V) {
        let mut g = self.inner.lock().unwrap();
        g.put(
            (account, msg),
            CacheEntry {
                value,
                inserted: Instant::now(),
            },
        );
    }

    pub fn get(&self, account: AccountId, msg: &MessageId) -> Option<V> {
        let mut g = self.inner.lock().unwrap();
        let key = (account, msg.clone());
        if let Some(entry) = g.get(&key) {
            if entry.inserted.elapsed() <= self.ttl {
                return Some(entry.value.clone());
            }
        }
        g.pop(&key);
        None
    }

    pub fn invalidate_account(&self, account: AccountId) {
        let mut g = self.inner.lock().unwrap();
        let to_remove: Vec<_> = g
            .iter()
            .filter_map(|(k, _)| {
                if k.0 == account {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        for k in to_remove {
            g.pop(&k);
        }
    }
}

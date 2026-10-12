use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Custom Snowflake epoch: 2026-01-01T00:00:00Z, in Unix milliseconds.
pub const EPOCH_MS: u64 = 1_767_225_600_000;

const WORKER_BITS: u32 = 10;
const SEQ_BITS: u32 = 12;
const SEQ_MASK: u64 = (1 << SEQ_BITS) - 1;
const MAX_WORKER: u16 = (1 << WORKER_BITS) - 1;

macro_rules! id_type {
    ($name:ident) => {
        /// Snowflake ID. Serialized as a decimal string so JavaScript clients keep full
        /// 64-bit precision.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub i64);

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map(Self).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(UserId);
id_type!(PostId);

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before 1970").as_millis() as u64
}

/// Unix-ms creation time encoded in a Snowflake ID.
pub fn id_timestamp_ms(id: i64) -> i64 {
    (id >> (WORKER_BITS + SEQ_BITS)) + EPOCH_MS as i64
}

/// Lock-free Snowflake generator: 41-bit ms timestamp | 10-bit worker | 12-bit sequence.
///
/// IDs are strictly increasing per generator. If the sequence for a millisecond is
/// exhausted, or the wall clock steps backwards, the generator borrows from the next
/// logical millisecond instead of blocking or producing duplicates.
pub struct IdGen {
    worker: u64,
    /// Packed `(logical_ms << SEQ_BITS) | seq` of the last issued ID.
    state: AtomicU64,
}

impl IdGen {
    pub fn new(worker: u16) -> Self {
        assert!(worker <= MAX_WORKER, "worker id must be 0..={MAX_WORKER}");
        Self { worker: worker as u64, state: AtomicU64::new(0) }
    }

    pub fn next(&self) -> i64 {
        let now = now_ms().saturating_sub(EPOCH_MS);
        let mut cur = self.state.load(Ordering::Relaxed);
        loop {
            let (ms, seq) = (cur >> SEQ_BITS, cur & SEQ_MASK);
            let next = if now > ms {
                now << SEQ_BITS
            } else if seq < SEQ_MASK {
                cur + 1
            } else {
                (ms + 1) << SEQ_BITS
            };
            match self.state.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => {
                    let ms = next >> SEQ_BITS;
                    return ((ms << (WORKER_BITS + SEQ_BITS)) | (self.worker << SEQ_BITS) | (next & SEQ_MASK)) as i64;
                }
                Err(actual) => cur = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn unique_and_increasing_across_threads() {
        let gen = Arc::new(IdGen::new(7));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let gen = gen.clone();
                std::thread::spawn(move || (0..50_000).map(|_| gen.next()).collect::<Vec<_>>())
            })
            .collect();
        let mut all = HashSet::new();
        for h in handles {
            let ids = h.join().unwrap();
            assert!(ids.windows(2).all(|w| w[0] < w[1]), "per-thread ids must increase");
            for id in ids {
                assert!(id > 0);
                assert_eq!((id >> SEQ_BITS) & MAX_WORKER as i64, 7);
                assert!(all.insert(id), "duplicate id {id}");
            }
        }
    }

    #[test]
    fn timestamp_roundtrip() {
        let before = now_ms() as i64;
        let id = IdGen::new(0).next();
        assert!((id_timestamp_ms(id) - before).abs() < 1_000);
    }

    #[test]
    fn serializes_as_string() {
        let id = PostId(9_007_199_254_740_993); // > 2^53
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"9007199254740993\"");
        assert_eq!(serde_json::from_str::<PostId>(&json).unwrap(), id);
    }
}

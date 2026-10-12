//! Per-IP token buckets (docs/SPEC.md §5). Node-local; a cluster moves the buckets
//! into Dragonfly.

use dashmap::DashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Bucket {
    pub burst: f64,
    pub per_sec: f64,
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub auth: Bucket,
    pub write: Bucket,
    pub read: Bucket,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            auth: Bucket { burst: 10.0, per_sec: 10.0 / 60.0 },
            write: Bucket { burst: 30.0, per_sec: 1.0 },
            read: Bucket { burst: 300.0, per_sec: 50.0 },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Auth,
    Write,
    Read,
}

struct State {
    tokens: f64,
    last: Instant,
}

pub struct RateLimiter {
    limits: Limits,
    buckets: DashMap<(Class, IpAddr), State>,
}

impl RateLimiter {
    pub fn new(limits: Limits) -> Self {
        Self { limits, buckets: DashMap::new() }
    }

    /// `Err(seconds)` until a token is available.
    pub fn check(&self, class: Class, ip: IpAddr) -> Result<(), u64> {
        let b = match class {
            Class::Auth => self.limits.auth,
            Class::Write => self.limits.write,
            Class::Read => self.limits.read,
        };
        let now = Instant::now();
        let mut entry = self.buckets.entry((class, ip)).or_insert(State { tokens: b.burst, last: now });
        let s = entry.value_mut();
        s.tokens = (s.tokens + now.duration_since(s.last).as_secs_f64() * b.per_sec).min(b.burst);
        s.last = now;
        if s.tokens >= 1.0 {
            s.tokens -= 1.0;
            Ok(())
        } else {
            Err(((1.0 - s.tokens) / b.per_sec).ceil() as u64)
        }
    }

    /// Drops buckets idle for longer than `idle` (they would be full again anyway).
    pub fn sweep(&self, idle: Duration) {
        let now = Instant::now();
        self.buckets.retain(|_, s| now.duration_since(s.last) < idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_reject() {
        let rl = RateLimiter::new(Limits { auth: Bucket { burst: 2.0, per_sec: 0.5 }, ..Limits::default() });
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(rl.check(Class::Auth, ip).is_ok());
        assert!(rl.check(Class::Auth, ip).is_ok());
        assert_eq!(rl.check(Class::Auth, ip), Err(2));
        assert!(rl.check(Class::Auth, "10.0.0.2".parse().unwrap()).is_ok(), "buckets are per IP");
        assert!(rl.check(Class::Read, ip).is_ok(), "buckets are per class");
    }
}

use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Token-bucket bandwidth governor.
/// `rate_bps = None` means Turbo / unlimited.
#[derive(Debug)]
pub struct Throttler {
    inner: Mutex<ThrottlerState>,
}

#[derive(Debug)]
struct ThrottlerState {
    rate_bps: Option<u64>,
    tokens: f64,
    last: Instant,
    max_burst: f64,
}

impl Throttler {
    pub fn new(rate_bps: Option<u64>) -> Self {
        let burst = rate_bps.map(|r| r as f64).unwrap_or(f64::MAX);
        Self {
            inner: Mutex::new(ThrottlerState {
                rate_bps,
                tokens: burst,
                last: Instant::now(),
                max_burst: burst,
            }),
        }
    }

    pub async fn set_rate(&self, rate_bps: Option<u64>) {
        let mut s = self.inner.lock().await;
        s.rate_bps = rate_bps;
        s.max_burst = rate_bps.map(|r| r as f64).unwrap_or(f64::MAX);
        s.tokens = s.max_burst.min(s.tokens + s.max_burst * 0.1);
        s.last = Instant::now();
    }

    /// Block until `n` bytes may be consumed under the cap.
    pub async fn acquire(&self, n: usize) {
        let rate = { self.inner.lock().await.rate_bps };
        let Some(rate) = rate else { return };
        if rate == 0 {
            return;
        }
        let mut need = n as f64;
        while need > 0.0 {
            let wait = {
                let mut s = self.inner.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(s.last).as_secs_f64();
                s.last = now;
                s.tokens = (s.tokens + elapsed * rate as f64).min(s.max_burst);
                if s.tokens >= need {
                    s.tokens -= need;
                    None
                } else {
                    let deficit = need - s.tokens;
                    need = deficit;
                    s.tokens = 0.0;
                    Some(Duration::from_secs_f64(deficit / rate as f64))
                }
            };
            match wait {
                None => break,
                Some(d) => tokio::time::sleep(d.min(Duration::from_millis(250))).await,
            }
        }
    }
}

/// Clamp a chunk to what the cap allows per 100ms tick (for UI pacing).
pub fn paced_chunk(rate_bps: Option<u64>, wanted: usize) -> usize {
    match rate_bps {
        None => wanted,
        Some(r) => ((r / 10).max(1024) as usize).min(wanted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paced_chunk_unlimited_passthrough() {
        assert_eq!(paced_chunk(None, 65536), 65536);
    }

    #[test]
    fn paced_chunk_floor() {
        // 5 MB/s -> 500KB per 100ms tick
        assert_eq!(paced_chunk(Some(5_000_000), 1_000_000), 500_000);
        // tiny caps still allow ≥1KB progress so streams never stall
        assert_eq!(paced_chunk(Some(100), 65536), 1024);
    }

    #[tokio::test]
    async fn unlimited_acquire_is_instant() {
        let t = Throttler::new(None);
        let start = Instant::now();
        t.acquire(10_000_000).await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}

//! What to do when a provider fails (docs/04 §8): jittered exponential
//! backoff for 5xx and transport errors, a circuit breaker per profile on
//! 429, fallback down the route chain, and no retry once a stream has
//! started delivering (a duplicated answer is worse than a failed one).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::provider::{Chunk, Completion, Provider, ProviderError, Request};

/// The decision for one failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Try the same provider again after this delay.
    Retry(Duration),
    /// Leave this provider alone (open its breaker) and try the next one.
    Fallback,
    /// The request itself is wrong; nobody else will do better.
    GiveUp,
}

/// Retry policy.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Attempts on one provider, first included.
    pub max_attempts: u32,
    /// First backoff; doubles per attempt.
    pub base: Duration,
    /// How long a breaker stays open after a 429.
    pub cooldown: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base: Duration::from_millis(500),
            cooldown: Duration::from_secs(60),
        }
    }
}

impl Policy {
    /// `base · 2^attempt` plus up to half a base of jitter, so a fleet of
    /// clients does not retry in lockstep.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let exp = self.base.saturating_mul(1u32 << attempt.min(6));
        let nanos = u128::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos()),
        );
        let jitter_ns = nanos % (self.base.as_nanos() / 2).max(1);
        exp + Duration::from_nanos(u64::try_from(jitter_ns).unwrap_or(0))
    }

    /// The verdict for `err` after `attempt` attempts (0-based) on one provider.
    pub fn classify(&self, err: &ProviderError, attempt: u32) -> Verdict {
        let more = attempt + 1 < self.max_attempts;
        match err {
            ProviderError::Status { status: 429, .. } => Verdict::Fallback,
            ProviderError::Status { status, .. } if *status >= 500 => {
                if more {
                    Verdict::Retry(self.backoff(attempt))
                } else {
                    Verdict::Fallback
                }
            }
            ProviderError::Transport { .. } => {
                if more {
                    Verdict::Retry(self.backoff(attempt))
                } else {
                    Verdict::Fallback
                }
            }
            ProviderError::Status { .. }
            | ProviderError::MissingKey { .. }
            | ProviderError::UnknownModel { .. }
            | ProviderError::Protocol { .. } => Verdict::GiveUp,
        }
    }
}

/// Open breakers by profile name.
#[derive(Debug, Default)]
pub struct Breakers {
    open: HashMap<String, Instant>,
}

impl Breakers {
    /// Opens `profile` until `now + cooldown`.
    pub fn open(&mut self, profile: &str, cooldown: Duration) {
        self.open
            .insert(profile.to_owned(), Instant::now() + cooldown);
    }

    /// Time left on the breaker, or `None` when the profile may be used.
    pub fn cooling(&mut self, profile: &str) -> Option<Duration> {
        let until = *self.open.get(profile)?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            self.open.remove(profile);
            None
        } else {
            Some(left)
        }
    }

    /// Whether `profile` is open.
    pub fn is_open(&mut self, profile: &str) -> bool {
        self.cooling(profile).is_some()
    }
}

/// The outcome of [`stream_with_retry`].
#[derive(Debug)]
pub struct Attempted {
    /// The completion.
    pub completion: Completion,
    /// Attempts it took, first included.
    pub attempts: u32,
}

/// Streams with retries per `policy`. Once a chunk has reached `on_chunk`
/// no retry happens: the caller already showed part of an answer.
pub fn stream_with_retry(
    provider: &dyn Provider,
    req: &Request,
    policy: &Policy,
    on_chunk: &mut dyn FnMut(Chunk),
) -> Result<Attempted, (ProviderError, Verdict)> {
    let mut attempt = 0;
    loop {
        let mut emitted = false;
        let result = provider.stream(req, &mut |chunk| {
            emitted = true;
            on_chunk(chunk);
        });
        match result {
            Ok(completion) => {
                return Ok(Attempted {
                    completion,
                    attempts: attempt + 1,
                });
            }
            Err(err) => {
                let verdict = if emitted {
                    Verdict::GiveUp
                } else {
                    policy.classify(&err, attempt)
                };
                match verdict {
                    Verdict::Retry(delay) => {
                        std::thread::sleep(delay);
                        attempt += 1;
                    }
                    other => return Err((err, other)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderCaps, StopReason, Usage};
    use std::sync::Mutex;

    struct Flaky {
        failures: Mutex<Vec<ProviderError>>,
        calls: Mutex<u32>,
    }

    impl Provider for Flaky {
        fn name(&self) -> &'static str {
            "flaky"
        }

        fn caps(&self, _: &str) -> Option<ProviderCaps> {
            None
        }

        fn stream(
            &self,
            _: &Request,
            on_chunk: &mut dyn FnMut(Chunk),
        ) -> Result<Completion, ProviderError> {
            *self.calls.lock().unwrap() += 1;
            if let Some(e) = self.failures.lock().unwrap().pop() {
                return Err(e);
            }
            on_chunk(Chunk::TextDelta("ok".into()));
            Ok(Completion {
                content: vec![],
                usage: Usage::default(),
                stop: StopReason::EndTurn,
            })
        }

        fn models(&self) -> Result<Vec<String>, ProviderError> {
            Ok(vec![])
        }
    }

    fn status(code: u16) -> ProviderError {
        ProviderError::Status {
            provider: "p".into(),
            status: code,
            body: String::new(),
        }
    }

    fn req() -> Request {
        Request {
            model: "m".into(),
            system: None,
            messages: vec![],
            tools: vec![],
            max_tokens: 1,
            temperature: None,
            top_p: None,
            stop: vec![],
        }
    }

    #[test]
    fn verdicts_follow_docs_04() {
        let p = Policy::default();
        assert_eq!(p.classify(&status(429), 0), Verdict::Fallback);
        assert!(matches!(p.classify(&status(503), 0), Verdict::Retry(_)));
        assert!(matches!(p.classify(&status(529), 1), Verdict::Retry(_)));
        assert_eq!(
            p.classify(&status(500), 2),
            Verdict::Fallback,
            "the third failure moves on"
        );
        assert_eq!(p.classify(&status(400), 0), Verdict::GiveUp);
        assert_eq!(
            p.classify(
                &ProviderError::MissingKey {
                    profile: "x".into()
                },
                0
            ),
            Verdict::GiveUp
        );
        let d0 = p.backoff(0);
        let d1 = p.backoff(1);
        assert!(d0 >= p.base && d0 < p.base * 2, "{d0:?}");
        assert!(d1 >= p.base * 2 && d1 < p.base * 3, "{d1:?}");
    }

    #[test]
    fn breakers_open_and_expire() {
        let mut b = Breakers::default();
        assert!(!b.is_open("a"));
        b.open("a", Duration::from_millis(30));
        assert!(b.is_open("a"));
        assert!(b.cooling("a").is_some());
        std::thread::sleep(Duration::from_millis(40));
        assert!(!b.is_open("a"));
    }

    #[test]
    fn retries_then_succeeds_but_never_after_a_chunk() {
        let policy = Policy {
            max_attempts: 3,
            base: Duration::from_millis(1),
            cooldown: Duration::from_secs(1),
        };
        let flaky = Flaky {
            failures: Mutex::new(vec![status(503), status(500)]),
            calls: Mutex::new(0),
        };
        let mut chunks = 0;
        let out = stream_with_retry(&flaky, &req(), &policy, &mut |_| chunks += 1).unwrap();
        assert_eq!(out.attempts, 3);
        assert_eq!(chunks, 1);
        let rate_limited = Flaky {
            failures: Mutex::new(vec![status(429)]),
            calls: Mutex::new(0),
        };
        let (err, verdict) =
            stream_with_retry(&rate_limited, &req(), &policy, &mut |_| {}).unwrap_err();
        assert_eq!(verdict, Verdict::Fallback);
        assert!(matches!(err, ProviderError::Status { status: 429, .. }));
        assert_eq!(
            *rate_limited.calls.lock().unwrap(),
            1,
            "no retry on the same provider after 429"
        );
    }
}

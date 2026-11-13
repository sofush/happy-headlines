use std::{
    future::Future,
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use tokio::sync::Mutex;

#[derive(Clone)]
pub struct CircuitBreaker {
    number_of_failures_before_tripping: usize,
    time_before_probing_starts: Duration,
    probe_attempts: usize,
    state: Arc<Mutex<CircuitBreakerState>>,
}

pub enum CircuitBreakerValue<T, E> {
    Ok(T),
    Err(E),
    Unavailable,
}

enum CircuitBreakerState {
    Closed { consecutive_failures: usize },
    Probing { probe_attempts: usize },
    Open { opened_at: Instant },
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            number_of_failures_before_tripping: 5,
            time_before_probing_starts: Duration::from_secs(10),
            probe_attempts: 5,
            state: Arc::new(Mutex::new(CircuitBreakerState::Closed {
                consecutive_failures: 0,
            })),
        }
    }
}

impl<T, E> From<Result<T, E>> for CircuitBreakerValue<T, E> {
    fn from(value: Result<T, E>) -> Self {
        match value {
            Ok(t) => CircuitBreakerValue::Ok(t),
            Err(e) => CircuitBreakerValue::Err(e),
        }
    }
}

#[derive(Clone, Copy)]
enum CallMode {
    Closed,
    Probing,
}

impl CircuitBreaker {
    pub async fn call_async<E, T, Fut, F, DF>(
        &self,
        func: F,
        determine_failure: DF,
    ) -> CircuitBreakerValue<T, E>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        DF: Fn(&Result<T, E>) -> bool + Send + Sync,
    {
        let Some(mode) = self.begin_call().await else {
            return CircuitBreakerValue::Unavailable;
        };

        let res = func().await;
        let is_failure = determine_failure(&res);

        self.record_result(mode, is_failure).await;

        res.into()
    }

    async fn begin_call(&self) -> Option<CallMode> {
        let mut state = self.state.lock().await;

        match &mut *state {
            CircuitBreakerState::Closed { .. } => Some(CallMode::Closed),
            CircuitBreakerState::Probing { .. } => Some(CallMode::Probing),
            CircuitBreakerState::Open { opened_at } => {
                if Instant::now() < *opened_at + self.time_before_probing_starts
                {
                    return None;
                }

                *state = CircuitBreakerState::Probing { probe_attempts: 0 };
                Some(CallMode::Probing)
            }
        }
    }

    async fn record_result(&self, mode: CallMode, is_failure: bool) {
        let mut state = self.state.lock().await;

        match mode {
            CallMode::Closed => {
                let should_open = {
                    let CircuitBreakerState::Closed {
                        consecutive_failures,
                    } = &mut *state
                    else {
                        return;
                    };

                    if is_failure {
                        *consecutive_failures += 1;
                    } else {
                        *consecutive_failures = 0;
                    }

                    *consecutive_failures
                        >= self.number_of_failures_before_tripping
                };

                if should_open {
                    *state = CircuitBreakerState::Open {
                        opened_at: Instant::now(),
                    };
                }
            }
            CallMode::Probing => {
                let next_state = {
                    let CircuitBreakerState::Probing { probe_attempts } =
                        &mut *state
                    else {
                        return;
                    };

                    if is_failure {
                        let next_probe_attempt = *probe_attempts + 1;

                        if next_probe_attempt >= self.probe_attempts {
                            Some(CircuitBreakerState::Open {
                                opened_at: Instant::now(),
                            })
                        } else {
                            *probe_attempts = next_probe_attempt;
                            None
                        }
                    } else {
                        Some(CircuitBreakerState::Closed {
                            consecutive_failures: 0,
                        })
                    }
                };

                if let Some(next_state) = next_state {
                    *state = next_state;
                }
            }
        }
    }
}

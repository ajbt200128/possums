//! Own verified generations without changing an admitted operation's client.
use super::{InferenceError, InferenceFailure};
use std::{
    future::Future,
    sync::{Arc, PoisonError, RwLock},
};
use tokio::sync::watch;

#[cfg(test)]
#[path = "verified_client_tests.rs"]
mod tests;

type Outcome = Option<Result<(), InferenceFailure>>;

struct State<C> {
    client: Arc<C>,
    renewal_needed: bool,
    attempt: Option<watch::Receiver<Outcome>>,
}

enum Acquisition<C> {
    Ready(Arc<C>),
    Join(watch::Receiver<Outcome>),
    Verify(Arc<C>, watch::Sender<Outcome>),
}

pub(super) struct VerifiedClient<C> {
    state: RwLock<State<C>>,
}

impl<C> VerifiedClient<C> {
    pub(super) fn new(client: C) -> Self {
        Self {
            state: RwLock::new(State {
                client: Arc::new(client),
                renewal_needed: false,
                attempt: None,
            }),
        }
    }

    /// Last completed verification, not authorization for a new dispatch.
    pub(super) fn last_verified(&self) -> Arc<C> {
        Arc::clone(
            &self
                .state
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .client,
        )
    }

    pub(super) fn invalidate(&self, observed: &Arc<C>) {
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if Arc::ptr_eq(observed, &state.client) {
            state.renewal_needed = true;
        }
    }

    fn begin(&self) -> Acquisition<C> {
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if !state.renewal_needed {
            return Acquisition::Ready(Arc::clone(&state.client));
        }
        if let Some(attempt) = &state.attempt {
            if attempt.borrow().is_none() && attempt.has_changed().is_ok() {
                return Acquisition::Join(attempt.clone());
            }
        }
        let (completion, attempt) = watch::channel(None);
        state.attempt = Some(attempt);
        Acquisition::Verify(Arc::clone(&state.client), completion)
    }

    pub(super) async fn acquire<F, Fut>(&self, verify: F) -> Result<Arc<C>, InferenceError>
    where
        F: FnOnce(Arc<C>) -> Fut,
        Fut: Future<Output = Result<C, InferenceError>>,
    {
        match self.begin() {
            Acquisition::Ready(client) => Ok(client),
            Acquisition::Join(mut attempt) => {
                loop {
                    let outcome = *attempt.borrow_and_update();
                    if let Some(outcome) = outcome {
                        outcome.map_err(InferenceError::from)?;
                        break;
                    }
                    // Dropping the owner cancels verification, closes this exact
                    // attempt and leaves dispatch gated. No detached task remains.
                    attempt
                        .changed()
                        .await
                        .map_err(|_| InferenceError::from(InferenceFailure::VerificationFailed))?;
                }
                let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
                if state.renewal_needed {
                    return Err(InferenceFailure::VerificationFailed.into());
                }
                Ok(Arc::clone(&state.client))
            }
            Acquisition::Verify(previous, completion) => match verify(previous).await {
                Ok(candidate) => {
                    let candidate = Arc::new(candidate);
                    {
                        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
                        state.client = Arc::clone(&candidate);
                        state.renewal_needed = false;
                    }
                    completion.send_replace(Some(Ok(())));
                    Ok(candidate)
                }
                Err(error) => {
                    completion.send_replace(Some(Err(error.failure())));
                    Err(error)
                }
            },
        }
    }
}

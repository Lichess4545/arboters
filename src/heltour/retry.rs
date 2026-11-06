use std::error::Error;

use backon::{ExponentialBuilder, Retryable};
use reqwest::StatusCode;

use super::RetryConfig;

pub trait Failure: Error {
    fn may_pass_on_its_own(&self) -> bool;
}

impl Failure for reqwest::Error {
    fn may_pass_on_its_own(&self) -> bool {
        self.is_request() || self.is_body() || self.status().is_some_and(may_pass_on_its_own)
    }
}

fn may_pass_on_its_own(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

pub async fn retrying<T, E: Failure, F: Future<Output = Result<T, E>>>(
    config: RetryConfig,
    attempt: impl FnMut() -> F,
) -> Result<T, E> {
    attempt
        .retry(backoff(config))
        .sleep(tokio::time::sleep)
        .when(E::may_pass_on_its_own)
        .notify(|failure, delay| {
            tracing::warn!(error = %failure, cause = ?failure.source(), retry_in = ?delay, "Heltour request failed");
        })
        .await
}

const fn backoff(config: RetryConfig) -> ExponentialBuilder {
    ExponentialBuilder::new()
        .with_min_delay(config.initial_backoff)
        .without_max_delay()
        .with_max_times(config.retries)
}

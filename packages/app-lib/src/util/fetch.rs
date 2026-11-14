//! Functions for fetching information from the Internet
use super::io::{self, IOError};
use crate::event::LoadingBarId;
use crate::event::emit::emit_loading;
use crate::{ErrorKind, LabrinthError};
use bytes::Bytes;
use chrono::{DateTime, TimeDelta, Utc};
use eyre::{Context, eyre};
use governor::clock::{Clock, DefaultClock};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use parking_lot::Mutex;
use rand::Rng;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::num::NonZeroU32;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Semaphore;
use tokio::{fs::File, io::AsyncReadExt, io::AsyncWriteExt};
use tracing::{debug, info};

pub const DOWNLOAD_META_HEADER: &str = "modrinth-download-meta";

#[derive(Debug, derive_more::Display, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[display(rename_all = "snake_case")]
pub enum DownloadReason {
    Standalone,
    Dependency,
    Modpack,
    Update,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadMeta {
    pub reason: DownloadReason,
    pub game_version: String,
    pub loader: String,
    pub dependent_on: Option<String>,
}

impl DownloadMeta {
    pub fn to_header_value(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct IoSemaphore(pub Semaphore);
#[derive(Debug)]
pub struct FetchSemaphore(pub Semaphore);

struct FetchFence {
    inner: Mutex<HashMap<&'static str, FenceInner>>,
}

impl FetchFence {
    pub fn is_blocked(&self, key: &'static str) -> bool {
        self.inner
            .lock()
            .entry(key)
            .or_insert_with(FenceInner::new)
            .is_blocked()
    }

    pub fn record_ok(&self, key: &'static str) {
        self.inner
            .lock()
            .entry(key)
            .or_insert_with(FenceInner::new)
            .record_ok()
    }

    pub fn record_fail(&self, key: &'static str) {
        self.inner
            .lock()
            .entry(key)
            .or_insert_with(FenceInner::new)
            .record_fail()
    }

    pub fn latest_block_minutes(&self) -> u32 {
        let now = Utc::now();

        self.inner
            .lock()
            .values()
            .filter_map(|fence| fence.block_until)
            .filter(|until| *until > now)
            .max()
            .map(|until| {
                let seconds = until.signed_duration_since(now).num_seconds();
                (seconds.max(0) as u32).div_ceil(60).max(1)
            })
            .unwrap_or(1)
    }
}

struct FenceInner {
    failures: VecDeque<DateTime<Utc>>,
    block_until: Option<DateTime<Utc>>,
    block_factor: i32,
}

impl FenceInner {
    const FAILURE_WINDOW: TimeDelta = TimeDelta::minutes(3);
    const FAILURE_THRESHOLD: usize = 4;
    const BLOCK_DURATION_MIN_BASE: TimeDelta = TimeDelta::minutes(2);
    const BLOCK_DURATION_MAX_BASE: TimeDelta = TimeDelta::minutes(5);
    const BLOCK_DURATION_MAX_FACTOR: i32 = 3;

    pub fn new() -> Self {
        Self {
            failures: VecDeque::new(),
            block_until: None,
            block_factor: 0,
        }
    }

    pub fn is_blocked(&mut self) -> bool {
        if let Some(until) = self.block_until {
            if until > Utc::now() {
                return true;
            } else {
                self.block_until = None;
            }
        }

        false
    }

    pub fn record_ok(&mut self) {
        self.prune(Utc::now());
    }

    pub fn record_fail(&mut self) {
        self.prune(Utc::now());
        self.failures.push_back(Utc::now());

        if self.failures.len() >= Self::FAILURE_THRESHOLD {
            self.trigger_block();
        }
    }

    /// Blocks further requests for a random duration between the min and max base durations, scaled by a factor
    /// of how many blocks have been triggered in this session.
    ///
    /// As such, for the first block, the duration will be between 2 and 5 minutes.
    /// - For the second block, between 4 and 10 minutes.
    /// - For the third block and any further blocks, between 6 and 15 minutes.
    fn trigger_block(&mut self) {
        self.block_factor =
            i32::min(self.block_factor + 1, Self::BLOCK_DURATION_MAX_FACTOR);

        let min = Self::BLOCK_DURATION_MIN_BASE
            .checked_mul(self.block_factor)
            .unwrap_or(Self::BLOCK_DURATION_MIN_BASE);
        let max = Self::BLOCK_DURATION_MAX_BASE
            .checked_mul(self.block_factor)
            .unwrap_or(Self::BLOCK_DURATION_MAX_BASE);

        let delta_seconds = (max - min).as_seconds_f64()
            * rand::thread_rng().gen_range(0.0..=1.0);
        let duration =
            min + TimeDelta::milliseconds((delta_seconds * 1000.0) as i64);

        self.block_until = Some(Utc::now() + duration);
    }

    /// Removes all failure points older than the failure window
    fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Self::FAILURE_WINDOW;

        while let Some(&front) = self.failures.front() {
            if front < cutoff {
                self.failures.pop_front();
            } else {
                break;
            }
        }
    }
}

static GLOBAL_FETCH_FENCE: LazyLock<FetchFence> =
    LazyLock::new(|| FetchFence {
        inner: Mutex::new(HashMap::new()),
    });

const API_RETRY_AFTER_FALLBACK: Duration = Duration::from_secs(60);

// This means the unit recovery time will be:
// replenish one unit time in seconds = (60 / (units recovered per minute))
// smooth recovery time = replenish one unit time in seconds * API_RATE_LIMIT_RECOVERY_SIZE.
//
// At 20 it means (60 / 120) * 20 = 10 seconds.
const API_RATE_LIMIT_RECOVERY_SIZE: u32 = 20;

/// Implements request-rate-limit handling as well as a local rate limiter of 120 RPM + 50 burst.
struct ApiRateLimit {
    block_until: Mutex<Option<Instant>>,
    check_lock: Mutex<()>,
    local: Arc<DefaultDirectRateLimiter>,
    recovery_padding: Duration,
}

impl ApiRateLimit {
    fn new() -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(120).unwrap())
            .allow_burst(NonZeroU32::new(50).unwrap());
        let recovery_size =
            API_RATE_LIMIT_RECOVERY_SIZE.min(quota.burst_size().get());

        Self {
            block_until: Mutex::new(None),
            check_lock: Mutex::new(()),
            local: Arc::new(RateLimiter::direct(quota)),
            recovery_padding: quota
                .replenish_interval()
                .saturating_mul(recovery_size.saturating_sub(1)),
        }
    }

    fn check(&self) -> crate::Result<()> {
        let _check_guard = self.check_lock.lock();
        self.ensure_not_blocked()?;

        if let Err(not_until) = self.local.check() {
            // Adds hysteresis to the rate limiting system, ensuring recovery happens
            // for longer but for more units, avoiding the "flapping" effect when running
            // out of units.

            let retry_after = not_until
                .wait_time_from(DefaultClock::default().now())
                .saturating_add(self.recovery_padding);
            info!(
                ?retry_after,
                "Hit builtin rate limiter; waiting for recovery"
            );
            self.block_for(retry_after);

            let retry_in_seconds = self
                .retry_in_seconds()
                .unwrap_or_else(|| duration_seconds_ceil(retry_after));

            return Err(ErrorKind::Ratelimited { retry_in_seconds }.into());
        }

        self.ensure_not_blocked()
    }

    fn handle_response(
        &self,
        response: &reqwest::Response,
    ) -> Option<ErrorKind> {
        if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return None;
        }

        debug!("Received 429 response; blocking");

        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| parse_retry_after(value, SystemTime::now()))
            .unwrap_or(API_RETRY_AFTER_FALLBACK);

        self.block_for(retry_after);

        Some(ErrorKind::Ratelimited {
            retry_in_seconds: self.retry_in_seconds().unwrap_or(0),
        })
    }

    fn ensure_not_blocked(&self) -> crate::Result<()> {
        if let Some(retry_in_seconds) = self.retry_in_seconds() {
            debug!("Hit builtin rate limiter; blocking");
            return Err(ErrorKind::Ratelimited { retry_in_seconds }.into());
        }

        Ok(())
    }

    fn block_for(&self, duration: Duration) {
        let Some(block_until) = Instant::now().checked_add(duration) else {
            return;
        };
        let mut current_block = self.block_until.lock();

        if current_block.is_none_or(|current| current < block_until) {
            *current_block = Some(block_until);
        }
    }

    fn retry_in_seconds(&self) -> Option<u64> {
        let mut block_until = self.block_until.lock();
        let remaining = (*block_until)?.checked_duration_since(Instant::now());

        if let Some(remaining) = remaining
            && !remaining.is_zero()
        {
            return Some(duration_seconds_ceil(remaining));
        }

        *block_until = None;
        None
    }
}

static GLOBAL_API_RATE_LIMIT: LazyLock<ApiRateLimit> =
    LazyLock::new(ApiRateLimit::new);

fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    let retry_at = httpdate::parse_http_date(value).ok()?;
    Some(retry_at.duration_since(now).unwrap_or(Duration::ZERO))
}

fn duration_seconds_ceil(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
}

fn reqwest_client_builder_with_timeout() -> reqwest::ClientBuilder {
    reqwest_client_builder().read_timeout(Duration::from_secs(30))
}

fn reqwest_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // Raised from the default: a cold API on a slow link can take well over a
        // minute for the first byte, and the caller has its own retry budget.
        .read_timeout(Duration::from_secs(120))
        .tcp_keepalive(Some(Duration::from_secs(10)))
        .user_agent(crate::launcher_user_agent())
}

pub static INSECURE_REQWEST_CLIENT: LazyLock<reqwest::Client> =
    LazyLock::new(|| {
        reqwest_client_builder_with_timeout()
            .build()
            .expect("client configuration should be valid")
    });

pub static REQWEST_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest_client_builder_with_timeout()
        .https_only(true)
        .build()
        .expect("client configuration should be valid")
});

pub static INSECURE_NO_TIMEOUT_REQWEST_CLIENT: LazyLock<reqwest::Client> =
    LazyLock::new(|| {
        reqwest_client_builder()
            .build()
            .expect("client configuration should be valid")
    });

pub static NO_TIMEOUT_REQWEST_CLIENT: LazyLock<reqwest::Client> =
    LazyLock::new(|| {
        reqwest_client_builder()
            .https_only(true)
            .build()
            .expect("client configuration should be valid")
    });

/// A client that does not follow redirects, so a caller can check each hop's
/// host itself (see [`send_checked`]). `https_only` is kept, matching
/// [`REQWEST_CLIENT`], so this does not widen what a caller may reach.
pub static NO_REDIRECT_REQWEST_CLIENT: LazyLock<reqwest::Client> =
    LazyLock::new(|| {
        reqwest_client_builder_with_timeout()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client configuration should be valid")
    });

/// How many redirects [`send_checked`] will follow before giving up.
const MAX_REDIRECTS: usize = 10;

/// Send a request, following redirects by hand so that every hop's host can be
/// checked against an allowlist.
///
/// reqwest's default policy follows a `Location` without consulting the caller,
/// so a URL that passed a host check could bounce to a host that never would —
/// an allowlist that only guards the first request is not a boundary. Passing a
/// `Policy::none()` client (such as [`NO_REDIRECT_REQWEST_CLIENT`]) and looping
/// here keeps every hop subject to `check_host`.
///
/// `check_host` takes an owned `String` rather than a `&str` so the future it
/// returns does not borrow its argument; a `Fn(&str) -> Fut` bound cannot be
/// satisfied by a closure whose future borrows the input.
///
/// Redirect semantics follow RFC 9110: `303` always, and `301`/`302` for a
/// `POST`, switch to `GET` and drop the body; `307`/`308` preserve both.
pub async fn send_checked<F, Fut>(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: reqwest::Url,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    check_host: F,
) -> crate::Result<reqwest::Response>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = crate::Result<()>>,
{
    let mut method = method;
    let mut url = url;
    let mut body = body.map(str::to_string);

    for _ in 0..=MAX_REDIRECTS {
        let host = url.host_str().ok_or_else(|| {
            crate::ErrorKind::InputError("URL has no host".to_string())
        })?;
        check_host(host.to_string()).await?;

        let mut builder = client.request(method.clone(), url.clone());
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = &body {
            builder = builder.body(body.clone());
        }

        let response = builder.send().await?;
        if !response.status().is_redirection() {
            return Ok(response);
        }

        let Some(location) =
            response.headers().get(reqwest::header::LOCATION)
        else {
            // A redirection status with no `Location` is the final response.
            return Ok(response);
        };
        let location = location.to_str().map_err(|_| {
            crate::ErrorKind::InputError(
                "Redirect Location is not valid UTF-8".to_string(),
            )
        })?;
        url = url.join(location).map_err(|error| {
            crate::ErrorKind::InputError(format!(
                "Invalid redirect target: {error}"
            ))
        })?;

        let status = response.status().as_u16();
        if status == 303
            || ((status == 301 || status == 302)
                && method == reqwest::Method::POST)
        {
            method = reqwest::Method::GET;
            body = None;
        }
    }

    Err(crate::ErrorKind::InputError(format!(
        "Too many redirects (more than {MAX_REDIRECTS})"
    ))
    .into())
}

const FETCH_ATTEMPTS: usize = 2;

pub type FetchProgressFn<'a> = dyn FnMut(
        u64,
        u64,
    ) -> Pin<Box<dyn Future<Output = crate::Result<()>> + Send + 'a>>
    + Send
    + 'a;

#[derive(Clone, Debug)]
pub struct DownloadedFile {
    path: Arc<tempfile::TempPath>,
    pub size: u64,
    pub sha1: String,
}

impl DownloadedFile {
    pub fn path(&self) -> &Path {
        self.path.as_ref().as_ref()
    }

    pub async fn copy_to(
        &self,
        path: &Path,
        semaphore: &IoSemaphore,
    ) -> crate::Result<()> {
        let _permit = semaphore.0.acquire().await?;
        let parent = path.parent().ok_or_else(|| {
            ErrorKind::InputError(
                "Download destination has no parent directory".to_string(),
            )
        })?;
        io::create_dir_all(parent).await?;
        let parent = parent.to_path_buf();
        let temporary = tokio::task::spawn_blocking(move || {
            tempfile::NamedTempFile::new_in(parent)
                .map(|file| file.into_temp_path())
        })
        .await??;
        io::copy(self.path(), &temporary).await?;
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            temporary
                .persist(&path)
                .map_err(|error| IOError::with_path(error.error, &path))
        })
        .await??;
        Ok(())
    }
}

enum FetchBody {
    Memory(Bytes),
    File(DownloadedFile),
}

/// Downloads and verifies a temporary file, which is removed when its last owner is dropped.
pub async fn fetch_file(
    url: &str,
    sha1: Option<&str>,
    download_meta: Option<&DownloadMeta>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    progress: Option<&mut FetchProgressFn<'_>>,
) -> crate::Result<DownloadedFile> {
    let body = fetch_advanced_with_target(
        Method::GET,
        url,
        sha1,
        None,
        None,
        None,
        download_meta,
        None,
        uri_path,
        semaphore,
        exec,
        &INSECURE_REQWEST_CLIENT,
        progress,
        true,
    )
    .await?;
    match body {
        FetchBody::File(file) => Ok(file),
        FetchBody::Memory(_) => unreachable!("requested a file download"),
    }
}

pub async fn fetch_file_mirrors(
    mirrors: &[&str],
    sha1: Option<&str>,
    download_meta: Option<&DownloadMeta>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite> + Copy,
    mut progress: Option<&mut FetchProgressFn<'_>>,
) -> crate::Result<DownloadedFile> {
    if mirrors.is_empty() {
        return Err(
            ErrorKind::InputError("No mirrors provided!".to_string()).into()
        );
    }
    for (index, mirror) in mirrors.iter().enumerate() {
        let body = fetch_advanced_with_target(
            Method::GET,
            mirror,
            sha1,
            None,
            None,
            None,
            download_meta,
            None,
            uri_path,
            semaphore,
            exec,
            &REQWEST_CLIENT,
            progress.as_deref_mut(),
            true,
        )
        .await;
        match body {
            Ok(FetchBody::File(file)) => return Ok(file),
            Ok(FetchBody::Memory(_)) => {
                unreachable!("requested a file download")
            }
            Err(error) if index + 1 == mirrors.len() => return Err(error),
            Err(_) => {}
        }
    }
    unreachable!()
}

#[derive(Debug)]
pub(crate) enum CheckedFileFailure {
	Retryable {
		error: crate::Error,
		retry_after: Option<Duration>,
		stalled: bool,
	},
	Rejected(crate::Error),
	Fatal(crate::Error),
}

impl CheckedFileFailure {
	pub(crate) fn into_error(self) -> crate::Error {
		match self {
			Self::Retryable { error, .. } | Self::Rejected(error) | Self::Fatal(error) => error,
		}
	}

	fn network(error: reqwest::Error) -> Self {
		let stalled = error.is_timeout();
		if stalled || error.is_connect() || error.is_body() {
			Self::Retryable { error: error.into(), retry_after: None, stalled }
		} else {
			Self::Rejected(error.into())
		}
	}

	fn timeout() -> Self {
		Self::Retryable {
			error: ErrorKind::FSError("Minecraft download made no progress before its deadline".into()).into(),
			retry_after: None,
			stalled: true,
		}
	}
}

/// A single anonymous, host-checked transfer. Minecraft owns the retry budget.
pub(crate) async fn fetch_checked_file(
	url: reqwest::Url,
	sha1: &str,
	expected_size: u64,
	allowed_hosts: &[String],
	semaphore: &FetchSemaphore,
	progress: Option<&mut FetchProgressFn<'_>>,
	stall_timeout: Duration,
) -> Result<DownloadedFile, CheckedFileFailure> {
	let _permit = semaphore.0.acquire().await
		.map_err(|error| CheckedFileFailure::Fatal(error.into()))?;
	let deadline = tokio::time::Instant::now() + stall_timeout;
	let response = tokio::time::timeout_at(deadline, send_checked(
		&NO_REDIRECT_REQWEST_CLIENT,
		Method::GET,
		url,
		&HashMap::new(),
		None,
		|host| async move {
			if allowed_hosts.iter().any(|allowed| allowed == &host) {
				Ok(())
			} else {
				Err(ErrorKind::InputError(format!("Unapproved Minecraft download host: {host}")).into())
			}
		},
	)).await.map_err(|_| CheckedFileFailure::timeout())?
		.map_err(|error| {
			let (retryable, stalled) = match error.raw.as_ref() {
				ErrorKind::FetchError(inner) => (inner.is_timeout() || inner.is_connect() || inner.is_body(), inner.is_timeout()),
				_ => (false, false),
			};
			if retryable {
				CheckedFileFailure::Retryable { error, retry_after: None, stalled }
			} else {
				CheckedFileFailure::Rejected(error)
			}
		})?;
	let status = response.status();
	if !status.is_success() {
		let retry_after = response.headers().get(reqwest::header::RETRY_AFTER)
			.and_then(|value| value.to_str().ok())
			.and_then(|value| value.parse::<u64>().ok().or_else(|| {
				DateTime::parse_from_rfc2822(value).ok().map(|time| {
					(time.timestamp() - Utc::now().timestamp()).max(0) as u64
				})
			}))
			.map(|seconds| Duration::from_secs(seconds.min(5)))
			.or_else(|| (status.as_u16() == 429).then_some(Duration::from_secs(1)));
		let error = ErrorKind::FSError(format!("Minecraft download returned HTTP {status}")).into();
		return Err(if matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504) {
			CheckedFileFailure::Retryable { error, retry_after, stalled: false }
		} else {
			CheckedFileFailure::Rejected(error)
		});
	}
	let file = read_file_response_with_policy(
		response, progress, Some((deadline, stall_timeout)), Some(expected_size),
	).await?;
	validate_checked_file(file, sha1, expected_size)
}

fn validate_checked_file(file: DownloadedFile, sha1: &str, expected_size: u64) -> Result<DownloadedFile, CheckedFileFailure> {
	if !file.sha1.eq_ignore_ascii_case(sha1) {
		return Err(CheckedFileFailure::Rejected(ErrorKind::HashError(file.sha1.clone(), sha1.to_string()).into()));
	}
	if file.size != expected_size {
		return Err(CheckedFileFailure::Rejected(ErrorKind::FSError(format!(
			"Minecraft download has size {}, expected {expected_size}", file.size,
		)).into()));
	}
	Ok(file)
}

async fn read_file_response(
	response: reqwest::Response,
	progress: Option<&mut FetchProgressFn<'_>>,
) -> crate::Result<DownloadedFile> {
	read_file_response_with_policy(response, progress, None, None).await
		.map_err(CheckedFileFailure::into_error)
}

async fn read_file_response_with_policy(
	response: reqwest::Response,
	mut progress: Option<&mut FetchProgressFn<'_>>,
	mut timing: Option<(tokio::time::Instant, Duration)>,
	size_limit: Option<u64>,
) -> Result<DownloadedFile, CheckedFileFailure> {
	use futures::StreamExt;
	let path = tokio::task::spawn_blocking(|| {
		tempfile::NamedTempFile::new().map(|file| file.into_temp_path())
	}).await.map_err(|error| CheckedFileFailure::Fatal(error.into()))?
		.map_err(|error| CheckedFileFailure::Fatal(error.into()))?;
	let mut file = File::create(&path).await
		.map_err(|error| CheckedFileFailure::Fatal(error.into()))?;
	let total = response.content_length().unwrap_or(0);
	let mut stream = response.bytes_stream();
	let mut hasher = sha1_smol::Sha1::new();
	let mut size = 0_u64;
	loop {
		let next = if let Some((deadline, _)) = timing {
			tokio::time::timeout_at(deadline, stream.next()).await
				.map_err(|_| CheckedFileFailure::timeout())?
		} else {
			stream.next().await
		};
		let Some(chunk) = next else { break };
		let chunk = chunk.map_err(CheckedFileFailure::network)?;
		if chunk.is_empty() { continue; }
		size = size.checked_add(chunk.len() as u64).ok_or_else(|| {
			CheckedFileFailure::Rejected(ErrorKind::FSError("Minecraft download size overflow".into()).into())
		})?;
		if size_limit.is_some_and(|limit| size > limit) {
			return Err(CheckedFileFailure::Rejected(ErrorKind::FSError("Minecraft download exceeds its expected size".into()).into()));
		}
		if let Some((deadline, timeout)) = timing.as_mut() {
			*deadline = tokio::time::Instant::now() + *timeout;
		}
		file.write_all(&chunk).await
			.map_err(|error| CheckedFileFailure::Fatal(error.into()))?;
		hasher.update(&chunk);
		if let Some(progress) = progress.as_mut() {
			progress(size, total).await.map_err(CheckedFileFailure::Fatal)?;
		}
	}
	file.flush().await.map_err(|error| CheckedFileFailure::Fatal(error.into()))?;
	drop(file);
	Ok(DownloadedFile { path: Arc::new(path), size, sha1: hasher.hexdigest() })
}

#[cfg(test)]
mod checked_file_tests {
	use super::*;

	async fn response(chunks: Vec<(Duration, &'static [u8])>) -> reqwest::Response {
		let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
		let address = listener.local_addr().unwrap();
		std::thread::spawn(move || {
			use std::io::{Read, Write};
			let (mut stream, _) = listener.accept().unwrap();
			let mut request = [0; 2048];
			let _ = stream.read(&mut request);
			let size: usize = chunks.iter().map(|(_, bytes)| bytes.len()).sum();
			if write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n").is_err() { return; }
			for (delay, bytes) in chunks {
				std::thread::sleep(delay);
				if stream.write_all(bytes).is_err() { return; }
			}
		});
		reqwest::Client::builder().no_proxy().build().unwrap()
			.get(format!("http://{address}/file")).send().await.unwrap()
	}

	#[tokio::test]
	async fn first_byte_stall_is_retryable() {
		let response = response(vec![(Duration::from_millis(100), b"1234")]).await;
		let timeout = Duration::from_millis(20);
		let error = read_file_response_with_policy(response, None, Some((tokio::time::Instant::now() + timeout, timeout)), Some(4)).await.unwrap_err();
		assert!(matches!(error, CheckedFileFailure::Retryable { stalled: true, .. }));
	}

	#[tokio::test]
	async fn flowing_body_can_take_longer_than_the_stall_timeout() {
		let response = response(vec![
			(Duration::from_millis(40), b"1"),
			(Duration::from_millis(40), b"2"),
			(Duration::from_millis(40), b"3"),
			(Duration::from_millis(40), b"4"),
		]).await;
		let timeout = Duration::from_millis(100);
		let file = read_file_response_with_policy(response, None, Some((tokio::time::Instant::now() + timeout, timeout)), Some(4)).await.unwrap();
		assert_eq!(std::fs::read(file.path()).unwrap(), b"1234");
		let mut hasher = sha1_smol::Sha1::new();
		hasher.update(b"1234");
		assert_eq!(file.sha1, hasher.hexdigest());
	}

	#[tokio::test]
	async fn size_and_hash_rejection_never_publish_a_file() {
		let response = response(vec![(Duration::ZERO, b"1234")]).await;
		assert!(matches!(read_file_response_with_policy(response, None, None, Some(3)).await, Err(CheckedFileFailure::Rejected(_))));
		let response = self::response(vec![(Duration::ZERO, b"1234")]).await;
		let file = read_file_response_with_policy(response, None, None, Some(4)).await.unwrap();
		let hash = file.sha1.clone();
		assert!(matches!(validate_checked_file(file.clone(), &"0".repeat(40), 4), Err(CheckedFileFailure::Rejected(_))));
		assert!(matches!(validate_checked_file(file, &hash, 5), Err(CheckedFileFailure::Rejected(_))));
	}

	#[tokio::test]
	async fn progress_callback_termination_is_fatal() {
		let response = response(vec![(Duration::ZERO, b"1234")]).await;
		let mut stop = |_current: u64, _total: u64| -> Pin<Box<dyn Future<Output = crate::Result<()>> + Send>> {
			Box::pin(async { Err(ErrorKind::InputError("cancelled by progress reporter".into()).into()) })
		};
		assert!(matches!(read_file_response_with_policy(response, Some(&mut stop), None, Some(4)).await, Err(CheckedFileFailure::Fatal(_))));
	}
}

async fn read_memory_response(
    response: reqwest::Response,
    url: &str,
    loading_bar: Option<(&LoadingBarId, f64)>,
    mut progress: Option<&mut FetchProgressFn<'_>>,
) -> crate::Result<Bytes> {
    let total_size = response.content_length();
    if (loading_bar.is_none() && progress.is_none()) || total_size.is_none() {
        return response
            .bytes()
            .await
            .wrap_err_with(|| eyre!("failed to read response body from {url}"))
            .map_err(Into::into);
    }
    use futures::StreamExt;
    let total_size = total_size.unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.wrap_err_with(|| {
            eyre!("failed to read response body from {url}")
        })?;
        bytes.extend_from_slice(&chunk);
        if let Some((bar, total)) = loading_bar
            && total_size > 0
        {
            emit_loading(
                bar,
                chunk.len() as f64 / total_size as f64 * total,
                None,
            )?;
        }
        if let Some(progress) = progress.as_mut() {
            progress(bytes.len() as u64, total_size).await?;
        }
    }
    Ok(Bytes::from(bytes))
}

#[tracing::instrument(skip(semaphore))]
pub async fn fetch(
    url: &str,
    sha1: Option<&str>,
    download_meta: Option<&DownloadMeta>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<Bytes> {
    fetch_advanced(
        Method::GET,
        url,
        sha1,
        None,
        None,
        download_meta,
        None,
        uri_path,
        semaphore,
        exec,
    )
    .await
}

#[tracing::instrument(skip(semaphore))]
pub async fn fetch_with_client(
    url: &str,
    sha1: Option<&str>,
    download_meta: Option<&DownloadMeta>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    client: &reqwest::Client,
) -> crate::Result<Bytes> {
    fetch_advanced_with_client(
        Method::GET,
        url,
        sha1,
        None,
        None,
        download_meta,
        None,
        uri_path,
        semaphore,
        exec,
        client,
    )
    .await
}

#[tracing::instrument(skip(json_body, semaphore))]
pub async fn fetch_json<T>(
    method: Method,
    url: &str,
    sha1: Option<&str>,
    json_body: Option<serde_json::Value>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<T>
where
    T: DeserializeOwned,
{
    let result = fetch_advanced(
        method, url, sha1, json_body, None, None, None, uri_path, semaphore,
        exec,
    )
    .await?;
    let value = serde_json::from_slice(&result)?;
    Ok(value)
}

/// Downloads a file with retry and checksum functionality, and a specific
/// [`reqwest::Client`].
#[tracing::instrument(skip(json_body, semaphore))]
#[allow(clippy::too_many_arguments)]
pub async fn fetch_advanced(
    method: Method,
    url: &str,
    sha1: Option<&str>,
    json_body: Option<serde_json::Value>,
    header: Option<(&str, &str)>,
    download_meta: Option<&DownloadMeta>,
    loading_bar: Option<(&LoadingBarId, f64)>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<Bytes> {
    fetch_advanced_with_client(
        method,
        url,
        sha1,
        json_body,
        header,
        download_meta,
        loading_bar,
        uri_path,
        semaphore,
        exec,
        &INSECURE_REQWEST_CLIENT,
    )
    .await
}

#[tracing::instrument(skip(body, semaphore))]
#[allow(clippy::too_many_arguments)]
pub async fn fetch_advanced_bytes(
    method: Method,
    url: &str,
    body: Bytes,
    header: Option<(&str, &str)>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<Bytes> {
    fetch_advanced_with_client_and_progress(
        method,
        url,
        None,
        None,
        Some(body),
        header,
        None,
        None,
        uri_path,
        semaphore,
        exec,
        &INSECURE_REQWEST_CLIENT,
        None,
    )
    .await
}

#[tracing::instrument(skip(json_body, semaphore, progress))]
#[allow(clippy::too_many_arguments)]
pub async fn fetch_advanced_with_progress(
    method: Method,
    url: &str,
    sha1: Option<&str>,
    json_body: Option<serde_json::Value>,
    header: Option<(&str, &str)>,
    download_meta: Option<&DownloadMeta>,
    loading_bar: Option<(&LoadingBarId, f64)>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    progress: Option<&mut FetchProgressFn<'_>>,
) -> crate::Result<Bytes> {
    fetch_advanced_with_client_and_progress(
        method,
        url,
        sha1,
        json_body,
        None,
        header,
        download_meta,
        loading_bar,
        uri_path,
        semaphore,
        exec,
        &INSECURE_REQWEST_CLIENT,
        progress,
    )
    .await
}

/// Downloads a file with retry and checksum functionality
#[tracing::instrument(skip(json_body, semaphore))]
#[allow(clippy::too_many_arguments)]
pub async fn fetch_advanced_with_client(
    method: Method,
    url: &str,
    sha1: Option<&str>,
    json_body: Option<serde_json::Value>,
    header: Option<(&str, &str)>,
    download_meta: Option<&DownloadMeta>,
    loading_bar: Option<(&LoadingBarId, f64)>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    client: &reqwest::Client,
) -> crate::Result<Bytes> {
    fetch_advanced_with_client_and_progress(
        method,
        url,
        sha1,
        json_body,
        None,
        header,
        download_meta,
        loading_bar,
        uri_path,
        semaphore,
        exec,
        client,
        None,
    )
    .await
}

#[tracing::instrument(skip(
    json_body, bytes_body, semaphore, client, progress
))]
#[allow(clippy::too_many_arguments)]
async fn fetch_advanced_with_client_and_progress(
    method: Method,
    url: &str,
    sha1: Option<&str>,
    json_body: Option<serde_json::Value>,
    bytes_body: Option<Bytes>,
    header: Option<(&str, &str)>,
    download_meta: Option<&DownloadMeta>,
    loading_bar: Option<(&LoadingBarId, f64)>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    client: &reqwest::Client,
    progress: Option<&mut FetchProgressFn<'_>>,
) -> crate::Result<Bytes> {
    let body = fetch_advanced_with_target(
        method,
        url,
        sha1,
        json_body,
        bytes_body,
        header,
        download_meta,
        loading_bar,
        uri_path,
        semaphore,
        exec,
        client,
        progress,
        false,
    )
    .await?;
    match body {
        FetchBody::Memory(bytes) => Ok(bytes),
        FetchBody::File(_) => unreachable!("requested an in-memory response"),
    }
}

async fn fetch_advanced_with_target(
    method: Method,
    url: &str,
    sha1: Option<&str>,
    json_body: Option<serde_json::Value>,
    bytes_body: Option<Bytes>,
    header: Option<(&str, &str)>,
    download_meta: Option<&DownloadMeta>,
    loading_bar: Option<(&LoadingBarId, f64)>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    client: &reqwest::Client,
    mut progress: Option<&mut FetchProgressFn<'_>>,
    to_file: bool,
) -> crate::Result<FetchBody> {
    let _permit = semaphore.0.acquire().await?;

    let is_api_url = url.starts_with(env!("MODRINTH_API_URL"))
        || url.starts_with(env!("MODRINTH_API_URL_V3"));
    let fence_key = if is_api_url { uri_path } else { None };

    let creds = if header
        .as_ref()
        .is_none_or(|x| &*x.0.to_lowercase() != "authorization")
        && (url.starts_with("https://cdn.modrinth.com") || is_api_url)
    {
        crate::state::ModrinthCredentials::get_active(exec).await?
    } else {
        None
    };

    let download_meta_header = download_meta
        .map(|m| (DOWNLOAD_META_HEADER.to_string(), m.to_header_value()));

    for attempt in 1..=(FETCH_ATTEMPTS + 1) {
        if is_api_url {
            GLOBAL_API_RATE_LIMIT.check()?;
        }

        if let Some(fence_key) = fence_key
            && GLOBAL_FETCH_FENCE.is_blocked(fence_key)
        {
            return Err(ErrorKind::ApiIsDownError(
                GLOBAL_FETCH_FENCE.latest_block_minutes(),
            )
            .into());
        }

        let mut req = client.request(method.clone(), url);

        if let Some(body) = &json_body {
            req = req.json(body);
        } else if let Some(body) = bytes_body.clone() {
            req = req.body(body);
        }

        if let Some(header) = header {
            req = req.header(header.0, header.1);
        }

        if let Some(ref creds) = creds {
            req = req.header("Authorization", &creds.session);
        }

        if let Some((name, value)) = &download_meta_header {
            tracing::debug!("Sending download analytics: {value}");
            req = req.header(name.as_str(), value.as_str());
        }

        let result = req.send().await;
        match result {
            Ok(resp) => {
                if is_api_url
                    && let Some(error) =
                        GLOBAL_API_RATE_LIMIT.handle_response(&resp)
                {
                    return Err(error.into());
                }

                if resp.status().is_server_error() {
                    if let Some(fence_key) = fence_key {
                        GLOBAL_FETCH_FENCE.record_fail(fence_key);
                    }

                    if attempt <= FETCH_ATTEMPTS {
                        continue;
                    }
                }

                if resp.status().is_client_error()
                    || resp.status().is_server_error()
                {
                    let status = resp.status();
                    let backup_error = resp.error_for_status_ref().unwrap_err();
                    if let Ok(mut error) = resp.json::<LabrinthError>().await {
                        error.status = Some(status.as_u16());
                        error.method = Some(method.as_str().to_string());
                        error.url = Some(url.to_string());
                        error.route = uri_path.map(str::to_string);
                        return Err(ErrorKind::LabrinthError(error).into());
                    }
                    return Err(backup_error.into());
                }

                let bytes = if to_file {
                    read_file_response(resp, progress.as_deref_mut())
                        .await
                        .map(FetchBody::File)
                } else {
                    read_memory_response(
                        resp,
                        url,
                        loading_bar,
                        progress.as_deref_mut(),
                    )
                    .await
                    .map(FetchBody::Memory)
                };

                if let Ok(bytes) = bytes {
                    if let Some(sha1) = sha1 {
                        let hash = match &bytes {
                            FetchBody::Memory(bytes) => {
                                sha1_async(bytes.clone()).await?
                            }
                            FetchBody::File(file) => file.sha1.clone(),
                        };
                        if &*hash != sha1 {
                            if attempt <= FETCH_ATTEMPTS {
                                continue;
                            } else {
                                return Err(ErrorKind::HashError(
                                    sha1.to_string(),
                                    hash,
                                )
                                .into());
                            }
                        }
                    }

                    tracing::trace!("Done downloading URL {url}");

                    if let Some(fence_key) = fence_key {
                        GLOBAL_FETCH_FENCE.record_ok(fence_key);
                    }

                    return Ok(bytes);
                } else if attempt <= FETCH_ATTEMPTS {
                    continue;
                } else {
                    bytes?;
                }
            }
            Err(_) if attempt <= FETCH_ATTEMPTS => continue,
            Err(err) => {
                return Err(err.into());
            }
        }
    }

    unreachable!()
}

/// Downloads a file from specified mirrors
#[tracing::instrument(skip(semaphore))]
pub async fn fetch_mirrors(
    mirrors: &[&str],
    sha1: Option<&str>,
    download_meta: Option<&DownloadMeta>,
    uri_path: Option<&'static str>,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite> + Copy,
) -> crate::Result<Bytes> {
    if mirrors.is_empty() {
        return Err(
            ErrorKind::InputError("No mirrors provided!".to_string()).into()
        );
    }

    for (index, mirror) in mirrors.iter().enumerate() {
        let result = fetch_with_client(
            mirror,
            sha1,
            download_meta,
            uri_path,
            semaphore,
            exec,
            &REQWEST_CLIENT,
        )
        .await;

        if result.is_ok() || (result.is_err() && index == (mirrors.len() - 1)) {
            return result;
        }
    }

    unreachable!()
}

/// Posts a JSON to a URL
#[tracing::instrument(skip(json_body, semaphore))]
pub async fn post_json(
    url: &str,
    json_body: serde_json::Value,
    semaphore: &FetchSemaphore,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<()> {
    let _permit = semaphore.0.acquire().await?;

    let mut req = INSECURE_REQWEST_CLIENT.post(url).json(&json_body);

    if let Some(creds) =
        crate::state::ModrinthCredentials::get_active(exec).await?
    {
        req = req.header("Authorization", &creds.session);
    }

    req.send().await?.error_for_status()?;
    Ok(())
}

pub async fn read_json<T>(
    path: &Path,
    semaphore: &IoSemaphore,
) -> crate::Result<T>
where
    T: DeserializeOwned,
{
    let _permit = semaphore.0.acquire().await?;

    let json = io::read(path).await?;
    let json = serde_json::from_slice::<T>(&json)?;

    Ok(json)
}

#[tracing::instrument(skip(bytes, semaphore))]
pub async fn write(
    path: &Path,
    bytes: &[u8],
    semaphore: &IoSemaphore,
) -> crate::Result<()> {
    let _permit = semaphore.0.acquire().await?;

    if let Some(parent) = path.parent() {
        io::create_dir_all(parent).await?;
    }

    let mut file = File::create(path)
        .await
        .map_err(|e| IOError::with_path(e, path))?;
    file.write_all(bytes)
        .await
        .map_err(|e| IOError::with_path(e, path))?;
    tracing::trace!("Done writing file {}", path.display());
    Ok(())
}

pub async fn copy(
    src: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    semaphore: &IoSemaphore,
) -> crate::Result<()> {
    let src: &Path = src.as_ref();
    let dest = dest.as_ref();

    let _permit = semaphore.0.acquire().await?;

    if let Some(parent) = dest.parent() {
        io::create_dir_all(parent).await?;
    }

    io::copy(src, dest).await?;
    tracing::trace!(
        "Done copying file {} to {}",
        src.display(),
        dest.display()
    );
    Ok(())
}

pub async fn sha1_async(bytes: Bytes) -> crate::Result<String> {
    let hash = tokio::task::spawn_blocking(move || {
        sha1_smol::Sha1::from(bytes).hexdigest()
    })
    .await?;

    Ok(hash)
}

pub async fn sha1_file_async(
    path: impl AsRef<Path>,
) -> crate::Result<(u64, String)> {
    sha1_file_async_with_progress(path, |_, _| Ok(())).await
}

pub async fn sha1_file_async_with_progress(
    path: impl AsRef<Path>,
    mut progress: impl FnMut(u64, u64) -> crate::Result<()>,
) -> crate::Result<(u64, String)> {
    let path = path.as_ref();
    // Local files can be multi-gigabyte .mrpacks, so hash them without materializing bytes.
    let mut file = File::open(path)
        .await
        .map_err(|e| IOError::with_path(e, path))?;
    let total = file
        .metadata()
        .await
        .map_err(|e| IOError::with_path(e, path))?
        .len();
    let mut hasher = sha1_smol::Sha1::new();
    let mut size = 0;
    let mut buffer = vec![0; 262144];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| IOError::with_path(e, path))?;
        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
        size += bytes_read as u64;
        progress(size, total)?;
    }

    Ok((size, hasher.digest().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, Utc};

    #[test]
    fn test_fence_block_after_4_fails() {
        // Update tests if the FenceInner constants change

        let mut fence = FenceInner::new();

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(fence.is_blocked());
    }

    #[test]
    fn test_fetch_fence_keys_are_independent() {
        let fence = FetchFence {
            inner: Mutex::new(HashMap::new()),
        };

        for _ in 0..FenceInner::FAILURE_THRESHOLD {
            fence.record_fail("/v3/version_file/:sha1/update");
        }

        assert!(fence.is_blocked("/v3/version_file/:sha1/update"));
        assert!(!fence.is_blocked("/v3/project/:id"));
    }

    #[test]
    fn test_fetch_fence_latest_block_minutes() {
        let fence = FetchFence {
            inner: Mutex::new(HashMap::new()),
        };

        {
            let mut inner = fence.inner.lock();
            inner.insert("/expired", FenceInner::new());
            inner.get_mut("/expired").unwrap().block_until =
                Some(Utc::now() - TimeDelta::minutes(1));
            inner.insert("/short", FenceInner::new());
            inner.get_mut("/short").unwrap().block_until =
                Some(Utc::now() + TimeDelta::seconds(61));
            inner.insert("/long", FenceInner::new());
            inner.get_mut("/long").unwrap().block_until =
                Some(Utc::now() + TimeDelta::seconds(140));
        }

        assert_eq!(fence.latest_block_minutes(), 3);
    }

    #[test]
    fn test_fence_block_after_4_fails_with_oks() {
        // Update tests if the FenceInner constants change

        let mut fence = FenceInner::new();

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_ok();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(fence.is_blocked());
    }

    #[test]
    fn test_fence_not_blocked_after_fails_expire() {
        // Update tests if the FenceInner constants change

        let mut fence = FenceInner::new();

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.prune(Utc::now() + TimeDelta::seconds(60 * 3 + 55)); // Should prune all failures

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(!fence.is_blocked());

        fence.record_fail();
        assert!(fence.is_blocked());
    }

    #[test]
    fn test_fence_trigger_block_windows() {
        // brute force flukes
        for i in 0..128 {
            let mut fence = FenceInner::new();

            fence.trigger_block();
            assert!(fence.is_blocked(), "Should be blocked (attempt {i})");

            let block_until = fence.block_until.unwrap();
            assert!(
                block_until > Utc::now() + TimeDelta::seconds(60 + 55),
                "Should be more than 2 minutes (with some leeway) (attempt {i})"
            ); // more than 2 minutes (with some leeway)
            assert!(
                block_until < Utc::now() + TimeDelta::seconds(60 * 5),
                "Should be less than 5 minutes (attempt {i})"
            ); // less than 5 minutes

            fence.block_until = None;

            fence.trigger_block();
            let block_until = fence.block_until.unwrap();
            assert!(
                block_until > Utc::now() + TimeDelta::seconds(60 * 3 + 55),
                "Should be more than 4 minutes (with some leeway) (attempt {i})"
            ); // more than 4 minutes (with some leeway)
            assert!(
                block_until < Utc::now() + TimeDelta::seconds(60 * 10),
                "Should be less than 10 minutes (attempt {i})"
            ); // less than 10 minutes

            fence.block_until = None;

            fence.trigger_block();
            let block_until = fence.block_until.unwrap();
            assert!(
                block_until > Utc::now() + TimeDelta::seconds(60 * 5 + 55),
                "Should be more than 6 minutes (with some leeway) (attempt {i})"
            ); // more than 6 minutes (with some leeway)
            assert!(
                block_until < Utc::now() + TimeDelta::seconds(60 * 15),
                "Should be less than 15 minutes (attempt {i})"
            ); // less than 15 minutes
        }
    }
}

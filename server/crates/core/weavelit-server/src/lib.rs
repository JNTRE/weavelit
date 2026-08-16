#![forbid(unsafe_code)]

//! Restricted lifecycle startup composition for the Weavelit Server.

use std::{
    collections::{BTreeSet, HashMap},
    env, fmt, fs,
    future::Future,
    io::{ErrorKind, Read},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::Request,
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri,
        header::{ALLOW, CONTENT_LENGTH, CONTENT_TYPE, EXPECT, TRANSFER_ENCODING},
    },
    response::Response,
};
use rustix::fs::{self as rustix_fs, FileType, Mode, OFlags};
use rustls::{
    ServerConfig,
    crypto::aws_lc_rs,
    pki_types::{
        CertificateDer, PrivateKeyDer,
        pem::{PemObject, SectionKind},
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    task::{self, JoinSet},
    time::{Instant as Deadline, timeout, timeout_at},
};
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;
use weavelit_module_client::{
    CookieEffect, CookieLines, DatabaseSelectionRejection, ExpectedOrigin, ProjectionSource,
    SelectedBackend, SelectionCommit,
};
use weavelit_server_components::{AvailableComponents, LogSettingsFormat, MfaFactorFormat};
use weavelit_server_database_sqlite::{RetainedSqliteInspection, SqliteDatabase};
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendIdentifier,
    BackendRegistration, DatabaseError, DeploymentIdentifier, InitializedState,
    InterruptedLifecycleAction, LifecycleClassification, LifecycleError, LifecycleProjection,
    LifecycleStore, ProtectedValueAccess, RetainedDatabaseInspection, TrustedBackendContext,
    ValidatedConnectionSettings, WorkflowArbiter, WorkflowError, WorkflowKind,
};
use weavelit_server_log::LogModuleCatalog;
use weavelit_server_restore::{Name, TOTAL_REQUEST_DEADLINE};

pub mod authentication;
pub mod authorization;
pub mod init;
pub mod operational;
pub mod restore;
pub mod transport;

pub use weavelit_module_client::typed_json;

use operational::{
    ActiveDatabase, OperationalComposer, OperationalDatabase, OperationalMount, OperationalRuntime,
};
use transport::{HeadRead, MountedSurface, ProcessingDeadline, TransportCapability};
use typed_json::TypedJsonEnvelope;

const STATE_ROOT_ENV: &str = "WEAVELIT_STATE_ROOT";
const HTTPS_LISTENER_ADDRESS_ENV: &str = "WEAVELIT_HTTPS_LISTENER_ADDRESS";
const TLS_CERTIFICATE_PATH_ENV: &str = "WEAVELIT_TLS_CERTIFICATE_PATH";
const TLS_PRIVATE_KEY_PATH_ENV: &str = "WEAVELIT_TLS_PRIVATE_KEY_PATH";
const APPLICATION_DATABASE_FILE: &str = "application.sqlite3";
const MAX_TLS_MATERIAL_BYTES: u64 = 1024 * 1024;
const MAX_NORMAL_CONNECTIONS: usize = 15;
const MAX_REJECTION_CONNECTIONS: usize = 1;
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_PROCESSING_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a signalled shutdown may spend finishing accepted requests.
///
/// It covers the ordinary transport stages only: the handshake, read, and
/// processing budgets in sequence, so a request admitted just before the signal
/// can still finish inside it. It says nothing about a lifecycle transition,
/// whose irreversible region runs outside any connection future and is bounded
/// separately by [`SHUTDOWN_LIFECYCLE_TRANSITION_THRESHOLD`].
const SHUTDOWN_DRAIN_BUDGET: Duration = Duration::from_secs(25);
/// How long a signalled shutdown observes an irreversible lifecycle transition
/// before reporting an overrun.
///
/// A Restore's replacement chain and an Init's commit chain run as blocking
/// work that no connection future owns and that nothing may abort, so a stop
/// arriving inside one must wait for it rather than drain around it. The
/// threshold is the approved total request deadline, which is the longest such
/// a transition may legitimately still be running for before shutdown reports
/// that it overran.
const SHUTDOWN_LIFECYCLE_TRANSITION_THRESHOLD: Duration = Duration::from_secs(300);
/// How long a signalled shutdown may spend closing the Application Database.
const SHUTDOWN_DATABASE_BUDGET: Duration = Duration::from_secs(5);

const _: () = assert!(
    SHUTDOWN_DRAIN_BUDGET.as_secs()
        > TLS_HANDSHAKE_TIMEOUT.as_secs()
            + REQUEST_READ_TIMEOUT.as_secs()
            + REQUEST_PROCESSING_TIMEOUT.as_secs(),
    "draining must outlast the longest ordinary connection the listener admits"
);
const _: () = assert!(
    SHUTDOWN_LIFECYCLE_TRANSITION_THRESHOLD.as_secs() >= TOTAL_REQUEST_DEADLINE.as_secs(),
    "the transition overrun threshold must cover the longest lifecycle request deadline"
);
const MAX_REQUEST_TARGET_BYTES: usize = 2 * 1024;
const MAX_REQUEST_HEADER_BYTES: usize = 8 * 1024;
const MAX_REQUEST_HEAD_BYTES: usize = MAX_REQUEST_TARGET_BYTES + MAX_REQUEST_HEADER_BYTES + 128;
/// Separate allowance for the one method that may carry a body; it never
/// relaxes the target, header, or aggregate head bounds above.
const MAX_REQUEST_BODY_BYTES: usize = 1024;
const RATE_LIMIT_REQUESTS_PER_MINUTE: u32 = 20;
/// Sized so one source can serve a full Web UI page load, which costs one
/// document, two asset, and one status request, plus two immediate reloads.
const RATE_LIMIT_BURST: u32 = 12;
const MAX_JSON_BODY_BYTES: usize = 128;
/// Derived from the typed envelope's own maxima rather than from the fixed
/// profile's bound: a 48-byte stable code, a correlation identifier at the
/// canonical 64-byte bound, and at most four result fields, each a 48-byte
/// name and a 48-byte code value. The largest error envelope is 144 bytes and
/// the largest result envelope is 504 bytes, so 512 bounds both. The listener
/// re-checks a serialized envelope against this bound and redacts rather than
/// truncating, so the derivation is enforced and not merely documented.
const MAX_TYPED_JSON_BODY_BYTES: usize = 512;
const MAX_HTML_BODY_BYTES: usize = 16 * 1024;
const MAX_JAVASCRIPT_BODY_BYTES: usize = 256 * 1024;
const MAX_CSS_BODY_BYTES: usize = 64 * 1024;
const ASSET_SECURITY_HEADERS: &str = concat!(
    "Content-Security-Policy: default-src 'none'; base-uri 'none'; object-src 'none'; ",
    "frame-ancestors 'none'; form-action 'none'; script-src 'self'; style-src 'self'; ",
    "connect-src 'self'\r\n",
    "X-Content-Type-Options: nosniff\r\n",
    "Cache-Control: no-store\r\n",
);

#[derive(Clone, Copy)]
struct ConnectionTimeouts {
    handshake: Duration,
    request_read: Duration,
    processing: Duration,
}

struct ConnectionSlots {
    normal: Arc<Semaphore>,
    rejection: Arc<Semaphore>,
}

impl ConnectionSlots {
    fn new() -> Self {
        Self {
            normal: Arc::new(Semaphore::new(MAX_NORMAL_CONNECTIONS)),
            rejection: Arc::new(Semaphore::new(MAX_REJECTION_CONNECTIONS)),
        }
    }
}

/// The future whose completion begins an orderly shutdown.
///
/// The listener is handed the trigger rather than installing one, so process
/// signal policy stays at the process boundary and a test drives the same
/// shutdown path the signal drives without raising a real signal. It is a
/// concrete type rather than a parameter so the composed listener future stays
/// free of caller-supplied lifetimes.
pub struct ShutdownSignal(Pin<Box<dyn Future<Output = ()> + Send>>);

impl ShutdownSignal {
    /// Wraps the future whose completion asks the listener to stop.
    pub fn new(signalled: impl Future<Output = ()> + Send + 'static) -> Self {
        Self(Box::pin(signalled))
    }
}

impl fmt::Debug for ShutdownSignal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ShutdownSignal")
    }
}

/// How long each stage of a signalled shutdown may take.
///
/// The stages are bounded separately because they fail differently: a request
/// that will not finish must not consume the allowance the database close needs
/// to leave no work for recovery, and a lifecycle transition that must not be
/// interrupted must not be cut short by whatever a client is doing to a
/// connection. Draining and transition observation run concurrently; the
/// database close follows both once the transition gate is held.
#[derive(Clone, Copy, Debug)]
struct ShutdownBudget {
    /// Time allowed for accepted requests to finish, response write included.
    drain: Duration,
    /// Threshold after which a transition that still holds its gate reports an
    /// overrun while shutdown continues waiting for it.
    transition_threshold: Duration,
    /// Time allowed, after both, to close the Application Database.
    database: Duration,
}

impl ShutdownBudget {
    const DEFAULT: Self = Self {
        drain: SHUTDOWN_DRAIN_BUDGET,
        transition_threshold: SHUTDOWN_LIFECYCLE_TRANSITION_THRESHOLD,
        database: SHUTDOWN_DATABASE_BUDGET,
    };
}

struct RateLimiter {
    sources: Mutex<HashMap<IpAddr, RateLimit>>,
}

struct RateLimit {
    available: f64,
    last_updated: Instant,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            sources: Mutex::new(HashMap::new()),
        }
    }

    fn allows(&self, source: IpAddr, now: Instant) -> bool {
        let mut sources = self
            .sources
            .lock()
            .expect("rate-limit lock must not poison");
        let limit = sources.entry(source).or_insert(RateLimit {
            available: f64::from(RATE_LIMIT_BURST),
            last_updated: now,
        });
        let elapsed = now.saturating_duration_since(limit.last_updated);
        let replenished = elapsed.as_secs_f64() * f64::from(RATE_LIMIT_REQUESTS_PER_MINUTE) / 60.0;
        limit.available = (limit.available + replenished).min(f64::from(RATE_LIMIT_BURST));
        limit.last_updated = now;
        if limit.available < 1.0 {
            return false;
        }
        limit.available -= 1.0;
        true
    }
}

/// Closed set of response shapes the restricted listener may emit.
///
/// The profile alone selects the media type, the security header block, and the
/// body bound. Nothing is taken from the request, a file extension, or the body
/// contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseProfile {
    /// Compile-time bodies drawn from the fixed allowlist, used by the frozen
    /// pre-operational lifecycle routes.
    Json,
    /// Envelopes the listener serializes itself, used by every other route.
    TypedJson,
    Html,
    JavaScript,
    Css,
}

impl ResponseProfile {
    const fn media_type(self) -> &'static str {
        match self {
            Self::Json | Self::TypedJson => "application/json; charset=utf-8",
            Self::Html => "text/html; charset=utf-8",
            Self::JavaScript => "text/javascript; charset=utf-8",
            Self::Css => "text/css; charset=utf-8",
        }
    }

    const fn max_body_bytes(self) -> usize {
        match self {
            Self::Json => MAX_JSON_BODY_BYTES,
            Self::TypedJson => MAX_TYPED_JSON_BODY_BYTES,
            Self::Html => MAX_HTML_BODY_BYTES,
            Self::JavaScript => MAX_JAVASCRIPT_BODY_BYTES,
            Self::Css => MAX_CSS_BODY_BYTES,
        }
    }

    const fn security_headers(self) -> &'static str {
        match self {
            Self::Json | Self::TypedJson => "",
            Self::Html | Self::JavaScript | Self::Css => ASSET_SECURITY_HEADERS,
        }
    }

    /// Maps an observed module media type to a profile.
    ///
    /// The typed profile is never selected here: a route reaches it only by
    /// returning a typed envelope the listener serializes itself.
    fn from_media_type(value: &HeaderValue) -> Option<Self> {
        match value.as_bytes() {
            b"application/json; charset=utf-8" => Some(Self::Json),
            b"text/html; charset=utf-8" => Some(Self::Html),
            b"text/javascript; charset=utf-8" => Some(Self::JavaScript),
            b"text/css; charset=utf-8" => Some(Self::Css),
            _ => None,
        }
    }
}

/// Closed set of methods the listener may advertise in an `Allow` header.
///
/// A module can only cause one of these fixed header lines to be emitted; it
/// can never supply header text of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AllowedMethod {
    Get,
    Put,
}

impl AllowedMethod {
    const fn allow_header(self) -> &'static str {
        match self {
            Self::Get => "Allow: GET\r\n",
            Self::Put => "Allow: PUT\r\n",
        }
    }

    fn from_header_value(value: &HeaderValue) -> Option<Self> {
        match value.as_bytes() {
            b"GET" => Some(Self::Get),
            b"PUT" => Some(Self::Put),
            _ => None,
        }
    }
}

/// An action the listener runs after it has written a response successfully.
///
/// A workflow attaches one to make a later capability reachable only once the
/// response that carries its one-time output has actually left the Server. The
/// listener owns when it runs: it is invoked after the response has been
/// written and never by the route that produced the response.
///
/// # What a successful write means
///
/// It means exactly this: every byte of the response was accepted by the TLS
/// transport and the connection was shut down cleanly, all inside the
/// connection's response budget. It does **not** mean that the peer received,
/// decrypted, stored, rendered, or read those bytes. No event observable at
/// this boundary can establish any of that, so nothing here should be read as
/// evidence that a person holds what the response contained.
///
/// The guarantee is therefore one-directional and is useful only in that
/// direction. A write failure, a peer that disappeared before the connection
/// was shut down, and a response budget that expired are indistinguishable
/// here, and none of them runs the action. A caller that publishes capability
/// from this action stays fail closed whenever delivery is merely uncertain,
/// and it must not treat the action running as proof of receipt.
#[derive(Clone)]
pub struct ResponseWriteAcknowledgement {
    /// Taken on the first run, so the action runs at most once however many
    /// responses or clones carry this value.
    action: Arc<Mutex<Option<AcknowledgementAction>>>,
}

/// The one-shot action a [`ResponseWriteAcknowledgement`] runs.
///
/// It consumes itself, so the type alone records that it cannot run twice.
type AcknowledgementAction = Box<dyn FnOnce() + Send>;

impl ResponseWriteAcknowledgement {
    /// Creates the acknowledgement a response carries to the listener.
    #[must_use]
    pub fn new<A>(action: A) -> Self
    where
        A: FnOnce() + Send + 'static,
    {
        Self {
            action: Arc::new(Mutex::new(Some(Box::new(action)))),
        }
    }

    /// Runs the action at most once.
    ///
    /// A lane left poisoned by a panicking action is recovered rather than
    /// re-entered: the action has already been taken, so a later call finds
    /// nothing to run.
    fn run(&self) {
        let action = self
            .action
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(action) = action {
            action();
        }
    }
}

impl fmt::Debug for ResponseWriteAcknowledgement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResponseWriteAcknowledgement(REDACTED)")
    }
}

#[derive(Clone)]
struct BoundedResponse {
    status: StatusCode,
    profile: ResponseProfile,
    body: Bytes,
    allow: Option<AllowedMethod>,
    /// Rendered `Set-Cookie` lines produced from a closed cookie effect.
    ///
    /// The listener renders these itself from a route's effect value; a route
    /// never supplies header text. Only the typed profile can carry one, and
    /// [`redacted_response`] leaves it absent, so a response that redacts
    /// emits no cookie at all.
    cookies: Option<CookieLines>,
    /// Action the listener runs only after this response is written.
    ///
    /// Only the typed profile can carry one, and [`redacted_response`] leaves
    /// it absent, so a response that redacts acknowledges nothing.
    acknowledgement: Option<ResponseWriteAcknowledgement>,
}

impl BoundedResponse {
    fn json(status: StatusCode, body: &'static str) -> Self {
        Self {
            status,
            profile: ResponseProfile::Json,
            body: Bytes::from_static(body.as_bytes()),
            allow: None,
            cookies: None,
            acknowledgement: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Trusted HTTPS listener configuration
// ---------------------------------------------------------------------------

/// Validated, host-supplied material for the Server's sole HTTPS listener.
pub struct TrustedHttpsListener {
    address: SocketAddr,
    tls_config: Arc<ServerConfig>,
}

impl TrustedHttpsListener {
    /// Returns the sole configured TLS listener address.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Returns the TLS configuration validated from trusted host material.
    pub fn tls_config(&self) -> Arc<ServerConfig> {
        Arc::clone(&self.tls_config)
    }
}

/// Reads and validates the trusted HTTPS listener configuration from the host.
pub fn read_trusted_https_listener() -> Result<TrustedHttpsListener, StartupError> {
    let address = read_required_environment(
        HTTPS_LISTENER_ADDRESS_ENV,
        StartupError::ListenerNotConfigured,
    )?;
    let certificate_path = read_required_path(
        TLS_CERTIFICATE_PATH_ENV,
        StartupError::TlsCertificateNotConfigured,
    )?;
    let private_key_path = read_required_path(
        TLS_PRIVATE_KEY_PATH_ENV,
        StartupError::TlsPrivateKeyNotConfigured,
    )?;

    validate_trusted_https_listener(&address, &certificate_path, &private_key_path)
}

/// Validates trusted host listener settings before startup can classify lifecycle state.
pub fn validate_trusted_https_listener(
    address: &str,
    certificate_path: &Path,
    private_key_path: &Path,
) -> Result<TrustedHttpsListener, StartupError> {
    let address = address
        .parse::<SocketAddr>()
        .map_err(|_| StartupError::ListenerAddressInvalid)?;
    if address.port() == 0 || !address.ip().is_loopback() {
        return Err(StartupError::ListenerAddressInvalid);
    }

    let certificate_bytes = read_tls_material(certificate_path, false)?;
    let private_key_bytes = read_tls_material(private_key_path, true)?;
    let certificates = parse_certificates(&certificate_bytes)?;
    let private_key = parse_private_key(&private_key_bytes)?;

    let tls_config = ServerConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| StartupError::TlsMaterialInvalid)?
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(|_| StartupError::TlsMaterialInvalid)?;

    Ok(TrustedHttpsListener {
        address,
        tls_config: Arc::new(tls_config),
    })
}

fn read_required_environment(
    variable: &str,
    missing_error: StartupError,
) -> Result<String, StartupError> {
    let value = env::var(variable).map_err(|_| missing_error)?;
    if value.is_empty() {
        return Err(missing_error);
    }
    Ok(value)
}

fn read_required_path(
    variable: &str,
    missing_error: StartupError,
) -> Result<PathBuf, StartupError> {
    Ok(PathBuf::from(read_required_environment(
        variable,
        missing_error,
    )?))
}

fn read_tls_material(path: &Path, private_key: bool) -> Result<Vec<u8>, StartupError> {
    let (file, length) = open_tls_material(path, private_key)?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_TLS_MATERIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StartupError::TlsMaterialInvalid)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_TLS_MATERIAL_BYTES {
        return Err(StartupError::TlsMaterialInvalid);
    }
    Ok(bytes)
}

fn open_tls_material(path: &Path, private_key: bool) -> Result<(fs::File, u64), StartupError> {
    if path
        .as_os_str()
        .as_encoded_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| matches!(component, b"." | b".."))
    {
        return Err(StartupError::TlsMaterialInvalid);
    }

    let mut names = Vec::new();
    let mut saw_root = false;
    for component in path.components() {
        match component {
            std::path::Component::RootDir if !saw_root => saw_root = true,
            std::path::Component::Normal(component) if saw_root => names.push(component),
            _ => return Err(StartupError::TlsMaterialInvalid),
        }
    }
    let name = names.pop().ok_or(StartupError::TlsMaterialInvalid)?;
    let mut directory = rustix_fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| StartupError::TlsMaterialInvalid)?;
    for component in names {
        directory = rustix_fs::openat(
            &directory,
            component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| StartupError::TlsMaterialInvalid)?;
    }
    let descriptor = rustix_fs::openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| StartupError::TlsMaterialInvalid)?;
    let metadata = rustix_fs::fstat(&descriptor).map_err(|_| StartupError::TlsMaterialInvalid)?;
    let mode = metadata.st_mode;
    let length = u64::try_from(metadata.st_size).map_err(|_| StartupError::TlsMaterialInvalid)?;
    if !FileType::from_raw_mode(mode).is_file()
        || metadata.st_nlink != 1
        || mode & 0o022 != 0
        || (private_key && mode & 0o007 != 0)
        || length == 0
        || length > MAX_TLS_MATERIAL_BYTES
    {
        return Err(StartupError::TlsMaterialInvalid);
    }
    Ok((fs::File::from(descriptor), length))
}

fn parse_certificates(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, StartupError> {
    validate_pem_envelope(bytes, &[b"CERTIFICATE"])?;
    let mut certificates = Vec::new();
    for section in <(SectionKind, Vec<u8>)>::pem_slice_iter(bytes) {
        let (kind, der) = section.map_err(|_| StartupError::TlsMaterialInvalid)?;
        match kind {
            SectionKind::Certificate => certificates.push(CertificateDer::from(der)),
            _ => return Err(StartupError::TlsMaterialInvalid),
        }
    }
    if certificates.is_empty() {
        return Err(StartupError::TlsMaterialInvalid);
    }
    Ok(certificates)
}

fn parse_private_key(bytes: &[u8]) -> Result<PrivateKeyDer<'static>, StartupError> {
    validate_pem_envelope(
        bytes,
        &[b"RSA PRIVATE KEY", b"PRIVATE KEY", b"EC PRIVATE KEY"],
    )?;
    let mut private_key = None;
    for section in <(SectionKind, Vec<u8>)>::pem_slice_iter(bytes) {
        let (kind, der) = section.map_err(|_| StartupError::TlsMaterialInvalid)?;
        let candidate = match kind {
            SectionKind::RsaPrivateKey => PrivateKeyDer::Pkcs1(der.into()),
            SectionKind::PrivateKey => PrivateKeyDer::Pkcs8(der.into()),
            SectionKind::EcPrivateKey => PrivateKeyDer::Sec1(der.into()),
            _ => return Err(StartupError::TlsMaterialInvalid),
        };
        if private_key.replace(candidate).is_some() {
            return Err(StartupError::TlsMaterialInvalid);
        }
    }
    private_key.ok_or(StartupError::TlsMaterialInvalid)
}

fn validate_pem_envelope(bytes: &[u8], allowed_labels: &[&[u8]]) -> Result<(), StartupError> {
    let mut expected_end_label = None;
    for line in bytes.split(|byte| matches!(*byte, b'\r' | b'\n')) {
        if let Some(label) = expected_end_label {
            if pem_end_label(line) == Some(label) {
                expected_end_label = None;
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let label = pem_begin_label(line).ok_or(StartupError::TlsMaterialInvalid)?;
        expected_end_label = allowed_labels
            .iter()
            .copied()
            .find(|allowed_label| *allowed_label == label)
            .ok_or(StartupError::TlsMaterialInvalid)?
            .into();
    }
    if expected_end_label.is_some() {
        return Err(StartupError::TlsMaterialInvalid);
    }
    Ok(())
}

fn pem_begin_label(line: &[u8]) -> Option<&[u8]> {
    line.strip_prefix(b"-----BEGIN ")?
        .strip_suffix(b"-----")
        .filter(|label| !label.is_empty())
}

fn pem_end_label(line: &[u8]) -> Option<&[u8]> {
    line.strip_prefix(b"-----END ")?.strip_suffix(b"-----")
}

// ---------------------------------------------------------------------------
// Lifecycle adapter
// ---------------------------------------------------------------------------

/// The gate a lifecycle transition must be inside to strand durable state.
///
/// Init and Restore each contain one region that must not be interrupted: from
/// the moment the fail-closed surface is published to the moment the deployment
/// record has been advanced or sealed. That region runs as blocking work no
/// connection future owns and nothing may abort, so a signalled shutdown can
/// neither drain around it nor cut it short. This gate is the only thing that
/// lets a shutdown wait for exactly that region, and refuse the workflows that
/// have not entered it yet.
///
/// It is deliberately not the mutation lane. A Restore holds that lane across
/// artifact upload, decryption, and validation, so waiting on it would expose
/// how long a stop takes to attacker-supplied work.
pub(crate) struct LifecycleTransitionGate {
    /// Set once, synchronously, when a stop is first observed.
    stopping: AtomicBool,
    /// Single permit, so at most one transition is ever inside the region and a
    /// shutdown that holds it knows the region is empty.
    permit: Arc<Semaphore>,
}

/// Proof that its holder is inside the irreversible transition region.
///
/// Dropping it is what tells a waiting shutdown the region is empty again, so
/// it is retained across the whole region rather than checked at its start.
pub(crate) struct LifecycleTransitionGuard {
    _permit: OwnedSemaphorePermit,
}

/// A shutdown-held transition permit and whether its reporting threshold elapsed.
///
/// The permit remains held until shutdown has completed its database close, so
/// no transition can enter after an overrun is observed.
struct LifecycleTransitionQuiescence {
    _guard: LifecycleTransitionGuard,
    overrun: bool,
}

impl LifecycleTransitionGate {
    fn new() -> Self {
        Self {
            stopping: AtomicBool::new(false),
            permit: Arc::new(Semaphore::new(1)),
        }
    }

    /// Enters the region, or refuses without waiting.
    ///
    /// Refusal is not a new outcome: every caller maps it onto the failure its
    /// surrounding chain already produces for an operation abandoned before its
    /// point of no return, so nothing an operator can observe changes.
    ///
    /// The flag is read on both sides of the acquisition, which is what closes
    /// the race between a stop being signalled and a transition entering. An
    /// entrant whose acquisition completes after the flag is set releases the
    /// permit and refuses, so the only entrants a shutdown can ever have to
    /// wait for are those that observed an open gate after taking the permit,
    /// and for those the shutdown's own acquisition necessarily blocks until
    /// the guard is dropped.
    ///
    /// It never waits for the permit. A second transition cannot exist, because
    /// the mutation lane already admits one workflow at a time, so a permit
    /// that is unavailable means a shutdown is holding it.
    pub(crate) fn try_enter(&self) -> Option<LifecycleTransitionGuard> {
        if self.stopping.load(AtomicOrdering::SeqCst) {
            return None;
        }
        let permit = Arc::clone(&self.permit).try_acquire_owned().ok()?;
        if self.stopping.load(AtomicOrdering::SeqCst) {
            drop(permit);
            return None;
        }
        Some(LifecycleTransitionGuard { _permit: permit })
    }

    /// Closes the gate to every transition that has not already entered it.
    ///
    /// Called synchronously on the stop, before anything is awaited, so no
    /// transition can enter during the shutdown's own scheduling.
    fn begin_stopping(&self) {
        self.stopping.store(true, AtomicOrdering::SeqCst);
    }

    /// Reports whether a transition is inside the region right now.
    ///
    /// It observes the permit without taking it, so a test can order itself
    /// against a real workflow's region without stealing the permit that
    /// workflow is about to acquire.
    ///
    /// It answers whether the permit is held, which becomes true when
    /// [`Self::try_enter`] takes it and before that entry is finalized against
    /// the stop flag. It is therefore an assertion a test makes about a
    /// workflow already known to be inside the region, never the signal by
    /// which a test learns that a workflow got in.
    #[cfg(test)]
    pub(crate) fn is_occupied(&self) -> bool {
        self.permit.available_permits() == 0
    }

    /// Waits for the region to empty, then holds it empty.
    ///
    /// The returned guard is retained by the shutdown for the rest of its own
    /// run, so nothing can enter the region behind the wait. When `threshold`
    /// expires first, shutdown records the overrun but continues awaiting this
    /// same acquisition; it never closes a database while a transition holds
    /// the gate.
    async fn quiesce(&self, threshold: Duration) -> LifecycleTransitionQuiescence {
        let acquire = Arc::clone(&self.permit).acquire_owned();
        tokio::pin!(acquire);
        if threshold.is_zero() {
            let permit = acquire
                .await
                .expect("the lifecycle transition gate semaphore is never closed");
            return LifecycleTransitionQuiescence {
                _guard: LifecycleTransitionGuard { _permit: permit },
                overrun: true,
            };
        }
        let threshold_elapsed = tokio::time::sleep(threshold);
        tokio::pin!(threshold_elapsed);

        let (overrun, acquired) = tokio::select! {
            biased;
            _ = &mut threshold_elapsed => (true, acquire.await),
            acquired = &mut acquire => (false, acquired),
        };
        let permit = acquired.expect("the lifecycle transition gate semaphore is never closed");

        LifecycleTransitionQuiescence {
            _guard: LifecycleTransitionGuard { _permit: permit },
            overrun,
        }
    }
}

/// Runtime-owned single-lane async adapter that moves all blocking arbiter
/// and SQLite access off the Tokio event-loop thread.
///
/// Status projection reads use `spawn_blocking` independently of the mutation
/// lane so they are never serialized behind an in-progress selection.
/// Selection mutations acquire a single-permit semaphore before entering
/// `spawn_blocking`: a mutation that has not yet started when its enclosing
/// `timeout` fires is cleanly cancelled at the semaphore `await` and never
/// reaches the blocking pool.  Once a mutation is inside `spawn_blocking` it
/// runs to completion regardless of the caller's timeout, preventing partial
/// durable state.
struct LifecycleAdapter {
    arbiter: Arc<WorkflowArbiter>,
    /// Single-permit gate that serializes selection mutations.
    mutation_lane: Arc<Semaphore>,
    /// Shared with Init, Restore, and the listener, so the one shutdown that
    /// closes the gate is the one both workflows are refused by.
    transition_gate: Arc<LifecycleTransitionGate>,
}

impl LifecycleAdapter {
    fn new(arbiter: Arc<WorkflowArbiter>) -> Self {
        Self {
            arbiter,
            mutation_lane: Arc::new(Semaphore::new(1)),
            transition_gate: Arc::new(LifecycleTransitionGate::new()),
        }
    }

    /// Reads the live lifecycle projection on a blocking thread.
    ///
    /// Does not compete with the mutation lane, so a concurrent selection does
    /// not delay the projection read on the event-loop thread.
    async fn project(&self) -> Option<LifecycleProjection> {
        let arbiter = Arc::clone(&self.arbiter);
        task::spawn_blocking(move || arbiter.projection().ok())
            .await
            .ok()
            .flatten()
    }

    /// Runs the database selection in the single mutation lane.
    ///
    /// Awaits the lane semaphore before entering `spawn_blocking`.  If the
    /// caller's enclosing `timeout` fires while waiting for the semaphore, the
    /// future is cancelled here and no blocking work starts.  Once the
    /// semaphore is acquired and `spawn_blocking` begins, the selection runs to
    /// completion regardless of any external cancellation.
    async fn select(
        &self,
        backend: SelectedBackend,
        catalog: Arc<BackendCatalog>,
        context: Arc<TrustedBackendContext>,
    ) -> Result<LifecycleProjection, DatabaseSelectionRejection> {
        let permit = Arc::clone(&self.mutation_lane)
            .acquire_owned()
            .await
            .map_err(|_| DatabaseSelectionRejection::ServiceUnavailable)?;

        let arbiter = Arc::clone(&self.arbiter);
        task::spawn_blocking(move || {
            let _permit = permit;
            let identifier = BackendIdentifier::new(backend.identifier())
                .map_err(|_| DatabaseSelectionRejection::BadRequest)?;
            arbiter
                .select_database(&catalog, &context, &identifier, Vec::new())
                .map(|(_database, projection)| projection)
                .map_err(|error| DatabaseSelectionRejection::from_selection_failure(error.kind()))
        })
        .await
        .map_err(|_| DatabaseSelectionRejection::ServiceUnavailable)?
    }
}

// ---------------------------------------------------------------------------
// SQLite backend factory
// ---------------------------------------------------------------------------

struct SqliteFactory;

impl ApplicationDatabaseFactory for SqliteFactory {
    fn open(
        &self,
        context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError> {
        SqliteDatabase::open(context.application_database_path())
            .map(|db| Box::new(db) as Box<dyn ApplicationDatabase>)
            .map_err(|_| LifecycleError::DependencyUnavailable)
    }

    fn inspect_retained(
        &self,
        context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<RetainedDatabaseInspection, LifecycleError> {
        SqliteDatabase::inspect_retained(
            context.application_database_path(),
            expected_deployment_identifier,
        )
        .map(|inspection| match inspection {
            RetainedSqliteInspection::Inspected(inspection) => {
                RetainedDatabaseInspection::Inspected(inspection)
            }
            RetainedSqliteInspection::WalPresent => RetainedDatabaseInspection::RedeployRequired,
        })
        .map_err(map_database_error)
    }
}

fn map_database_error(error: DatabaseError) -> LifecycleError {
    match error {
        DatabaseError::DeploymentMismatch => LifecycleError::DeploymentMismatch,
        DatabaseError::Unavailable => LifecycleError::DependencyUnavailable,
        DatabaseError::IntegrityFailure => LifecycleError::IntegrityFailure,
        DatabaseError::ConfigurationInvalid => LifecycleError::ConfigurationInvalid,
        DatabaseError::InvalidState
        | DatabaseError::AlreadyInitialized
        | DatabaseError::NotInitialized => LifecycleError::InvalidState,
        _ => LifecycleError::InvalidState,
    }
}

/// Builds the compiled-in SQLite backend catalog.
pub fn sqlite_catalog() -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "sqlite",
        vec![],
        Box::new(SqliteFactory),
    )])
    .expect("compiled-in SQLite catalog must be valid")
}

/// Builds the compiled-in SQLite Log Module catalog.
pub fn sqlite_log_catalog() -> LogModuleCatalog {
    LogModuleCatalog::new(vec![weavelit_module_log_sqlite::registration()])
        .expect("compiled-in SQLite Log Module catalog must be valid")
}

// ---------------------------------------------------------------------------
// Startup classification outcome
// ---------------------------------------------------------------------------

/// Successful restricted startup classification result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupOutcome {
    /// No database has been selected; Server is in restricted pre-operational state.
    UninitializedWithoutDatabase,
    /// A database has been selected; Server is in restricted pre-operational state.
    UninitializedWithDatabase,
    /// Init checkpoint is pending; Server exposes only Init reconciliation.
    InitializationPending(WorkflowKind),
    /// The deployment is sealed; Server serves its operational surface.
    Initialized,
}

/// Restricted startup state, including the process-lifetime state-root lock.
pub struct RestrictedStartup {
    outcome: StartupOutcome,
    /// Trusted state root the anchor set, Application Database, and Log Module
    /// local storage all live under.
    state_root: PathBuf,
    /// Shared so a lifecycle workflow builds its log destination from the same
    /// compiled-in catalog the process retains.
    log_catalog: Arc<LogModuleCatalog>,
    composition: PreoperationalComposition,
    /// Present only for a sealed deployment, which hands its loaded state and
    /// its open Application Database to the operational runtime.
    sealed: Option<SealedRuntime>,
    /// The process-wide owner shutdown closes the serving database through,
    /// whether operation was reached at startup or by an in-process Restore.
    active_database: ActiveDatabase,
}

/// A sealed deployment's loaded state and the open Application Database its
/// operational runtime owns.
///
/// The database was opened once, while the deployment was loaded under the
/// exclusive lifecycle permit, and is retained here for the process lifetime
/// rather than reopened by whatever serves it.
struct SealedRuntime {
    state: InitializedState,
    database: OperationalDatabase,
}

impl RestrictedStartup {
    /// Returns the lifecycle outcome used to select restricted routes.
    pub fn outcome(&self) -> StartupOutcome {
        self.outcome
    }

    /// Returns the trusted state root this startup is bound to.
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// Returns the compiled-in Log Module catalog retained for process lifetime.
    pub fn log_catalog(&self) -> &LogModuleCatalog {
        &self.log_catalog
    }

    /// Returns a sealed deployment's loaded application state, if any.
    pub fn initialized_state(&self) -> Option<&InitializedState> {
        self.sealed.as_ref().map(|sealed| &sealed.state)
    }

    /// Returns the Application Database a sealed deployment holds open, if any.
    pub fn application_database(&self) -> Option<&OperationalDatabase> {
        self.sealed.as_ref().map(|sealed| &sealed.database)
    }

    /// Returns the process-wide owner of whichever database is serving.
    pub fn active_database(&self) -> &ActiveDatabase {
        &self.active_database
    }

    /// Returns the deployment's at-rest protection capability.
    ///
    /// It is the arbiter itself, so sealing or opening an enrolled factor
    /// passes through the same serialized lifecycle authority a Restore holds
    /// while it replaces state.
    pub(crate) fn protection(&self) -> Arc<dyn ProtectedValueAccess> {
        Arc::clone(&self.composition.adapter.arbiter) as Arc<dyn ProtectedValueAccess>
    }
}

/// Shared lifecycle composition every mounted pre-operational route observes.
///
/// Cloning shares the same `WorkflowArbiter`, so the status route and the
/// selection route read and mutate one serialized lifecycle authority. A future
/// Init workflow must reuse this same arbiter, or selection and Init would no
/// longer serialize against each other.
#[derive(Clone)]
struct PreoperationalComposition {
    outcome: StartupOutcome,
    adapter: Arc<LifecycleAdapter>,
    catalog: Arc<BackendCatalog>,
    context: Arc<TrustedBackendContext>,
}

impl PreoperationalComposition {
    /// Returns a source that reads the projection asynchronously through the adapter.
    fn projection_source(&self) -> ProjectionSource {
        let adapter = Arc::clone(&self.adapter);
        Arc::new(move || {
            let adapter = Arc::clone(&adapter);
            Box::pin(async move { adapter.project().await })
        })
    }

    /// Returns the commit hook the selection route delegates its decision to.
    ///
    /// The returned projection is the one the arbiter observed under the same
    /// permit that committed the selection, so a later status read agrees with
    /// it. The opened database is dropped here; this milestone exposes no
    /// operational path that would use it.
    fn selection_commit(&self) -> SelectionCommit {
        let adapter = Arc::clone(&self.adapter);
        let catalog = Arc::clone(&self.catalog);
        let context = Arc::clone(&self.context);
        Arc::new(move |backend| {
            let adapter = Arc::clone(&adapter);
            let catalog = Arc::clone(&catalog);
            let context = Arc::clone(&context);
            Box::pin(async move { adapter.select(backend, catalog, context).await })
        })
    }
}

impl fmt::Debug for RestrictedStartup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RestrictedStartup")
            .field("outcome", &self.outcome)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Startup error
// ---------------------------------------------------------------------------

/// Stable failure category returned by restricted startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupError {
    /// The HTTPS listener address host configuration is not set.
    ListenerNotConfigured,
    /// The HTTPS listener address is malformed or has port zero.
    ListenerAddressInvalid,
    /// The TLS certificate path host configuration is not set.
    TlsCertificateNotConfigured,
    /// The TLS private-key path host configuration is not set.
    TlsPrivateKeyNotConfigured,
    /// TLS material is unsafe, unreadable, malformed, or incompatible.
    TlsMaterialInvalid,
    /// The configured HTTPS listener cannot be composed or bound.
    HttpsListenerUnavailable,
    /// The WEAVELIT_STATE_ROOT environment variable is not set.
    StateRootNotConfigured,
    /// The configured state root path is invalid.
    StateRootPathInvalid,
    /// The state root is currently in use by another Server instance.
    StateRootInUse,
    /// Lifecycle persistence could not complete.
    StorageOperationFailed,
    /// The Application Database is unavailable.
    DatabaseUnavailable,
    /// Anchor set is missing, malformed, or cannot be trusted.
    AnchorSetInvalid,
    /// Anchor format version is not supported.
    AnchorVersionUnsupported,
    /// Anchor binding does not match this deployment.
    AnchorBindingInvalid,
    /// Database integrity or schema cannot be trusted.
    DatabaseIntegrityFailure,
    /// Startup state combination is invalid or unsupported.
    StateCombinationInvalid,
    /// A retained new deployment requires operator redeployment.
    LifecycleInterruptedRedeployNew,
    /// A retained Restore requires operator redeployment.
    LifecycleInterruptedRedeployRestore,
    /// A retained state requires operator redeployment before a workflow decision.
    LifecycleInterruptedRedeployRequired,
    /// A signalled shutdown exceeded its budget or could not close cleanly.
    ShutdownIncomplete,
}

impl StartupError {
    /// Returns the stable category/reason pair for centralized error presentation.
    pub fn category_reason(&self) -> (&'static str, &'static str) {
        match self {
            Self::ListenerNotConfigured => ("configuration_invalid", "listener_not_configured"),
            Self::ListenerAddressInvalid => ("configuration_invalid", "listener_address_invalid"),
            Self::TlsCertificateNotConfigured => {
                ("configuration_invalid", "tls_certificate_not_configured")
            }
            Self::TlsPrivateKeyNotConfigured => {
                ("configuration_invalid", "tls_private_key_not_configured")
            }
            Self::TlsMaterialInvalid => ("configuration_invalid", "tls_material_invalid"),
            Self::HttpsListenerUnavailable => {
                ("preoperational_unavailable", "https_listener_unavailable")
            }
            Self::StateRootNotConfigured => ("configuration_invalid", "state_root_not_configured"),
            Self::StateRootPathInvalid => ("configuration_invalid", "state_root_path_invalid"),
            Self::StateRootInUse => ("preoperational_unavailable", "state_root_in_use"),
            Self::StorageOperationFailed => ("storage_unavailable", "storage_operation_failed"),
            Self::DatabaseUnavailable => ("storage_unavailable", "database_unavailable"),
            Self::AnchorSetInvalid => ("storage_integrity_failure", "anchor_set_invalid"),
            Self::AnchorVersionUnsupported => {
                ("storage_integrity_failure", "anchor_version_unsupported")
            }
            Self::AnchorBindingInvalid => ("storage_integrity_failure", "anchor_binding_invalid"),
            Self::DatabaseIntegrityFailure => {
                ("storage_integrity_failure", "database_integrity_failure")
            }
            Self::StateCombinationInvalid => {
                ("deployment_state_invalid", "state_combination_invalid")
            }
            Self::LifecycleInterruptedRedeployNew => {
                ("lifecycle_interrupted", "operator_redeploy_new")
            }
            Self::LifecycleInterruptedRedeployRestore => {
                ("lifecycle_interrupted", "operator_redeploy_restore")
            }
            Self::LifecycleInterruptedRedeployRequired => {
                ("lifecycle_interrupted", "operator_redeploy_required")
            }
            Self::ShutdownIncomplete => ("shutdown_incomplete", "shutdown_incomplete"),
        }
    }
}

// ---------------------------------------------------------------------------
// Restricted HTTPS listener and route composition
// ---------------------------------------------------------------------------

/// Binds and serves the sole direct-TLS pre-operational listener.
///
/// The shutdown trigger is supplied by the caller rather than installed here,
/// because deciding what asks this process to stop is process policy.
pub async fn run_restricted_https_listener(
    listener: TrustedHttpsListener,
    startup: RestrictedStartup,
    shutdown: ShutdownSignal,
) -> Result<(), StartupError> {
    let tcp_listener = TcpListener::bind(listener.address())
        .await
        .map_err(|_| StartupError::HttpsListenerUnavailable)?;
    let serving =
        serve_restricted_https_listener(tcp_listener, listener.tls_config(), &startup, shutdown)?;
    let result = serving.await;
    drop(startup);
    result
}

/// Composes the listener's serving modes and returns the future that serves them.
///
/// Composition is synchronous and the returned future owns every value it
/// needs, so the listener can be spawned independently of the startup it was
/// composed from. A sealed startup's open Application Database is shared into
/// that composition by handle rather than borrowed, but the startup still owns
/// the process-lifetime state-root lock, so the caller keeps it alive for as
/// long as it drives the returned future.
fn serve_restricted_https_listener(
    tcp_listener: TcpListener,
    tls_config: Arc<ServerConfig>,
    startup: &RestrictedStartup,
    shutdown: ShutdownSignal,
) -> Result<impl Future<Output = Result<(), StartupError>> + Send + use<>, StartupError> {
    // The expected request origin is the address actually bound, never a value
    // taken from a request header or a certificate subject alternative name.
    let bound_address = tcp_listener
        .local_addr()
        .map_err(|_| StartupError::HttpsListenerUnavailable)?;
    // The switch is created before any surface is composed, and starts closed.
    // Restore needs the publisher to move the listener between modes, and the
    // pre-operational surface needs Restore, so the publisher must exist first.
    // Nothing is served until the initial mode below is published.
    let (serving_mode_switch, serving_modes) = ServingModeSwitch::new(ServingMode::FailClosed(
        MountedSurface::without_registrations(fallback_router()),
    ));
    let serving_mode_switch = Arc::new(serving_mode_switch);
    let composer = PreoperationalComposer::new(startup, bound_address, &serving_mode_switch);
    composer.publish_initial(startup.composition.outcome);
    let active_database = startup.active_database.clone();
    // The same gate Init and Restore enter, so the stop this listener observes
    // is the one that refuses and waits for them.
    let transition_gate = Arc::clone(&startup.composition.adapter.transition_gate);

    Ok(async move {
        // Retained for the listener's lifetime so a later transition can still
        // republish a surface into the running listener.
        let _composer = composer;
        let tls_acceptor = TlsAcceptor::from(tls_config);

        accept_and_drain_connections(
            tcp_listener,
            tls_acceptor,
            serving_modes,
            shutdown,
            ShutdownBudget::DEFAULT,
            transition_gate,
            active_database,
        )
        .await
    })
}

/// Accepts connections until shutdown is signalled, then drains and closes.
///
/// Every accepted connection is tracked rather than detached, because a
/// shutdown that cannot observe an in-flight request cannot wait for its
/// response to be written. On the signal the listener stops accepting and drops
/// its bound socket, but keeps serving what it already accepted: those
/// connections hold the serving-mode snapshot they took when they were
/// accepted, so no republished mode could change what they serve, and cutting
/// them off would abandon a response the client is still owed.
///
/// An irreversible lifecycle transition is not a connection, so draining cannot
/// see it. The transition gate is closed on the signal and waited on beside the
/// drain, so a Restore or Init already past its point of no return finishes
/// before the process exits, and one that has not reached it is refused.
async fn accept_and_drain_connections(
    tcp_listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    serving_modes: watch::Receiver<ServingMode>,
    mut shutdown: ShutdownSignal,
    budget: ShutdownBudget,
    transition_gate: Arc<LifecycleTransitionGate>,
    active_database: ActiveDatabase,
) -> Result<(), StartupError> {
    let slots = ConnectionSlots::new();
    let rate_limiter = Arc::new(RateLimiter::new());
    let mut connections = JoinSet::new();

    let accepting: Result<(), StartupError> = loop {
        let accepted = tokio::select! {
            // Polled in order, so a shutdown already signalled always wins
            // against a connection that arrived at the same moment.
            biased;
            () = &mut shutdown.0 => {
                // Closed here, synchronously, so no lifecycle transition can
                // enter its irreversible region between the stop being
                // observed and the wait below being scheduled.
                transition_gate.begin_stopping();
                break Ok(());
            }
            accepted = tcp_listener.accept() => accepted,
            // Reaps finished connections while accepting, so a long-lived
            // listener does not accumulate their handles.
            Some(_) = connections.join_next(), if !connections.is_empty() => continue,
        };
        let Ok((stream, source)) = accepted else {
            break Err(StartupError::HttpsListenerUnavailable);
        };
        if !is_trusted_loopback_peer(source.ip()) {
            drop(stream);
            continue;
        }
        // Snapshot before spawning: an in-flight connection keeps serving
        // the surface it snapshotted, and only a newly accepted connection
        // sees a newer mode. The router and its transport registrations are
        // one value, so they are always snapshotted and swapped together.
        // The borrow guard is dropped at the end of this statement because
        // holding it across an await would block the publisher.
        let surface = serving_modes.borrow().surface().clone();
        if let Ok(connection_permit) = Arc::clone(&slots.normal).try_acquire_owned() {
            let tls_acceptor = tls_acceptor.clone();
            let rate_limiter = Arc::clone(&rate_limiter);
            connections.spawn(async move {
                serve_normal_connection(stream, source.ip(), tls_acceptor, surface, rate_limiter)
                    .await;
                drop(connection_permit);
            });
            continue;
        }

        let Ok(rejection_permit) = Arc::clone(&slots.rejection).try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let tls_acceptor = tls_acceptor.clone();

        connections.spawn(async move {
            serve_rejection_connection(stream, tls_acceptor).await;
            drop(rejection_permit);
        });
    };

    // Accepting has stopped, so the bound socket is released now rather than
    // held for the length of the drain.
    drop(tcp_listener);
    // Concurrent, not sequential: a connection that will not finish must not
    // spend a transition's allowance, and a transition that must not be
    // interrupted must not spend the drain's.
    let (drained, quiesced) = tokio::join!(
        async {
            timeout(budget.drain, async {
                while connections.join_next().await.is_some() {}
            })
            .await
            .is_ok()
        },
        transition_gate.quiesce(budget.transition_threshold),
    );
    // Retained until this shutdown returns, so nothing enters the transition
    // region behind the wait while the database is being closed. An overrun
    // does not release this guard or permit the close to race the transition.
    let transition = quiesced;
    // Whatever the drain did not finish is terminated, because the database
    // close must not wait behind a request that will not end.
    connections.shutdown().await;
    let closed = close_active_database(active_database, budget.database).await;

    match accepting {
        Err(error) => Err(error),
        Ok(()) if drained && !transition.overrun && closed => Ok(()),
        Ok(()) => Err(StartupError::ShutdownIncomplete),
    }
}

/// Closes the serving Application Database inside its own budget.
///
/// The close checkpoints and closes a real connection, which is blocking work,
/// so it runs on a blocking thread. A close that outlives its budget is left
/// there rather than delaying the process further, and is reported as an
/// unclean stop instead of a clean one.
async fn close_active_database(database: ActiveDatabase, budget: Duration) -> bool {
    matches!(
        timeout(budget, task::spawn_blocking(move || database.close())).await,
        Ok(Ok(Ok(())))
    )
}

fn is_trusted_loopback_peer(peer: IpAddr) -> bool {
    peer == IpAddr::V4(Ipv4Addr::LOCALHOST) || peer == IpAddr::V6(Ipv6Addr::LOCALHOST)
}

async fn serve_normal_connection(
    stream: TcpStream,
    source: IpAddr,
    tls_acceptor: TlsAcceptor,
    surface: MountedSurface,
    rate_limiter: Arc<RateLimiter>,
) {
    serve_normal_connection_with_timeouts(
        stream,
        source,
        tls_acceptor,
        surface,
        rate_limiter,
        ConnectionTimeouts {
            handshake: TLS_HANDSHAKE_TIMEOUT,
            request_read: REQUEST_READ_TIMEOUT,
            processing: REQUEST_PROCESSING_TIMEOUT,
        },
    )
    .await;
}

async fn serve_normal_connection_with_timeouts(
    stream: TcpStream,
    source: IpAddr,
    tls_acceptor: TlsAcceptor,
    surface: MountedSurface,
    rate_limiter: Arc<RateLimiter>,
    timeouts: ConnectionTimeouts,
) {
    let Ok(Ok(mut tls_stream)) = timeout(timeouts.handshake, tls_acceptor.accept(stream)).await
    else {
        return;
    };

    let response =
        process_restricted_request(&mut tls_stream, source, surface, rate_limiter, timeouts).await;
    write_response_and_acknowledge(&mut tls_stream, response, timeouts.processing).await;
}

async fn serve_rejection_connection(stream: TcpStream, tls_acceptor: TlsAcceptor) {
    serve_rejection_connection_with_timeouts(
        stream,
        tls_acceptor,
        TLS_HANDSHAKE_TIMEOUT,
        REQUEST_PROCESSING_TIMEOUT,
    )
    .await;
}

async fn serve_rejection_connection_with_timeouts(
    stream: TcpStream,
    tls_acceptor: TlsAcceptor,
    handshake_timeout: Duration,
    response_timeout: Duration,
) {
    let Ok(Ok(mut tls_stream)) = timeout(handshake_timeout, tls_acceptor.accept(stream)).await
    else {
        return;
    };
    let _ = timeout(
        response_timeout,
        write_bounded_response(&mut tls_stream, service_unavailable_response()),
    )
    .await;
}

/// Serves one request through the ordered admission chain.
///
/// The chain is: head read within its own absolute deadline, rate admission,
/// exact route classification against the mounted registrations, framing,
/// registered pre-body validation, admission permit, and only then a fallible
/// body allocation. Each stage consumes the previous stage's value, so the
/// ordering is enforced by the type system rather than by this comment.
async fn process_restricted_request<S>(
    stream: &mut S,
    source: IpAddr,
    surface: MountedSurface,
    rate_limiter: Arc<RateLimiter>,
    timeouts: ConnectionTimeouts,
) -> BoundedResponse
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let started = Deadline::now();
    // The head's deadline is absolute and is never extended by a route, a
    // registration, or an admitted body.
    let head_deadline = started + timeouts.request_read;
    // Everything between the head and the body allocation is either immediate
    // or a bounded wait for a route permit, and stays inside the connection's
    // own processing budget.
    let admission_deadline = started + timeouts.processing;

    let completed = match read_request_head_until(stream, head_deadline).await {
        RequestHeadRead::Completed(result) => *result,
        RequestHeadRead::Incomplete(error) => return response_for_request_read_error(error),
    };

    // A completed head consumes rate-limit quota whether or not it parsed.
    let head = match completed {
        Ok(head) => head,
        Err(error) => {
            return if rate_limiter.allows(source, Instant::now()) {
                response_for_request_read_error(error)
            } else {
                rate_limited_response()
            };
        }
    };

    let rate_admitted = match head.admit_rate(&rate_limiter, source, Instant::now()) {
        Ok(admitted) => admitted,
        Err(head) => {
            let mut response = rate_limited_response();
            // RFC 9110 §9.3.2: HEAD responses must not include a message body.
            if *head.method() == Method::HEAD {
                response.body = Bytes::new();
            }
            return response;
        }
    };

    let framed = match rate_admitted.classify(surface.registry()).check_framing() {
        Ok(framed) => framed,
        Err(error) => return response_for_request_read_error(error),
    };
    let validated = match framed.validate() {
        Ok(validated) => validated,
        Err(rejection) => return rejection.response(),
    };
    // A pre-body check may hand back the time already spent by an earlier
    // request in the same workflow. Everything after this point is capped at
    // that remainder, so a multi-request workflow cannot restart its own total
    // deadline by starting a new request.
    let inherited_deadline = validated
        .remaining_budget()
        .map(|remaining| Deadline::now() + remaining);
    let admitted = match timeout_at(admission_deadline, validated.acquire()).await {
        Ok(Ok(admitted)) => admitted,
        Ok(Err(rejection)) => return rejection.response(),
        Err(_) => return gateway_timeout_response(),
    };

    let profile = admitted.profile();
    let body_deadline = capped_deadline(
        profile.body_deadline(head_deadline, Deadline::now()),
        inherited_deadline,
    );
    let request = match timeout_at(body_deadline, admitted.read_body(stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => return response_for_request_read_error(error),
        Err(_) => return request_timeout_response(),
    };

    let is_head = *request.method() == Method::HEAD;
    let processing_deadline = ProcessingDeadline::new(capped_deadline(
        profile.processing_deadline(started, timeouts.processing, body_deadline),
        inherited_deadline,
    ));
    // Attached before dispatch and never recomputed: work a route hands to a
    // blocking pool outlives the future this timeout cancels, so it must be
    // able to observe the very instant the listener stops waiting.
    let mut request = request;
    request.extensions_mut().insert(processing_deadline);
    let router = surface.into_router();
    let mut bounded = processing_response(processing_deadline, async move {
        let response = router
            .oneshot(request)
            .await
            .expect("restricted router response is infallible");
        bounded_response_from_axum(response).await
    })
    .await;
    // RFC 9110 §9.3.2: HEAD responses must not include a message body.
    if is_head {
        bounded.body = Bytes::new();
    }
    bounded
}

/// Narrows a deadline to an inherited remainder, and never widens it.
///
/// A registered profile already bounds a request. An inherited budget can only
/// take time away, so a workflow that spans two requests cannot obtain more
/// total time than its first request started with.
fn capped_deadline(deadline: Deadline, inherited: Option<Deadline>) -> Deadline {
    match inherited {
        Some(inherited) => deadline.min(inherited),
        None => deadline,
    }
}
#[derive(Debug)]
enum RequestReadError {
    TimedOut,
    TargetTooLong,
    MethodNotAllowed,
    HeadersTooLarge,
    /// The host could not supply memory for an admitted body.
    BodyUnavailable,
    Invalid,
}

enum RequestHeadRead {
    Completed(Box<Result<HeadRead, RequestReadError>>),
    Incomplete(RequestReadError),
}

#[derive(Clone, Copy)]
enum RequestLineClassification {
    Get,
    Put,
    OtherMethod,
    Invalid,
}

enum RequestLineState {
    Method,
    Target(usize),
    Version,
    VersionEnding,
    Headers(RequestLineClassification),
    Invalid,
}

impl RequestLineState {
    fn observe(&mut self, byte: u8, bytes: &[u8]) -> bool {
        match self {
            Self::Method if byte == b' ' => *self = Self::Target(0),
            Self::Method if byte == b'\r' => *self = Self::Invalid,
            Self::Target(_) if byte == b' ' => *self = Self::Version,
            Self::Target(_) if byte == b'\r' => *self = Self::Invalid,
            Self::Target(length) => {
                *length += 1;
            }
            Self::Version if byte == b'\r' => *self = Self::VersionEnding,
            Self::Version => {}
            Self::VersionEnding if byte == b'\n' => {
                *self = Self::Headers(classify_completed_request_line(bytes));
            }
            Self::VersionEnding => *self = Self::Invalid,
            Self::Headers(_) | Self::Invalid | Self::Method => {}
        }
        matches!(self, Self::Target(length) if *length > MAX_REQUEST_TARGET_BYTES)
    }

    fn limit_error(&self, bytes: &[u8]) -> RequestReadError {
        match self {
            Self::Target(length) if *length > MAX_REQUEST_TARGET_BYTES => {
                RequestReadError::TargetTooLong
            }
            Self::Method | Self::Target(_) if request_line_has_non_get_method(bytes) => {
                RequestReadError::MethodNotAllowed
            }
            Self::Method
            | Self::Target(_)
            | Self::Version
            | Self::VersionEnding
            | Self::Invalid => RequestReadError::Invalid,
            Self::Headers(RequestLineClassification::OtherMethod) => {
                RequestReadError::MethodNotAllowed
            }
            Self::Headers(RequestLineClassification::Get | RequestLineClassification::Put) => {
                RequestReadError::HeadersTooLarge
            }
            Self::Headers(RequestLineClassification::Invalid) => RequestReadError::Invalid,
        }
    }

    fn completed_classification(&self) -> RequestLineClassification {
        match self {
            Self::Headers(classification) => *classification,
            _ => RequestLineClassification::Invalid,
        }
    }
}

fn classify_completed_request_line(bytes: &[u8]) -> RequestLineClassification {
    let Some(request_line) = bytes.strip_suffix(b"\r\n") else {
        return RequestLineClassification::Invalid;
    };
    let mut parts = request_line.split(|byte| *byte == b' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return RequestLineClassification::Invalid;
    };
    let Ok(target) = std::str::from_utf8(target) else {
        return RequestLineClassification::Invalid;
    };
    if method.is_empty()
        || target.is_empty()
        || !matches!(version, b"HTTP/1.0" | b"HTTP/1.1")
        || target.parse::<Uri>().is_err()
    {
        return RequestLineClassification::Invalid;
    }
    match Method::from_bytes(method) {
        Ok(method) if method == Method::GET => RequestLineClassification::Get,
        Ok(method) if method == Method::PUT => RequestLineClassification::Put,
        Ok(_) => RequestLineClassification::OtherMethod,
        Err(_) => RequestLineClassification::Invalid,
    }
}

fn request_line_has_non_get_method(bytes: &[u8]) -> bool {
    let method = bytes.split(|byte| *byte == b' ').next().unwrap_or(bytes);
    method != b"GET" && Method::from_bytes(method).is_ok()
}

/// Reads a request head within an absolute deadline.
///
/// The deadline is the head's own, so it is never extended by a route, a
/// registration, or an admitted body.
async fn read_request_head_until<S>(stream: &mut S, deadline: Deadline) -> RequestHeadRead
where
    S: AsyncRead + Unpin,
{
    match timeout_at(deadline, read_request_head_outcome(stream)).await {
        Ok(result) => result,
        Err(_) => RequestHeadRead::Incomplete(RequestReadError::TimedOut),
    }
}

/// Reads a complete request through the production admission chain.
///
/// Used by reader-level tests so they exercise the same stages the listener
/// runs rather than a parallel implementation.
#[cfg(test)]
async fn read_default_profile_request<S>(stream: &mut S) -> Result<Request, RequestReadError>
where
    S: AsyncRead + Unpin,
{
    let deadline = Deadline::now() + Duration::from_secs(30);
    let head = match read_request_head_until(stream, deadline).await {
        RequestHeadRead::Completed(result) => (*result)?,
        RequestHeadRead::Incomplete(error) => return Err(error),
    };
    head.admit_rate(
        &RateLimiter::new(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        Instant::now(),
    )
    .map_err(|_| RequestReadError::Invalid)?
    .classify(&transport::TransportRegistry::default())
    .check_framing()?
    .validate()
    .map_err(|_| RequestReadError::Invalid)?
    .acquire()
    .await
    .map_err(|_| RequestReadError::Invalid)?
    .read_body(stream)
    .await
}

async fn read_request_head_outcome<S>(stream: &mut S) -> RequestHeadRead
where
    S: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(MAX_REQUEST_HEAD_BYTES);
    let mut request_line = RequestLineState::Method;
    let mut target_too_long = false;
    loop {
        let byte = match stream.read_u8().await {
            Ok(byte) => byte,
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
                return RequestHeadRead::Incomplete(RequestReadError::Invalid);
            }
            Err(_) => return RequestHeadRead::Incomplete(RequestReadError::Invalid),
        };
        if byte == b'\n' && bytes.last().is_none_or(|previous| *previous != b'\r') {
            return RequestHeadRead::Incomplete(RequestReadError::Invalid);
        }
        bytes.push(byte);
        target_too_long |= request_line.observe(byte, &bytes);
        if bytes.len() > MAX_REQUEST_HEAD_BYTES {
            return RequestHeadRead::Incomplete(request_line.limit_error(&bytes));
        }
        if bytes.ends_with(b"\r\n\r\n") {
            let request = if target_too_long {
                Err(RequestReadError::TargetTooLong)
            } else {
                parse_http_request(&bytes, request_line.completed_classification())
                    .map(HeadRead::new)
            };
            return RequestHeadRead::Completed(Box::new(request));
        }
    }
}

/// Returns the exact number of body bytes a completed head declared.
///
/// Only `PUT` may carry a body, and only through exactly one canonical
/// `Content-Length` within the classified profile's bound. Every other method
/// must declare no body at all. Chunked framing and `Expect` are never
/// supported.
fn declared_request_body_length(
    method: &Method,
    headers: &HeaderMap,
    max_body_bytes: usize,
) -> Result<Option<usize>, RequestReadError> {
    if headers.contains_key(TRANSFER_ENCODING) || headers.contains_key(EXPECT) {
        return Err(RequestReadError::Invalid);
    }
    if method != Method::PUT {
        let declares_body = headers
            .get_all(CONTENT_LENGTH)
            .iter()
            .any(|value| content_length_digits(value) != Some(0));
        return if declares_body {
            Err(RequestReadError::Invalid)
        } else {
            Ok(None)
        };
    }
    let mut lengths = headers.get_all(CONTENT_LENGTH).iter();
    let (Some(value), None) = (lengths.next(), lengths.next()) else {
        return Err(RequestReadError::Invalid);
    };
    let Some(length) = canonical_content_length(value) else {
        return Err(RequestReadError::Invalid);
    };
    if length > max_body_bytes {
        return Err(RequestReadError::Invalid);
    }
    Ok(Some(length))
}

/// Parses a `Content-Length` that carries only decimal digits.
fn content_length_digits(value: &HeaderValue) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes.iter().try_fold(0_usize, |length, byte| {
        length
            .checked_mul(10)?
            .checked_add(usize::from(*byte - b'0'))
    })
}

/// Parses the single canonical decimal `Content-Length` a `PUT` must declare.
fn canonical_content_length(value: &HeaderValue) -> Option<usize> {
    if value.as_bytes().len() > 1 && value.as_bytes().starts_with(b"0") {
        return None;
    }
    content_length_digits(value)
}

/// Reports whether the peer already sent bytes beyond the declared body.
///
/// The probe never waits, so it cannot consume the shared read budget; the
/// connection serves exactly one request and is then closed.
async fn stream_has_pending_bytes<S>(stream: &mut S) -> bool
where
    S: AsyncRead + Unpin,
{
    let mut probe = [0_u8; 1];
    matches!(
        timeout(Duration::ZERO, stream.read(&mut probe)).await,
        Ok(Ok(1..))
    )
}

fn response_for_request_read_error(error: RequestReadError) -> BoundedResponse {
    match error {
        RequestReadError::TimedOut => request_timeout_response(),
        RequestReadError::TargetTooLong => {
            json_fixed_response(StatusCode::URI_TOO_LONG, "{\"error\":\"uri_too_long\"}")
        }
        RequestReadError::MethodNotAllowed => method_not_allowed_response(),
        RequestReadError::HeadersTooLarge => json_fixed_response(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "{\"error\":\"request_header_fields_too_large\"}",
        ),
        RequestReadError::BodyUnavailable => service_unavailable_response(),
        RequestReadError::Invalid => {
            json_fixed_response(StatusCode::BAD_REQUEST, "{\"error\":\"bad_request\"}")
        }
    }
}

fn parse_http_request(
    bytes: &[u8],
    request_line: RequestLineClassification,
) -> Result<Request, RequestReadError> {
    let raw_header_bytes = raw_header_section_bytes(bytes)?;
    if raw_header_bytes > MAX_REQUEST_HEADER_BYTES {
        return Err(RequestReadError::HeadersTooLarge);
    }
    let mut parsed_headers = vec![httparse::EMPTY_HEADER; raw_header_bytes / b"a:\r\n".len()];
    let mut parsed = httparse::Request::new(&mut parsed_headers);
    let httparse::Status::Complete(_) =
        parsed.parse(bytes).map_err(|_| RequestReadError::Invalid)?
    else {
        return Err(RequestReadError::Invalid);
    };
    let method = parsed.method.ok_or(RequestReadError::Invalid)?;
    let target = parsed.path.ok_or(RequestReadError::Invalid)?;
    if target.len() > MAX_REQUEST_TARGET_BYTES {
        return Err(RequestReadError::TargetTooLong);
    }
    let uri = target.parse().map_err(|_| RequestReadError::Invalid)?;
    let method = Method::from_bytes(method.as_bytes()).map_err(|_| RequestReadError::Invalid)?;
    let mut headers = HeaderMap::new();
    for header in parsed.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| RequestReadError::Invalid)?;
        let value = HeaderValue::from_bytes(header.value).map_err(|_| RequestReadError::Invalid)?;
        headers.append(name, value);
    }
    match request_line {
        RequestLineClassification::Get
        | RequestLineClassification::Put
        | RequestLineClassification::OtherMethod => {}
        RequestLineClassification::Invalid => return Err(RequestReadError::Invalid),
    }
    let mut request = Request::new(Body::empty());
    *request.method_mut() = method;
    *request.uri_mut() = uri;
    *request.headers_mut() = headers;
    Ok(request)
}

async fn bounded_response_from_axum(response: Response) -> BoundedResponse {
    let status = response.status();
    let allow = response
        .headers()
        .get(ALLOW)
        .and_then(AllowedMethod::from_header_value);
    // A cookie effect is a value, not header text. It is read here and
    // rendered by the listener, so the only cookies that can reach the wire
    // are the two the closed effect names.
    let effect = response.extensions().get::<CookieEffect>().cloned();
    // The post-write action is read here and run by the listener, so a route
    // cannot run it itself and cannot run it before its response is written.
    let acknowledgement = response
        .extensions()
        .get::<ResponseWriteAcknowledgement>()
        .cloned();
    // The typed profile serializes the route's envelope itself and ignores the
    // response body and every header the route set, so it can emit no
    // cross-origin header, message, path, trace, or dependency detail.
    if let Some(envelope) = response.extensions().get::<TypedJsonEnvelope>() {
        let body = envelope.serialize();
        if body.len() > ResponseProfile::TypedJson.max_body_bytes() {
            return redacted_response(status, allow);
        }
        // An effect that does not render is not emitted partially: the whole
        // response redacts to the fixed failure and carries no cookie.
        let cookies = match effect {
            Some(effect) => match effect.render() {
                Some(cookies) => Some(cookies),
                None => return redacted_response(status, allow),
            },
            None => None,
        };
        return BoundedResponse {
            status,
            profile: ResponseProfile::TypedJson,
            body: Bytes::from(body),
            allow,
            cookies,
            acknowledgement,
        };
    }
    // Only the typed profile may carry a cookie effect or a post-write action.
    // Either on any other response is an invalid composition, so it redacts
    // rather than silently dropping the effect and returning the route's body,
    // or acknowledging a write of a response the listener did not compose.
    if effect.is_some() || acknowledgement.is_some() {
        return redacted_response(status, allow);
    }
    let Some(profile) = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(ResponseProfile::from_media_type)
    else {
        return redacted_response(status, allow);
    };
    let Ok(body) = to_bytes(response.into_body(), profile.max_body_bytes()).await else {
        return redacted_response(status, allow);
    };
    if profile == ResponseProfile::Json {
        let Some(body) = fixed_json_body(&body) else {
            return redacted_response(status, allow);
        };
        return BoundedResponse {
            status,
            profile,
            body: Bytes::from_static(body.as_bytes()),
            allow,
            cookies: None,
            acknowledgement: None,
        };
    }
    BoundedResponse {
        status,
        profile,
        body,
        allow,
        cookies: None,
        acknowledgement: None,
    }
}

fn fixed_json_body(body: &[u8]) -> Option<&'static str> {
    match body {
        b"{\"error\":\"already_initialized\"}" => Some("{\"error\":\"already_initialized\"}"),
        b"{\"error\":\"backup_incompatible\"}" => Some("{\"error\":\"backup_incompatible\"}"),
        b"{\"error\":\"backup_invalid\"}" => Some("{\"error\":\"backup_invalid\"}"),
        b"{\"error\":\"bad_request\"}" => Some("{\"error\":\"bad_request\"}"),
        b"{\"error\":\"database_selection_not_allowed\"}" => {
            Some("{\"error\":\"database_selection_not_allowed\"}")
        }
        b"{\"error\":\"initialization_failed\"}" => Some("{\"error\":\"initialization_failed\"}"),
        b"{\"error\":\"method_not_allowed\"}" => Some("{\"error\":\"method_not_allowed\"}"),
        b"{\"error\":\"not_found\"}" => Some("{\"error\":\"not_found\"}"),
        b"{\"error\":\"recovery_key_confirmation_invalid\"}" => {
            Some("{\"error\":\"recovery_key_confirmation_invalid\"}")
        }
        b"{\"error\":\"recovery_key_confirmation_required\"}" => {
            Some("{\"error\":\"recovery_key_confirmation_required\"}")
        }
        b"{\"error\":\"recovery_key_invalid\"}" => Some("{\"error\":\"recovery_key_invalid\"}"),
        b"{\"error\":\"request_header_fields_too_large\"}" => {
            Some("{\"error\":\"request_header_fields_too_large\"}")
        }
        b"{\"error\":\"request_origin_denied\"}" => Some("{\"error\":\"request_origin_denied\"}"),
        b"{\"error\":\"restore_failed\"}" => Some("{\"error\":\"restore_failed\"}"),
        b"{\"error\":\"restore_not_allowed\"}" => Some("{\"error\":\"restore_not_allowed\"}"),
        b"{\"error\":\"restore_pending\"}" => Some("{\"error\":\"restore_pending\"}"),
        b"{\"error\":\"restore_ticket_invalid\"}" => Some("{\"error\":\"restore_ticket_invalid\"}"),
        b"{\"error\":\"service_unavailable\"}" => Some("{\"error\":\"service_unavailable\"}"),
        b"{\"error\":\"uri_too_long\"}" => Some("{\"error\":\"uri_too_long\"}"),
        b"{\"lifecycle\":\"uninitialized\",\"database_selected\":false}" => {
            Some("{\"lifecycle\":\"uninitialized\",\"database_selected\":false}")
        }
        b"{\"lifecycle\":\"uninitialized\",\"database_selected\":true}" => {
            Some("{\"lifecycle\":\"uninitialized\",\"database_selected\":true}")
        }
        _ => None,
    }
}

/// Replaces an unknown, unbounded, or otherwise invalid module response with the
/// fixed redacted body while preserving the observed status framing.
fn redacted_response(status: StatusCode, allow: Option<AllowedMethod>) -> BoundedResponse {
    BoundedResponse {
        allow,
        ..BoundedResponse::json(status, "{\"error\":\"gateway_timeout\"}")
    }
}

async fn write_bounded_response<S>(stream: &mut S, response: BoundedResponse) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let allow = response.allow.map_or("", AllowedMethod::allow_header);
    let cookies = response.cookies.as_ref().map_or("", CookieLines::as_str);
    let head = format!(
        "HTTP/1.1 {} \r\nContent-Type: {}\r\n{}{}{}\r\n",
        response.status.as_u16(),
        response.profile.media_type(),
        allow,
        cookies,
        response.profile.security_headers(),
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}

/// Writes a bounded response and runs its post-write action only on success.
///
/// The action is taken out of the response before the write consumes it and is
/// run only when the whole write, including the connection shutdown, completed
/// inside the response budget. A write failure, a peer that vanished before the
/// shutdown, and an expired budget all leave it unrun, so a workflow that
/// gates capability on it stays fail closed whenever delivery did not visibly
/// complete.
async fn write_response_and_acknowledge<S>(
    stream: &mut S,
    mut response: BoundedResponse,
    write_timeout: Duration,
) where
    S: AsyncWrite + Unpin,
{
    let acknowledgement = response.acknowledgement.take();
    let written = matches!(
        timeout(write_timeout, write_bounded_response(stream, response)).await,
        Ok(Ok(()))
    );
    if written && let Some(acknowledgement) = acknowledgement {
        acknowledgement.run();
    }
}

async fn processing_response<F>(
    processing_deadline: ProcessingDeadline,
    processing: F,
) -> BoundedResponse
where
    F: Future<Output = BoundedResponse>,
{
    match timeout_at(processing_deadline.instant(), processing).await {
        Ok(response) => response,
        Err(_) => gateway_timeout_response(),
    }
}

fn json_fixed_response(status: StatusCode, body: &'static str) -> BoundedResponse {
    BoundedResponse::json(status, body)
}

fn service_unavailable_response() -> BoundedResponse {
    json_fixed_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "{\"error\":\"service_unavailable\"}",
    )
}

fn method_not_allowed_response() -> BoundedResponse {
    BoundedResponse {
        allow: Some(AllowedMethod::Get),
        ..BoundedResponse::json(
            StatusCode::METHOD_NOT_ALLOWED,
            "{\"error\":\"method_not_allowed\"}",
        )
    }
}

fn request_timeout_response() -> BoundedResponse {
    json_fixed_response(
        StatusCode::REQUEST_TIMEOUT,
        "{\"error\":\"request_timeout\"}",
    )
}

fn gateway_timeout_response() -> BoundedResponse {
    json_fixed_response(
        StatusCode::GATEWAY_TIMEOUT,
        "{\"error\":\"gateway_timeout\"}",
    )
}

fn rate_limited_response() -> BoundedResponse {
    json_fixed_response(
        StatusCode::TOO_MANY_REQUESTS,
        "{\"error\":\"rate_limited\"}",
    )
}

/// The surface the listener currently serves.
///
/// Every mode starts from the same fixed not-found fallback, so a mode serves
/// exactly the routes its Client Module surface declared and nothing else. Each
/// mode carries its router and its transport registrations as one value, so a
/// registration can never describe a route the published mode did not mount.
enum ServingMode {
    /// The pre-operational Client Module surface permitted before sealing.
    PreOperational(MountedSurface),
    /// No functional route at all; every valid request receives not-found.
    FailClosed(MountedSurface),
    /// The sealed deployment's operational Client Module surface.
    ///
    /// This variant carries the composed mount rather than a bare surface, so
    /// the operational mode cannot be built from a router that was separated
    /// from the registrations composed with it.
    Operational(OperationalMount),
}

impl ServingMode {
    /// Returns the router and registrations this mode serves.
    fn surface(&self) -> &MountedSurface {
        match self {
            Self::PreOperational(surface) | Self::FailClosed(surface) => surface,
            Self::Operational(mount) => mount.surface(),
        }
    }

    #[cfg(test)]
    fn router(&self) -> &Router {
        self.surface().router()
    }
}

/// Publisher half of the listener's serving-mode switch.
///
/// Restore holds this to move a running listener from its pre-operational
/// surface to fail-closed and then to the sealed deployment's operational
/// surface without a restart. It owns no lifecycle authority: a caller must
/// already have completed the trusted transition it is publishing.
pub struct ServingModeSwitch {
    modes: watch::Sender<ServingMode>,
}

impl ServingModeSwitch {
    /// Creates the switch and the receiver the listener reads before each connection.
    fn new(initial: ServingMode) -> (Self, watch::Receiver<ServingMode>) {
        let (modes, receiver) = watch::channel(initial);
        (Self { modes }, receiver)
    }

    /// Serves no functional route from the next accepted connection onward.
    pub fn publish_fail_closed(&self) {
        let _ = self.modes.send(ServingMode::FailClosed(
            MountedSurface::without_registrations(fallback_router()),
        ));
    }

    /// Serves the composed operational surface from the next accepted
    /// connection onward.
    ///
    /// The mount is produced by the operational composer and by nothing else,
    /// so the router and every registration that admits one of its routes are
    /// published as the one value they were composed as.
    pub fn publish_operational(&self, mount: OperationalMount) {
        let _ = self.modes.send(ServingMode::Operational(mount));
    }

    /// Serves a newly composed pre-operational surface from the next accepted
    /// connection onward.
    ///
    /// Selecting an Application Database makes Restore eligible, and Restore is
    /// mounted only where it is eligible. Republishing the whole surface is
    /// what makes it reachable without a restart.
    fn publish_preoperational(&self, surface: MountedSurface) {
        let _ = self.modes.send(ServingMode::PreOperational(surface));
    }
}

/// The router every serving mode starts from: only the fixed not-found fallback.
fn fallback_router() -> Router {
    Router::new().fallback(not_found)
}

/// The Server's compiled-in component inventory.
///
/// A pre-operational workflow is judged against what this build can actually
/// serve. The names come from the module crates themselves rather than from
/// string literals restated here, so a compiled-in module and the inventory it
/// is judged by cannot drift apart. An MFA Module carries the factor-data
/// format it declares alongside its name, and a Log Module carries the settings
/// its factory declares it accepts, both read through the same registrations the
/// runtime serves from. This build compiles in one Client Module, one Log
/// Module, and one MFA Module, and no Service Module or named operation.
fn server_components() -> AvailableComponents {
    fn named(identifier: &str) -> BTreeSet<Name> {
        Name::new(identifier).into_iter().collect()
    }

    let totp = weavelit_module_mfa_totp::registration();

    AvailableComponents {
        client_modules: named(weavelit_module_client_webui::MODULE_IDENTIFIER),
        log_modules: sqlite_log_catalog()
            .declarations()
            .filter_map(|declaration| {
                let module = Name::new(declaration.identifier().as_str()).ok()?;
                let accepted_keys = declaration
                    .accepted_settings()
                    .keys()
                    .map(str::to_owned)
                    .collect();
                Some((module, LogSettingsFormat { accepted_keys }))
            })
            .collect(),
        mfa_modules: named(totp.identifier())
            .into_iter()
            .map(|module| {
                (
                    module,
                    MfaFactorFormat {
                        factor_data_bytes: totp.secret_length(),
                    },
                )
            })
            .collect(),
        service_modules: BTreeSet::new(),
        operations: BTreeSet::new(),
    }
}

/// Composes and publishes the pre-operational surface of a running listener.
///
/// The Web UI Client Module declares which capabilities it supplies; this
/// composer supplies the trusted lifecycle authority and the Init and Restore
/// collaborators behind them, and pairs each mounted route with the transport
/// registration that admits its body. Every mounted path is exact, so an
/// unknown target, including any `/api/` target, falls through to the fixed
/// not-found response.
struct PreoperationalComposer {
    composition: PreoperationalComposition,
    listener: SocketAddr,
    orchestrator: Arc<restore::RestoreOrchestrator>,
    /// Absent only for a build whose administration Client Module is not
    /// compiled in, which can declare no Init at all.
    init: Option<Arc<init::InitOrchestrator>>,
    serving_modes: Arc<ServingModeSwitch>,
    /// Present only for a startup that classified an already-sealed record.
    /// It owns the Application Database that startup handed over, so the
    /// operational surface it composes outlives this composition only through
    /// the mount it published.
    operational: Option<OperationalComposer>,
}

impl PreoperationalComposer {
    fn new(
        startup: &RestrictedStartup,
        listener: SocketAddr,
        serving_modes: &Arc<ServingModeSwitch>,
    ) -> Arc<Self> {
        Self::with_clock(startup, listener, serving_modes, init::system_event_clock())
    }

    /// Composes against an explicit event clock.
    ///
    /// Production always supplies the host clock through [`Self::new`]. The
    /// clock is a parameter only so a test can assert the exact time an Init
    /// completion record carries without reading the wall clock it is
    /// asserting against.
    fn with_clock(
        startup: &RestrictedStartup,
        listener: SocketAddr,
        serving_modes: &Arc<ServingModeSwitch>,
        clock: init::EventClock,
    ) -> Arc<Self> {
        let components = server_components();
        // Every sealing path composes its operational surface from this one
        // value, so a deployment sealed at startup, a deployment sealed by an
        // Init, and a deployment sealed by a Restore serve the same routes
        // against the same authority.
        let operational_runtime = Arc::new(OperationalRuntime {
            listener,
            state_root: startup.state_root.clone(),
            log_catalog: Arc::clone(&startup.log_catalog),
            client_modules: components.client_modules.clone(),
            active_database: startup.active_database.clone(),
            protection: startup.protection(),
        });

        Arc::new(Self {
            composition: startup.composition.clone(),
            listener,
            orchestrator: restore::RestoreOrchestrator::new(
                startup,
                components.clone(),
                Arc::clone(serving_modes),
                Arc::clone(&operational_runtime),
            ),
            init: init::InitOrchestrator::with_clock(
                startup,
                components,
                Arc::clone(serving_modes),
                Arc::clone(&operational_runtime),
                clock,
            ),
            serving_modes: Arc::clone(serving_modes),
            operational: startup.sealed.as_ref().map(|sealed| {
                OperationalComposer::new(
                    Arc::clone(&operational_runtime),
                    &sealed.state,
                    sealed.database.clone(),
                )
            }),
        })
    }

    /// Publishes the surface a classified startup begins with.
    ///
    /// The fail-closed mode mounts no route at all and therefore carries no
    /// transport registration. The pre-operational mode registers Init
    /// preparation and Restore when the deployment is eligible for them, and
    /// the operational mode is composed by the operational composer, which
    /// pairs every Server-owned route it mounts with the registration that
    /// admits it. Neither mounts Init or Restore.
    ///
    /// A record already inside an Init cannot be resumed. A restart over one
    /// never reaches this composer at all, because startup classification fails
    /// closed before a listener is bound.
    fn publish_initial(self: &Arc<Self>, outcome: StartupOutcome) {
        match outcome {
            StartupOutcome::UninitializedWithoutDatabase => self.publish(false),
            StartupOutcome::UninitializedWithDatabase => self.publish(true),
            StartupOutcome::InitializationPending(_) => self.serving_modes.publish_fail_closed(),
            // A sealed classification always loaded its deployment, so the
            // composer is present. A startup that reports sealed without one
            // serves nothing rather than an operational surface with no
            // Application Database behind it.
            StartupOutcome::Initialized => match &self.operational {
                Some(operational) => self.serving_modes.publish_operational(operational.mount()),
                None => self.serving_modes.publish_fail_closed(),
            },
        }
    }

    fn publish(self: &Arc<Self>, database_selected: bool) {
        self.serving_modes
            .publish_preoperational(self.surface(database_selected));
    }

    /// Composes the pre-operational surface, mounting Init preparation and
    /// Restore only when the deployment has already selected an Application
    /// Database.
    ///
    /// Init finalization is deliberately absent here. It becomes reachable only
    /// once a recovery key has actually been written to a client, through
    /// [`Self::publish_finalization`].
    fn surface(self: &Arc<Self>, database_selected: bool) -> MountedSurface {
        let expected_origin = ExpectedOrigin::from_listener(self.listener);
        let restore = database_selected.then(|| self.orchestrator.capability(expected_origin));
        let init = database_selected
            .then(|| {
                self.init
                    .as_ref()
                    .map(|init| init.capability(expected_origin))
            })
            .flatten();
        let (declared, restore) = weavelit_module_client_webui::preoperational_surface(
            self.composition.projection_source(),
            expected_origin,
            self.selection_commit(),
            restore,
            init,
        )
        .split_restore();
        let (declared, init) = declared.split_init();

        let mut surface = MountedSurface::without_registrations(declared.mount(fallback_router()));
        if let Some(restore) = restore {
            let key_route = restore.key_route();
            let artifact_route = restore.artifact_route();
            surface = surface
                .with_capability(TransportCapability::new(
                    self.orchestrator.key_registration(expected_origin),
                    move |router| router.route(weavelit_module_client::RESTORE_ROUTE, key_route),
                ))
                .with_capability(TransportCapability::new(
                    self.orchestrator.artifact_registration(expected_origin),
                    move |router| {
                        router.route(
                            weavelit_module_client::RESTORE_ARTIFACT_ROUTE,
                            artifact_route,
                        )
                    },
                ));
        }
        if let (Some(init), Some(orchestrator)) = (init, self.init.as_ref()) {
            // The declared route is wrapped so the one response that really
            // delivered a key, and no other, carries the post-write action that
            // publishes finalization.
            let key_route =
                init::delivering_route(init.recovery_key_route(), self.delivery_publication());
            surface = surface.with_capability(TransportCapability::new(
                orchestrator.recovery_key_registration(expected_origin),
                move |router| {
                    router.route(weavelit_module_client::INIT_RECOVERY_KEY_ROUTE, key_route)
                },
            ));
        }
        surface
    }

    /// Composes the surface this Server serves while a delivered recovery key
    /// awaits its finalization request.
    ///
    /// Only finalization is mounted. The status projection, Application
    /// Database selection, Restore, and browser asset delivery are absent, so
    /// the already-loaded page that holds the key has exactly one call left to
    /// make and a new client has none.
    fn finalization_surface(
        self: &Arc<Self>,
        orchestrator: &Arc<init::InitOrchestrator>,
    ) -> MountedSurface {
        let expected_origin = ExpectedOrigin::from_listener(self.listener);
        let (declared, init) = weavelit_module_client_webui::finalization_surface(
            orchestrator.capability(expected_origin),
        )
        .split_init();

        let mut surface = MountedSurface::without_registrations(declared.mount(fallback_router()));
        if let Some(init) = init {
            let route = init.finalize_route();
            surface = surface.with_capability(TransportCapability::new(
                orchestrator.finalization_registration(expected_origin),
                move |router| router.route(weavelit_module_client::INIT_ROUTE, route),
            ));
        }
        surface
    }

    /// Returns the action the listener runs once a key response is written.
    ///
    /// This is the same republication mechanism a committed database selection
    /// uses: a commit that changes what may be mounted republishes the surface
    /// rather than mounting a route that then denies itself.
    fn delivery_publication(self: &Arc<Self>) -> init::DeliveryPublication {
        let composer = Arc::clone(self);
        Arc::new(move || composer.publish_finalization())
    }

    /// Promotes a written delivery and publishes the finalization surface.
    ///
    /// Nothing is published unless the delivery was actually promoted, so a
    /// write failure, a disconnect, and an expired budget all leave this Server
    /// fail closed with no finalization route at all.
    fn publish_finalization(self: &Arc<Self>) {
        let Some(orchestrator) = self.init.clone() else {
            return;
        };
        if !orchestrator.acknowledge_delivery() {
            return;
        }
        self.serving_modes
            .publish_preoperational(self.finalization_surface(&orchestrator));
    }

    /// Wraps the lifecycle selection commit so a successful selection also
    /// republishes the surface that selection made Init and Restore eligible
    /// on.
    fn selection_commit(self: &Arc<Self>) -> SelectionCommit {
        let composer = Arc::clone(self);
        let commit = self.composition.selection_commit();
        Arc::new(move |backend| {
            let composer = Arc::clone(&composer);
            let commit = Arc::clone(&commit);
            Box::pin(async move {
                let projection = commit(backend).await?;
                if projection.database_selected() {
                    composer.publish(true);
                }
                Ok(projection)
            })
        })
    }
}

async fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, "not_found")
}

fn raw_header_section_bytes(bytes: &[u8]) -> Result<usize, RequestReadError> {
    let request_line_end = bytes
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or(RequestReadError::Invalid)?
        + 2;
    let section_end = bytes
        .len()
        .checked_sub(2)
        .ok_or(RequestReadError::Invalid)?;
    section_end
        .checked_sub(request_line_end)
        .ok_or(RequestReadError::Invalid)
}

fn json_response(status: StatusCode, error: &'static str) -> Response {
    let body = match error {
        "bad_request" => "{\"error\":\"bad_request\"}",
        "method_not_allowed" => "{\"error\":\"method_not_allowed\"}",
        "not_found" => "{\"error\":\"not_found\"}",
        "request_header_fields_too_large" => "{\"error\":\"request_header_fields_too_large\"}",
        "uri_too_long" => "{\"error\":\"uri_too_long\"}",
        _ => unreachable!("all pre-operational error responses use fixed codes"),
    };
    json_response_body(status, body)
}

fn json_response_body(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(body))
        .expect("fixed pre-operational responses must be valid")
}

// ---------------------------------------------------------------------------
// Startup composition
// ---------------------------------------------------------------------------

/// Reads the trusted state root from host/process configuration.
///
/// Returns `Err(StartupError::StateRootNotConfigured)` if the environment
/// variable is absent, or `Err(StartupError::StateRootPathInvalid)` if the
/// value is empty or not an absolute path string.
pub fn read_state_root() -> Result<PathBuf, StartupError> {
    let value = env::var(STATE_ROOT_ENV).map_err(|_| StartupError::StateRootNotConfigured)?;
    if value.is_empty() {
        return Err(StartupError::StateRootPathInvalid);
    }
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(StartupError::StateRootPathInvalid);
    }
    Ok(path)
}

/// Maps a lifecycle open/creation error to a startup error category.
fn map_open_error(error: LifecycleError) -> StartupError {
    match error {
        LifecycleError::ConfigurationInvalid => StartupError::StateRootPathInvalid,
        LifecycleError::LockContended => StartupError::StateRootInUse,
        LifecycleError::Persistence => StartupError::StorageOperationFailed,
        LifecycleError::IntegrityFailure => StartupError::AnchorSetInvalid,
        LifecycleError::UnsupportedVersion => StartupError::AnchorVersionUnsupported,
        LifecycleError::DeploymentMismatch => StartupError::AnchorBindingInvalid,
        LifecycleError::InvalidState => StartupError::AnchorSetInvalid,
        _ => StartupError::StorageOperationFailed,
    }
}

/// Maps a lifecycle classification error to a startup error category.
fn map_classification_error(error: LifecycleError) -> StartupError {
    match error {
        LifecycleError::DependencyUnavailable => StartupError::DatabaseUnavailable,
        LifecycleError::IntegrityFailure => StartupError::DatabaseIntegrityFailure,
        LifecycleError::DeploymentMismatch => StartupError::AnchorBindingInvalid,
        LifecycleError::ConfigurationInvalid => StartupError::StateRootPathInvalid,
        LifecycleError::InvalidState => StartupError::StateCombinationInvalid,
        LifecycleError::UnsupportedVersion => StartupError::AnchorVersionUnsupported,
        LifecycleError::Persistence => StartupError::StorageOperationFailed,
        _ => StartupError::StorageOperationFailed,
    }
}

/// Maps a sealed-deployment load failure to a startup error category.
fn map_workflow_error(error: WorkflowError) -> StartupError {
    match error {
        WorkflowError::Lifecycle(error) => map_classification_error(error),
        _ => StartupError::StateCombinationInvalid,
    }
}

/// Maps a lifecycle classification to the surface the runtime may serve.
fn startup_outcome(
    classification: LifecycleClassification,
) -> Result<StartupOutcome, StartupError> {
    Ok(match classification {
        LifecycleClassification::UninitializedWithoutDatabase => {
            StartupOutcome::UninitializedWithoutDatabase
        }
        LifecycleClassification::UninitializedWithDatabase => {
            StartupOutcome::UninitializedWithDatabase
        }
        LifecycleClassification::InitializationPending(kind) => {
            StartupOutcome::InitializationPending(kind)
        }
        LifecycleClassification::Initialized => StartupOutcome::Initialized,
        LifecycleClassification::Interrupted(action) => {
            return Err(match action {
                InterruptedLifecycleAction::RedeployNew => {
                    StartupError::LifecycleInterruptedRedeployNew
                }
                InterruptedLifecycleAction::RedeployRestore => {
                    StartupError::LifecycleInterruptedRedeployRestore
                }
                InterruptedLifecycleAction::RedeployRequired => {
                    StartupError::LifecycleInterruptedRedeployRequired
                }
            });
        }
        // Post-commit reconciliation is not implemented, so it fails closed.
        LifecycleClassification::PostCommitReconciliationRequired => {
            return Err(StartupError::StateCombinationInvalid);
        }
    })
}

/// Composes the lifecycle crate and SQLite backend, opens or creates the anchor
/// set, and classifies startup state.
///
/// Returns a `StartupOutcome` for every supported state. A sealed deployment is
/// classified from its record alone and then loads its application state under
/// the lifecycle mutation permit, which reopens the Application Database
/// read-write so a retained write-ahead log is recovered by the backend rather
/// than treated as an uninspectable artifact. That load is the authority: a
/// database that cannot be recovered, is corrupt, is bound to another
/// deployment, or is not completely initialized fails startup closed before any
/// listener binds. The retained open database is kept for the process lifetime.
/// `PostCommitReconciliationRequired` fails closed because reconciliation is not
/// implemented.
pub fn classify_restricted_startup(state_root: &Path) -> Result<RestrictedStartup, StartupError> {
    let store = LifecycleStore::open_or_create(state_root).map_err(map_open_error)?;

    let catalog = Arc::new(sqlite_catalog());
    let context = Arc::new(TrustedBackendContext::new(
        state_root.join(APPLICATION_DATABASE_FILE),
    ));

    let classification = store
        .classify_startup(&catalog, &context)
        .map_err(map_classification_error)?;

    let outcome = startup_outcome(classification)?;
    let arbiter = Arc::new(WorkflowArbiter::new(store));
    // The sealed deployment's state is loaded under the lifecycle mutation
    // permit, which independently re-verifies the record and the database. The
    // database it opened to do that is retained rather than reopened later.
    let sealed = match outcome {
        StartupOutcome::Initialized => {
            let (state, database) = OperationalDatabase::from_sealed(
                arbiter
                    .load_sealed_deployment(&catalog, &context)
                    .map_err(map_workflow_error)?,
            );
            Some(SealedRuntime { state, database })
        }
        _ => None,
    };
    Ok(RestrictedStartup {
        outcome,
        state_root: state_root.to_path_buf(),
        log_catalog: Arc::new(sqlite_log_catalog()),
        // The arbiter takes ownership of the store, so it also retains the
        // process-lifetime state-root lock.
        composition: PreoperationalComposition {
            outcome,
            adapter: Arc::new(LifecycleAdapter::new(arbiter)),
            catalog,
            context,
        },
        sealed,
        active_database: ActiveDatabase::default(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        ffi::OsString,
        future::pending,
        io,
        net::SocketAddr,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::{Path, PathBuf},
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll},
        time::Duration,
    };

    use axum::{
        Router,
        body::Body,
        http::{HeaderValue, Method, Request, StatusCode, header::CONTENT_TYPE},
        response::{Html, Response},
        routing::any,
    };
    use http_body_util::BodyExt;
    use rustls::{
        ClientConfig, RootCertStore, ServerConfig,
        crypto::aws_lc_rs,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
        net::{TcpListener, TcpSocket, TcpStream},
        sync::{Notify, Semaphore, mpsc, oneshot, watch},
        task::JoinHandle,
    };
    use tokio_rustls::{TlsAcceptor, TlsConnector};
    use tower::ServiceExt;
    use weavelit_module_client::{
        APPLICATION_DATABASE_ROUTE, AUTH_LOGIN_ROUTE, AUTH_LOGOUT_ROUTE,
        AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE, AUTH_MFA_ENROLLMENT_ROUTE,
        AUTH_MFA_SELF_ENROLLMENT_ROUTE, AUTH_MFA_VERIFY_ROUTE, AUTH_SESSION_ROUTE,
        DatabaseSelectionRejection, ExpectedOrigin, INIT_RECOVERY_KEY_ROUTE, INIT_ROUTE,
        InitRejection, RESTORE_ARTIFACT_ROUTE, RESTORE_ROUTE, RESTORE_TICKET_HEADER_NAME,
        RestoreDeclaration, RestoreRejection, STATUS_ROUTE,
    };
    use weavelit_server_database::{
        ApplicationStateInput, CompletionObligation, CorrelationIdentifier, LogAssignment,
        LogClassification, LogDetail, LogModuleConfiguration, LogType, Name, RecoveryPublicKey,
    };
    use weavelit_server_lifecycle::{
        ApplicationState, BackendIdentifier, CheckpointMetadata, DatabaseInspection,
        DeploymentIdentifier, InitializedState, LifecycleClassification, LifecycleProjection,
        LifecycleState, LifecycleStore, StateIdentifier, TrustedBackendContext, WorkflowKind,
    };
    use weavelit_server_restore::{
        AvailableComponents, RequestBudget, RequestDeadline, RestoreError,
    };
    use zeroize::Zeroizing;

    use super::{
        APPLICATION_DATABASE_FILE, ASSET_SECURITY_HEADERS, AllowedMethod, BoundedResponse,
        ConnectionSlots, ConnectionTimeouts, DatabaseError, Deadline, LifecycleTransitionGate,
        MAX_JSON_BODY_BYTES, MAX_REQUEST_BODY_BYTES, MAX_TYPED_JSON_BODY_BYTES, RATE_LIMIT_BURST,
        RATE_LIMIT_REQUESTS_PER_MINUTE, REQUEST_PROCESSING_TIMEOUT, REQUEST_READ_TIMEOUT,
        RateLimiter, RequestReadError, ResponseProfile, ResponseWriteAcknowledgement,
        RestrictedStartup, SHUTDOWN_DATABASE_BUDGET, SHUTDOWN_LIFECYCLE_TRANSITION_THRESHOLD,
        ServingMode, ServingModeSwitch, ShutdownBudget, ShutdownSignal, StartupError,
        StartupOutcome, TLS_HANDSHAKE_TIMEOUT, WorkflowArbiter, accept_and_drain_connections,
        bounded_response_from_axum, classify_restricted_startup, close_active_database,
        fallback_router, gateway_timeout_response,
        operational::{
            ActiveDatabase, OperationalComposer, OperationalMount, OperationalRuntime, test_support,
        },
        parse_http_request, processing_response, raw_header_section_bytes,
        read_default_profile_request, read_request_head_until, redacted_response,
        request_timeout_response,
        restore::RestoreOrchestrator,
        serve_normal_connection_with_timeouts, serve_rejection_connection_with_timeouts,
        server_components, sqlite_catalog, startup_outcome,
        transport::{
            BodyAdmission, MountedSurface, ProcessingDeadline, TransportCapability,
            TransportProfile, TransportRegistration,
        },
        typed_json::{
            RecoveryKeyLine, ResponseCorrelation, StableCode, TypedJsonEnvelope, TypedResult,
            TypedValue, typed_json_response,
        },
        write_response_and_acknowledge,
    };

    /// Listener authority used by router-level tests that bind no socket.
    pub(crate) const UNBOUND_LISTENER: &str = "127.0.0.1:8443";

    /// The exact accepted Application Database selection body.
    const SELECTION_BODY: &str = "{\"backend\":\"sqlite\",\"settings\":{}}";

    /// Composes and publishes the serving mode a listener starts a startup on.
    ///
    /// The listener creates its switch closed and lets the pre-operational
    /// composer publish the first real mode, so a test that needs the initial
    /// mode composes it exactly the same way.
    pub(crate) fn published_serving_modes(
        startup: &RestrictedStartup,
        listener: SocketAddr,
    ) -> (Arc<ServingModeSwitch>, watch::Receiver<ServingMode>) {
        let (switch, modes) = ServingModeSwitch::new(ServingMode::FailClosed(
            MountedSurface::without_registrations(fallback_router()),
        ));
        let switch = Arc::new(switch);
        super::PreoperationalComposer::new(startup, listener, &switch)
            .publish_initial(startup.composition.outcome);
        (switch, modes)
    }

    /// Composes the operational mount exactly as a sealed startup does.
    ///
    /// The startup must be a genuinely sealed one, because the composer owns
    /// the Application Database that startup handed over.
    fn operational_mount(startup: &RestrictedStartup) -> OperationalMount {
        operational_composer(startup, "127.0.0.1:8443".parse().unwrap()).mount()
    }

    /// The exact transport registrations a sealed deployment's surface carries.
    ///
    /// This build serves one Server-owned operational family, authentication,
    /// and each of its three routes is registered for `PUT` alone.
    fn operational_registrations() -> Vec<(Method, &'static str)> {
        vec![
            (Method::PUT, AUTH_LOGIN_ROUTE),
            (Method::PUT, AUTH_SESSION_ROUTE),
            (Method::PUT, AUTH_LOGOUT_ROUTE),
            (Method::PUT, AUTH_MFA_VERIFY_ROUTE),
            (Method::PUT, AUTH_MFA_ENROLLMENT_ROUTE),
            (Method::PUT, AUTH_MFA_SELF_ENROLLMENT_ROUTE),
            (Method::PUT, AUTH_MFA_ENROLLMENT_CONFIRM_ROUTE),
        ]
    }

    /// Composes the Server-wide operational values a startup composes from.
    ///
    /// This is the same value `PreoperationalComposer` builds, so a test drives
    /// startup sealing and Restore sealing against one authority rather than
    /// two that could disagree.
    fn operational_runtime(
        startup: &RestrictedStartup,
        listener: SocketAddr,
    ) -> Arc<OperationalRuntime> {
        Arc::new(OperationalRuntime {
            listener,
            state_root: startup.state_root().to_path_buf(),
            log_catalog: Arc::clone(&startup.log_catalog),
            client_modules: server_components().client_modules,
            active_database: startup.active_database.clone(),
            protection: startup.protection(),
        })
    }

    /// Composes the operational surface a sealed startup hands over.
    fn operational_composer(
        startup: &RestrictedStartup,
        listener: SocketAddr,
    ) -> OperationalComposer {
        OperationalComposer::new(
            operational_runtime(startup, listener),
            startup
                .initialized_state()
                .expect("a sealed startup hands over its loaded state"),
            startup
                .application_database()
                .expect("a sealed startup hands over its open Application Database")
                .clone(),
        )
    }

    const SELECTED_STATUS: &str = "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}";
    const UNSELECTED_STATUS: &str = "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}";

    /// A restricted surface composed over a real lifecycle arbiter.
    ///
    /// Every route the surface mounts shares one `WorkflowArbiter`, so a
    /// committed selection is immediately observable through the status route.
    struct Surface {
        /// Retained so the state root outlives every request the surface serves.
        _root: tempfile::TempDir,
        state_root: PathBuf,
        startup: RestrictedStartup,
    }

    impl Surface {
        fn new(outcome: StartupOutcome) -> Self {
            let root = tempfile::tempdir().unwrap();
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let state_root = root.path().canonicalize().unwrap();
            if outcome == StartupOutcome::UninitializedWithDatabase {
                let mut store = LifecycleStore::open_or_create(&state_root).unwrap();
                store
                    .select_database(
                        &sqlite_catalog(),
                        &TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE)),
                        &BackendIdentifier::new("sqlite").unwrap(),
                        Vec::new(),
                    )
                    .unwrap();
            }
            // A sealed classification is produced by really sealing a
            // deployment, so the startup under test carries the loaded state
            // and the open Application Database a sealed startup hands over.
            if outcome == StartupOutcome::Initialized {
                seal_deployment(&state_root);
            }
            let mut startup = classify_restricted_startup(&state_root).unwrap();
            // A pending classification cannot be produced from a fresh state
            // root, and the gate under test is route mounting, not lifecycle
            // classification.
            startup.outcome = outcome;
            startup.composition.outcome = outcome;
            Self {
                _root: root,
                state_root,
                startup,
            }
        }

        fn routes(&self) -> Router {
            self.routes_for(UNBOUND_LISTENER.parse().unwrap())
        }

        fn routes_for(&self, listener: SocketAddr) -> Router {
            let (_switch, modes) = published_serving_modes(&self.startup, listener);
            modes.borrow().router().clone()
        }

        /// Snapshots the lifecycle record and every locator file by name, bytes,
        /// and modification time.
        fn anchor_snapshot(&self) -> Vec<(OsString, Vec<u8>, i64, i64)> {
            anchor_snapshot(&self.state_root)
        }
    }

    pub(crate) fn anchor_snapshot(state_root: &Path) -> Vec<(OsString, Vec<u8>, i64, i64)> {
        let mut entries = std::fs::read_dir(state_root)
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.unwrap();
                let name = entry.file_name();
                let name_text = name.to_string_lossy().into_owned();
                if name_text != "deployment-record.json"
                    && !name_text.starts_with("database-locator-")
                {
                    return None;
                }
                let metadata = entry.metadata().unwrap();
                Some((
                    name,
                    std::fs::read(entry.path()).unwrap(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                ))
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    /// Builds restricted routes over a real lifecycle arbiter.
    ///
    /// The temporary state root is removed before this returns. The arbiter
    /// keeps its open descriptors and the live status projection is read from
    /// in-memory lifecycle state, so every read-only route behaves exactly as it
    /// does in a running Server. A test that commits a selection must use
    /// [`Surface`] directly, which keeps its state root alive.
    fn restricted_routes(outcome: StartupOutcome) -> Router {
        Surface::new(outcome).routes()
    }

    /// A shutdown trigger that is never fired.
    ///
    /// A test about serving, rather than about stopping, composes the listener
    /// exactly as the process does but never asks it to stop.
    fn never_signalled() -> ShutdownSignal {
        ShutdownSignal::new(std::future::pending())
    }

    /// Serves the restricted listener over a surface composed for `outcome`.
    ///
    /// The surface is retained for the whole call because the listener is
    /// composed from the startup it holds.
    async fn serve_restricted_https_listener(
        tcp_listener: TcpListener,
        tls_config: Arc<ServerConfig>,
        outcome: StartupOutcome,
    ) -> Result<(), StartupError> {
        let surface = Surface::new(outcome);
        let serving = super::serve_restricted_https_listener(
            tcp_listener,
            tls_config,
            &surface.startup,
            never_signalled(),
        )?;
        let result = serving.await;
        drop(surface);
        result
    }

    pub(crate) async fn response_body(response: axum::response::Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[derive(Debug, PartialEq, Eq)]
    enum RequestHeadResult {
        Accepted,
        Invalid,
        MethodNotAllowed,
        TargetTooLong,
        HeadersTooLarge,
    }

    async fn read_request_head(bytes: &[u8]) -> RequestHeadResult {
        let (mut client, mut server) = tokio::io::duplex(bytes.len().max(1));
        client.write_all(bytes).await.unwrap();
        client.shutdown().await.unwrap();
        match read_default_profile_request(&mut server).await {
            Ok(_) => RequestHeadResult::Accepted,
            Err(error) => head_result(error),
        }
    }

    /// Reads a complete request and returns the body bytes it accepted.
    async fn read_request_with_body(bytes: &[u8]) -> Result<Vec<u8>, RequestHeadResult> {
        let (mut client, mut server) = tokio::io::duplex(bytes.len().max(1));
        client.write_all(bytes).await.unwrap();
        client.shutdown().await.unwrap();
        match read_default_profile_request(&mut server).await {
            Ok(request) => Ok(request
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec()),
            Err(error) => Err(head_result(error)),
        }
    }

    fn head_result(error: RequestReadError) -> RequestHeadResult {
        match error {
            RequestReadError::Invalid => RequestHeadResult::Invalid,
            RequestReadError::MethodNotAllowed => RequestHeadResult::MethodNotAllowed,
            RequestReadError::TargetTooLong => RequestHeadResult::TargetTooLong,
            RequestReadError::HeadersTooLarge => RequestHeadResult::HeadersTooLarge,
            RequestReadError::BodyUnavailable => {
                panic!("reader test must not exhaust host memory")
            }
            RequestReadError::TimedOut => panic!("reader test must not time out"),
        }
    }

    fn assert_fixed_tls_response(
        response: &[u8],
        status: u16,
        body: &str,
        allow: Option<AllowedMethod>,
    ) {
        let allow = allow.map_or("", AllowedMethod::allow_header);
        assert_eq!(
            response,
            format!(
                "HTTP/1.1 {status} \r\nContent-Type: application/json; charset=utf-8\r\n{allow}\r\n{body}"
            )
            .as_bytes()
        );
    }

    #[tokio::test]
    async fn uninitialized_status_routes_report_database_selection() {
        let without_database = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
            .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(without_database.status(), StatusCode::OK);
        assert_eq!(
            without_database.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            response_body(without_database).await,
            "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
        );

        let with_database = restricted_routes(StartupOutcome::UninitializedWithDatabase)
            .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(with_database.status(), StatusCode::OK);
        assert_eq!(
            response_body(with_database).await,
            "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}"
        );
    }

    #[tokio::test]
    async fn restricted_routes_reject_unsupported_requests_and_hide_unavailable_routes() {
        let method = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers().get("allow").unwrap(), "GET");
        assert_eq!(
            response_body(method).await,
            "{\"error\":\"method_not_allowed\"}"
        );

        let accept = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .header("accept", "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(accept.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body(accept).await, "{\"error\":\"bad_request\"}");

        let missing = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
            .oneshot(Request::get("/absent").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );
        assert_eq!(response_body(missing).await, "{\"error\":\"not_found\"}");

        let pending = restricted_routes(StartupOutcome::InitializationPending(
            weavelit_server_lifecycle::WorkflowKind::Init,
        ))
        .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
        assert_eq!(pending.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_body(pending).await, "{\"error\":\"not_found\"}");
    }

    fn generated_asset_bytes(relative: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../web-ui/dist")
                .join(relative),
        )
        .unwrap()
    }

    const EMBEDDED_ASSETS: [(&str, &str, &str); 3] = [
        ("/", "index.html", "text/html; charset=utf-8"),
        (
            "/assets/weavelit-application.js",
            "assets/weavelit-application.js",
            "text/javascript; charset=utf-8",
        ),
        (
            "/assets/weavelit-application.css",
            "assets/weavelit-application.css",
            "text/css; charset=utf-8",
        ),
    ];

    #[tokio::test]
    async fn uninitialized_routes_mount_the_web_ui_asset_allowlist() {
        for outcome in [
            StartupOutcome::UninitializedWithoutDatabase,
            StartupOutcome::UninitializedWithDatabase,
        ] {
            for (target, relative, media_type) in EMBEDDED_ASSETS {
                let response = restricted_routes(outcome)
                    .oneshot(Request::get(target).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK, "{target}");
                assert_eq!(
                    response.headers().get("content-type").unwrap(),
                    media_type,
                    "{target}"
                );
                let body = axum::body::to_bytes(response.into_body(), 256 * 1024)
                    .await
                    .unwrap();
                assert_eq!(body.as_ref(), generated_asset_bytes(relative), "{target}");
            }
        }
    }

    #[tokio::test]
    async fn asset_security_headers_match_the_module_and_the_wire_profile() {
        let response = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        for (name, wire_name) in [
            ("content-security-policy", "Content-Security-Policy"),
            ("x-content-type-options", "X-Content-Type-Options"),
            ("cache-control", "Cache-Control"),
        ] {
            let value = response.headers().get(name).unwrap().to_str().unwrap();
            assert!(
                ASSET_SECURITY_HEADERS.contains(&format!("{wire_name}: {value}\r\n")),
                "{name}"
            );
        }
        assert_eq!(ResponseProfile::Json.security_headers(), "");
    }

    #[tokio::test]
    async fn asset_routes_never_shadow_api_targets_or_unknown_targets() {
        let status = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
            .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(
            status.headers().get("content-type").unwrap(),
            "application/json; charset=utf-8"
        );
        assert!(!status.headers().contains_key("content-security-policy"));
        assert_eq!(
            response_body(status).await,
            "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
        );

        for target in [
            "/api/",
            "/api/v1/",
            "/api/v1/status/",
            "/api/v1/Status",
            "/api/v1/assets/weavelit-application.js",
            "/api/v1/unknown",
            "/index.html",
            "/assets/",
            "/assets/weavelit-application.js/",
            "/ASSETS/weavelit-application.js",
            "/assets/%77eavelit-application.js",
            "/assets/../assets/weavelit-application.js",
            "/../assets/weavelit-application.js",
            "/assets/weavelit-application.js.map",
        ] {
            let response = restricted_routes(StartupOutcome::UninitializedWithoutDatabase)
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response.headers().get("content-type").unwrap(),
                "application/json; charset=utf-8",
                "{target}"
            );
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    #[tokio::test]
    async fn asset_routes_are_absent_outside_the_uninitialized_gates() {
        for (target, _, _) in EMBEDDED_ASSETS {
            let response = restricted_routes(StartupOutcome::InitializationPending(
                weavelit_server_lifecycle::WorkflowKind::Init,
            ))
            .oneshot(Request::get(target).body(Body::empty()).unwrap())
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Tests: serving modes and sealed-deployment startup
    // -----------------------------------------------------------------------

    /// Builds the operational router exactly as the sealed serving mode does.
    fn operational_routes() -> Router {
        restricted_routes(StartupOutcome::Initialized)
    }

    #[tokio::test]
    async fn the_operational_surface_serves_the_web_ui_asset_allowlist() {
        for (target, relative, media_type) in EMBEDDED_ASSETS {
            let response = operational_routes()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{target}");
            assert_eq!(
                response.headers().get("content-type").unwrap(),
                media_type,
                "{target}"
            );
            let body = axum::body::to_bytes(response.into_body(), 256 * 1024)
                .await
                .unwrap();
            assert_eq!(body.as_ref(), generated_asset_bytes(relative), "{target}");
        }
    }

    #[tokio::test]
    async fn the_operational_surface_removes_every_preoperational_route() {
        for target in [STATUS_ROUTE, APPLICATION_DATABASE_ROUTE] {
            let response = operational_routes()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response.headers().get("content-type").unwrap(),
                "application/json; charset=utf-8",
                "{target}"
            );
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    #[tokio::test]
    async fn the_fail_closed_serving_mode_serves_only_not_found() {
        let router =
            ServingMode::FailClosed(MountedSurface::without_registrations(fallback_router()))
                .router()
                .clone();
        for target in [
            "/",
            "/assets/weavelit-application.js",
            "/assets/weavelit-application.css",
            STATUS_ROUTE,
            APPLICATION_DATABASE_ROUTE,
            "/unknown",
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    #[tokio::test]
    async fn publishing_a_serving_mode_changes_only_later_connection_snapshots() {
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let sealed = Surface::new(StartupOutcome::Initialized);
        let (switch, serving_modes) =
            published_serving_modes(&surface.startup, UNBOUND_LISTENER.parse().unwrap());

        // A connection accepted before the switch publishes any later mode.
        let in_flight = serving_modes.borrow().router().clone();

        switch.publish_operational(operational_mount(&sealed.startup));
        let after_operational = serving_modes.borrow().router().clone();

        // The already-snapshotted connection keeps its pre-operational surface.
        let status = in_flight
            .clone()
            .oneshot(Request::get(STATUS_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);

        // A newly accepted connection observes the published mode instead.
        let removed = after_operational
            .clone()
            .oneshot(Request::get(STATUS_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_body(removed).await, "{\"error\":\"not_found\"}");
        let asset = after_operational
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);

        switch.publish_fail_closed();
        let after_fail_closed = serving_modes.borrow().router().clone();
        let closed = after_fail_closed
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(closed.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_body(closed).await, "{\"error\":\"not_found\"}");

        // The first connection is still unaffected by either publication.
        let unchanged = in_flight
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unchanged.status(), StatusCode::OK);
    }

    /// Seals a deployment by driving the real lifecycle typestate chain.
    ///
    /// Returns the sealed deployment identifier and the committed application
    /// state. The arbiter is dropped before returning so the state-root lock is
    /// released for the startup path under test.
    fn seal_deployment(state_root: &Path) -> (DeploymentIdentifier, ApplicationState) {
        let state = sealed_application_state();
        let deployment_identifier = seal_deployment_with(state_root, &state);
        (deployment_identifier, state)
    }

    /// Seals a deployment over an explicitly supplied application state.
    ///
    /// Exposed so a test that needs accounts, password verifiers, or any other
    /// committed state seals a real deployment through the same typestate
    /// chain rather than assembling a second sealing path of its own.
    pub(crate) fn seal_deployment_with(
        state_root: &Path,
        state: &ApplicationState,
    ) -> DeploymentIdentifier {
        seal_deployment_from(state_root, |_sealer| state.clone())
    }

    /// Seals a deployment whose state is built against the deployment's own key.
    ///
    /// An enrolled factor carries protected data sealed under the anchor key,
    /// and that key exists only once the lifecycle store for this state root
    /// has been opened. The state is therefore built inside the workflow rather
    /// than handed in already built, so a test cannot produce a factor sealed
    /// against a key no deployment holds.
    pub(crate) fn seal_deployment_from(
        state_root: &Path,
        build: impl FnOnce(&dyn weavelit_server_lifecycle::ProtectedValueSealer) -> ApplicationState,
    ) -> DeploymentIdentifier {
        let mut store = LifecycleStore::open_or_create(state_root).unwrap();
        let catalog = sqlite_catalog();
        let context = TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE));
        store
            .select_database(
                &catalog,
                &context,
                &BackendIdentifier::new("sqlite").unwrap(),
                Vec::new(),
            )
            .unwrap();

        let arbiter = WorkflowArbiter::new(store);
        let permit = arbiter.authorize_workflow(&catalog, &context).unwrap();
        let deployment_identifier = permit.deployment_identifier();
        let state = build(permit.sealer());
        permit
            .create_checkpoint(
                WorkflowKind::Restore,
                CheckpointMetadata::from_bytes(b"restore-checkpoint-metadata".as_slice()).unwrap(),
            )
            .unwrap()
            .complete_checkpoint(&state)
            .unwrap()
            .acknowledge_completion(completion_record_identifier())
            .unwrap()
            .seal()
            .unwrap();
        drop(arbiter);
        deployment_identifier
    }

    fn completion_record_identifier() -> StateIdentifier {
        StateIdentifier::from_bytes([0x5A; 16]).unwrap()
    }

    /// Builds the smallest state the Application Database contract accepts.
    fn sealed_application_state() -> ApplicationState {
        sealed_application_state_with(Vec::new(), Vec::new())
    }

    /// Builds the smallest accepted state that also carries the supplied
    /// accounts and password verifiers.
    pub(crate) fn sealed_application_state_with(
        accounts: Vec<weavelit_server_database::Account>,
        password_verifiers: Vec<weavelit_server_database::AccountPasswordVerifier>,
    ) -> ApplicationState {
        sealed_application_state_from(SealedStateParts {
            accounts,
            password_verifiers,
            ..SealedStateParts::default()
        })
    }

    /// The parts of a sealed application state a test varies.
    ///
    /// Everything a sealed state must carry regardless of the test, which is
    /// the recovery key, the Log Module configuration, its assignments, and the
    /// completion obligation, is supplied by
    /// [`sealed_application_state_from`], so a test states only what it decides
    /// against.
    #[derive(Default)]
    pub(crate) struct SealedStateParts {
        pub(crate) configuration: Vec<weavelit_server_database::ConfigurationEntry>,
        pub(crate) accounts: Vec<weavelit_server_database::Account>,
        pub(crate) password_verifiers: Vec<weavelit_server_database::AccountPasswordVerifier>,
        pub(crate) groups: Vec<weavelit_server_database::Group>,
        pub(crate) group_memberships: Vec<weavelit_server_database::GroupMembership>,
        pub(crate) group_grants: Vec<weavelit_server_database::GroupGrantRecord>,
        pub(crate) mfa_factors: Vec<weavelit_server_database::MfaFactor>,
    }

    /// Builds the smallest accepted state that carries the supplied parts.
    pub(crate) fn sealed_application_state_from(parts: SealedStateParts) -> ApplicationState {
        let SealedStateParts {
            configuration,
            accounts,
            password_verifiers,
            groups,
            group_memberships,
            group_grants,
            mfa_factors,
        } = parts;
        let configuration_identifier = StateIdentifier::from_bytes([0x11; 16]).unwrap();
        ApplicationState::new(ApplicationStateInput {
            configuration,
            protected_secrets: vec![],
            accounts,
            password_verifiers,
            groups,
            group_memberships,
            group_grants,
            mfa_factors,
            service_connections: vec![],
            recovery_public_key: RecoveryPublicKey::new("age1recoverypublickeyvalue").unwrap(),
            log_module_configurations: vec![LogModuleConfiguration {
                identifier: configuration_identifier,
                module: Name::new("log-sqlite").unwrap(),
                name: Name::new("local").unwrap(),
                enabled: true,
                settings: vec![],
            }],
            log_assignments: LogType::ALL
                .into_iter()
                .map(|log_type| LogAssignment {
                    log_type,
                    configuration: configuration_identifier,
                })
                .collect(),
            completion_obligation: CompletionObligation::new(
                completion_record_identifier(),
                WorkflowKind::Restore,
                LogClassification::new("lifecycle.restore").unwrap(),
                CorrelationIdentifier::new("correlation-identifier").unwrap(),
                1_700_000_000_000,
                LogDetail::new("restore completed").unwrap(),
            )
            .unwrap(),
        })
        .unwrap()
    }

    #[test]
    fn a_sealed_deployment_starts_and_loads_its_application_state() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state_root = root.path().canonicalize().unwrap();
        let (deployment_identifier, state) = seal_deployment(&state_root);

        let startup = classify_restricted_startup(&state_root)
            .expect("a sealed deployment must reach normal operation");

        assert_eq!(startup.outcome(), StartupOutcome::Initialized);
        let loaded = startup
            .initialized_state()
            .expect("a sealed deployment must retain its loaded application state");
        assert_eq!(loaded.deployment_identifier(), deployment_identifier);
        assert!(loaded.completion_acknowledged());
        assert_eq!(loaded.state(), &state);
        assert_eq!(
            startup
                .application_database()
                .expect("a sealed deployment must hold its database open")
                .with(|database| database.inspect(deployment_identifier))
                .expect("the handed-over database lane must be usable")
                .unwrap(),
            DatabaseInspection::Initialized {
                deployment_identifier
            }
        );
    }

    #[test]
    fn a_sealed_startup_hands_its_open_application_database_to_the_operational_runtime() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state_root = root.path().canonicalize().unwrap();
        let (deployment_identifier, _state) = seal_deployment(&state_root);

        let startup = classify_restricted_startup(&state_root)
            .expect("a sealed deployment must reach normal operation");
        let composer = operational_composer(&startup, "127.0.0.1:8443".parse().unwrap());

        // The composer and the startup name one shared handle rather than two
        // opens of the same file, so the composer keeps serving the sealed
        // deployment after the startup value that opened it is gone.
        drop(startup);
        assert_eq!(
            composer
                .database()
                .with(|database| database.inspect(deployment_identifier))
                .expect("the handed-over database lane must be usable")
                .unwrap(),
            DatabaseInspection::Initialized {
                deployment_identifier
            }
        );
    }

    #[test]
    fn post_commit_reconciliation_still_fails_closed() {
        assert_eq!(
            startup_outcome(LifecycleClassification::PostCommitReconciliationRequired).unwrap_err(),
            StartupError::StateCombinationInvalid
        );
        assert_eq!(
            startup_outcome(LifecycleClassification::Initialized).unwrap(),
            StartupOutcome::Initialized
        );
    }

    #[test]
    fn request_header_bound_preserves_raw_header_bytes() {
        let bytes = format!(
            "GET / HTTP/1.1\r\nX-Bound: {}value\r\n\r\n",
            " ".repeat(8 * 1024)
        );
        assert!(raw_header_section_bytes(bytes.as_bytes()).unwrap() > 8 * 1024);
        assert!(matches!(
            parse_http_request(bytes.as_bytes(), super::RequestLineClassification::Get),
            Err(super::RequestReadError::HeadersTooLarge)
        ));
    }

    #[tokio::test]
    async fn request_head_reader_classification_matrix_covers_transitions_and_bounds() {
        let exact_target = format!("/{}", "a".repeat(super::MAX_REQUEST_TARGET_BYTES - 1));
        let exact_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len())
        );
        let oversized_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len() + 1)
        );
        let many_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\n{}\r\n",
            (0..65)
                .map(|index| format!("X-{index}: a\r\n"))
                .collect::<String>()
        );
        let valid_non_get_at_aggregate_limit = format!(
            "{} /api/v1/status HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "P".repeat(super::MAX_REQUEST_TARGET_BYTES + 128),
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len())
        );
        let malformed_line_at_aggregate_limit = format!(
            "GET /api/v1/status HTTP/{}\r\nX-Pad: {}\r\n\r\n",
            "1".repeat(super::MAX_REQUEST_TARGET_BYTES + 128),
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len())
        );

        assert_eq!(
            raw_header_section_bytes(exact_headers.as_bytes()).unwrap(),
            super::MAX_REQUEST_HEADER_BYTES
        );
        assert_eq!(
            raw_header_section_bytes(oversized_headers.as_bytes()).unwrap(),
            super::MAX_REQUEST_HEADER_BYTES + 1
        );
        assert!(many_headers.len() < super::MAX_REQUEST_HEAD_BYTES);
        assert!(valid_non_get_at_aggregate_limit.len() > super::MAX_REQUEST_HEAD_BYTES);
        assert!(malformed_line_at_aggregate_limit.len() > super::MAX_REQUEST_HEAD_BYTES);

        for (name, request, expected) in [
            (
                "method phase EOF",
                "GET".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "target phase EOF",
                "GET /".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "version phase EOF",
                "GET / HTTP/1.1".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "version ending EOF",
                "GET / HTTP/1.1\r".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "headers phase EOF",
                "GET / HTTP/1.1\r\nX-Test: a\r\n".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "strict CRLF GET",
                "GET /api/v1/status HTTP/1.1\r\nX-Test: a\r\n\r\n".to_owned(),
                RequestHeadResult::Accepted,
            ),
            (
                "LF-only framing",
                "GET /api/v1/status HTTP/1.1\n\n".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "valid non-GET method is left to the route",
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                RequestHeadResult::Accepted,
            ),
            (
                "invalid method",
                "GE(T /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "target exactly two KiB",
                format!("GET {exact_target} HTTP/1.1\r\n\r\n"),
                RequestHeadResult::Accepted,
            ),
            (
                "target over two KiB before malformed content",
                format!(
                    "GET /{} HTTP/1.1\r\nmalformed\r\n\r\n",
                    "a".repeat(super::MAX_REQUEST_TARGET_BYTES)
                ),
                RequestHeadResult::TargetTooLong,
            ),
            (
                "malformed HTTP version",
                "GET /api/v1/status HTTP/2\r\n\r\n".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "malformed header",
                "GET /api/v1/status HTTP/1.1\r\nnot-a-header\r\n\r\n".to_owned(),
                RequestHeadResult::Invalid,
            ),
            (
                "headers exactly eight KiB",
                exact_headers,
                RequestHeadResult::Accepted,
            ),
            (
                "headers over eight KiB",
                oversized_headers,
                RequestHeadResult::HeadersTooLarge,
            ),
            (
                "more than sixty-four headers within the byte limit",
                many_headers,
                RequestHeadResult::Accepted,
            ),
            (
                "valid non-GET method at aggregate limit",
                valid_non_get_at_aggregate_limit,
                RequestHeadResult::MethodNotAllowed,
            ),
            (
                "malformed request line at aggregate limit",
                malformed_line_at_aggregate_limit,
                RequestHeadResult::Invalid,
            ),
        ] {
            assert_eq!(
                read_request_head(request.as_bytes()).await,
                expected,
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn request_body_reader_bounds_put_bodies_and_rejects_other_framing() {
        let at_limit = "a".repeat(MAX_REQUEST_BODY_BYTES);
        let over_limit = "a".repeat(MAX_REQUEST_BODY_BYTES + 1);

        assert_eq!(
            read_request_with_body(
                format!(
                    "PUT /api/v1/database HTTP/1.1\r\nContent-Length: {MAX_REQUEST_BODY_BYTES}\r\n\r\n{at_limit}"
                )
                .as_bytes()
            )
            .await,
            Ok(at_limit.clone().into_bytes()),
            "PUT body at the one KiB limit"
        );
        assert_eq!(
            read_request_with_body(b"PUT /api/v1/database HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
                .await,
            Ok(Vec::new()),
            "PUT declaring an empty body"
        );

        for (name, request) in [
            (
                "body one byte over the limit",
                format!(
                    "PUT /api/v1/database HTTP/1.1\r\nContent-Length: {}\r\n\r\n{over_limit}",
                    MAX_REQUEST_BODY_BYTES + 1
                ),
            ),
            (
                "no Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\n\r\n".to_owned(),
            ),
            (
                "duplicate Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: 4\r\nContent-Length: 4\r\n\r\nbody"
                    .to_owned(),
            ),
            (
                "conflicting Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: 4\r\nContent-Length: 5\r\n\r\nbody"
                    .to_owned(),
            ),
            (
                "non-numeric Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: four\r\n\r\nbody".to_owned(),
            ),
            (
                "negative Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: -4\r\n\r\nbody".to_owned(),
            ),
            (
                "plus-prefixed Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: +4\r\n\r\nbody".to_owned(),
            ),
            (
                "non-canonical Content-Length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: 04\r\n\r\nbody".to_owned(),
            ),
            (
                "chunked transfer encoding",
                "PUT /api/v1/database HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
                    .to_owned(),
            ),
            (
                "expect continue",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: 4\r\nExpect: 100-continue\r\n\r\nbody"
                    .to_owned(),
            ),
            (
                "stream ends before the declared length",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: 8\r\n\r\nbody".to_owned(),
            ),
            (
                "more bytes than declared",
                "PUT /api/v1/database HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody-extra".to_owned(),
            ),
            (
                "GET carrying a declared body",
                "GET /api/v1/status HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody".to_owned(),
            ),
            (
                "GET carrying chunked framing",
                "GET /api/v1/status HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
                    .to_owned(),
            ),
        ] {
            assert_eq!(
                read_request_with_body(request.as_bytes()).await,
                Err(RequestHeadResult::Invalid),
                "{name}"
            );
        }
    }

    /// The head arrives at 400 ms and the body at 700 ms. A 500 ms budget shared
    /// by both expires, while a budget restarted at the body would not. This
    /// drives the production request path, so it also proves the default
    /// profile still shares one budget after admission was introduced.
    #[tokio::test]
    async fn request_read_timeout_covers_the_head_and_body_as_one_budget() {
        let surface = MountedSurface::without_registrations(restricted_routes(
            StartupOutcome::UninitializedWithDatabase,
        ));
        let schedule = |budget: Duration| {
            let surface = surface.clone();
            async move {
                let (mut client, mut server) = tokio::io::duplex(256);
                let writer = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    client
                        .write_all(b"PUT /api/v1/database HTTP/1.1\r\nContent-Length: 4\r\n\r\n")
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    client.write_all(b"body").await.unwrap();
                    std::future::pending::<()>().await;
                });
                let outcome = super::process_restricted_request(
                    &mut server,
                    "127.0.0.1".parse().unwrap(),
                    surface,
                    Arc::new(RateLimiter::new()),
                    ConnectionTimeouts {
                        handshake: TLS_HANDSHAKE_TIMEOUT,
                        request_read: budget,
                        processing: REQUEST_PROCESSING_TIMEOUT,
                    },
                )
                .await;
                writer.abort();
                outcome
            }
        };

        let expired = schedule(Duration::from_millis(500)).await;
        assert_eq!(expired.status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(expired.body, request_timeout_response().body);

        // The same schedule inside a budget that covers both stages is read in
        // full and reaches the router, which answers not-found for a target the
        // surface never mounted.
        let accepted = schedule(Duration::from_millis(1_500)).await;
        assert_eq!(accepted.status, StatusCode::NOT_FOUND);
        assert_eq!(accepted.body.as_ref(), b"{\"error\":\"not_found\"}");
    }

    #[tokio::test]
    async fn direct_tls_emits_the_allow_header_the_route_selected() {
        let (server_config, client_config) = tls_configs();

        let put_only_route = direct_tls_response(
            Router::new().route(
                "/api/v1/database",
                any(|| async { DatabaseSelectionRejection::MethodNotAllowed.response() }),
            ),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"GET /api/v1/database HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(
            &put_only_route,
            405,
            "{\"error\":\"method_not_allowed\"}",
            Some(AllowedMethod::Put),
        );

        let get_only_route = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"PUT /api/v1/status HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(
            &get_only_route,
            405,
            "{\"error\":\"method_not_allowed\"}",
            Some(AllowedMethod::Get),
        );

        // An oversized method token never yields a bounded target, so this early
        // fallback answers without any route context.
        let early_fallback = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            server_config,
            client_config,
            format!(
                "{} /api/v1/status HTTP/1.1\r\n\r\n",
                "P".repeat(super::MAX_REQUEST_HEAD_BYTES + 1)
            )
            .as_bytes(),
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(
            &early_fallback,
            405,
            "{\"error\":\"method_not_allowed\"}",
            Some(AllowedMethod::Get),
        );
    }

    #[tokio::test]
    async fn head_request_returns_correct_status_with_empty_body() {
        use axum::routing::get;

        let (server_config, client_config) = tls_configs();

        // A route defined with axum::routing::get handles HEAD automatically:
        // the handler runs, the body is stripped by Axum, and our fix must not
        // re-inject gateway_timeout or any other body.
        let router = Router::new().route(
            "/api/v1/status",
            get(|| async {
                super::json_response_body(
                    StatusCode::OK,
                    "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                )
            }),
        );
        let response = direct_tls_response(
            router,
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"HEAD /api/v1/status HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        // Status and headers must be present; body must be absent per RFC 9110 §9.3.2.
        assert_eq!(
            response,
            b"HTTP/1.1 200 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n"
        );

        // Confirm the fix also suppresses the body on a HEAD 405 (method rejected by the
        // mounted route before Axum can auto-handle it).
        let response_405 = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            server_config,
            client_config,
            b"HEAD /api/v1/status HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_eq!(
            response_405,
            b"HTTP/1.1 405 \r\nContent-Type: application/json; charset=utf-8\r\nAllow: GET\r\n\r\n"
        );
    }

    #[test]
    fn connection_slots_reserve_one_lane_for_rejection() {
        let slots = ConnectionSlots::new();
        let normal_permits = (0..15)
            .map(|_| slots.normal.clone().try_acquire_owned().unwrap())
            .collect::<Vec<_>>();
        let rejection_permit = slots.rejection.clone().try_acquire_owned().unwrap();

        assert!(slots.normal.clone().try_acquire_owned().is_err());
        assert!(slots.rejection.clone().try_acquire_owned().is_err());

        drop(rejection_permit);
        drop(normal_permits);
    }

    fn tls_configs() -> (Arc<ServerConfig>, Arc<ClientConfig>) {
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate_der = CertificateDer::from(certificate.cert.der().to_vec());
        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certificate.signing_key.serialize_der(),
        ));
        let provider = Arc::new(aws_lc_rs::default_provider());
        let server = ServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![certificate_der.clone()], private_key)
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(certificate_der).unwrap();
        let client = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        (Arc::new(server), Arc::new(client))
    }

    async fn tls_client(
        address: std::net::SocketAddr,
        config: Arc<ClientConfig>,
    ) -> tokio_rustls::client::TlsStream<TcpStream> {
        TlsConnector::from(config)
            .connect(
                ServerName::try_from("localhost").unwrap().to_owned(),
                TcpStream::connect(address).await.unwrap(),
            )
            .await
            .unwrap()
    }

    async fn direct_tls_response(
        router: Router,
        server_config: Arc<ServerConfig>,
        client_config: Arc<ClientConfig>,
        request: &[u8],
        request_read_timeout: Duration,
        processing_timeout: Duration,
    ) -> Vec<u8> {
        direct_tls_response_with_rate_limiter(
            router,
            server_config,
            client_config,
            Arc::new(RateLimiter::new()),
            request,
            request_read_timeout,
            processing_timeout,
        )
        .await
    }

    async fn direct_tls_response_with_rate_limiter(
        router: Router,
        server_config: Arc<ServerConfig>,
        client_config: Arc<ClientConfig>,
        rate_limiter: Arc<RateLimiter>,
        request: &[u8],
        request_read_timeout: Duration,
        processing_timeout: Duration,
    ) -> Vec<u8> {
        direct_tls_surface_response(
            MountedSurface::without_registrations(router),
            server_config,
            client_config,
            rate_limiter,
            request,
            request_read_timeout,
            processing_timeout,
        )
        .await
    }

    /// Serves one direct-TLS request against a surface that may carry its own
    /// transport registrations.
    async fn direct_tls_surface_response(
        surface: MountedSurface,
        server_config: Arc<ServerConfig>,
        client_config: Arc<ClientConfig>,
        rate_limiter: Arc<RateLimiter>,
        request: &[u8],
        request_read_timeout: Duration,
        processing_timeout: Duration,
    ) -> Vec<u8> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, source) = listener.accept().await.unwrap();
            serve_normal_connection_with_timeouts(
                stream,
                source.ip(),
                TlsAcceptor::from(server_config),
                surface,
                rate_limiter,
                ConnectionTimeouts {
                    handshake: TLS_HANDSHAKE_TIMEOUT,
                    request_read: request_read_timeout,
                    processing: processing_timeout,
                },
            )
            .await;
        });
        let mut client = tls_client(address, client_config).await;
        if !request.is_empty() {
            client.write_all(request).await.unwrap();
        }
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("direct TLS response must complete with close_notify");
        server.await.unwrap();
        response
    }

    async fn direct_tls_response_after_eof(
        router: Router,
        server_config: Arc<ServerConfig>,
        client_config: Arc<ClientConfig>,
        rate_limiter: Arc<RateLimiter>,
        request: &[u8],
        request_read_timeout: Duration,
        processing_timeout: Duration,
    ) -> Vec<u8> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, source) = listener.accept().await.unwrap();
            serve_normal_connection_with_timeouts(
                stream,
                source.ip(),
                TlsAcceptor::from(server_config),
                MountedSurface::without_registrations(router),
                rate_limiter,
                ConnectionTimeouts {
                    handshake: TLS_HANDSHAKE_TIMEOUT,
                    request_read: request_read_timeout,
                    processing: processing_timeout,
                },
            )
            .await;
        });
        let mut client = tls_client(address, client_config).await;
        client.write_all(request).await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("direct TLS response must complete with close_notify");
        server.await.unwrap();
        response
    }

    async fn direct_tls_rejection_response(
        server_config: Arc<ServerConfig>,
        client_config: Arc<ClientConfig>,
    ) -> Vec<u8> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_rejection_connection_with_timeouts(
                stream,
                TlsAcceptor::from(server_config),
                TLS_HANDSHAKE_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
        });
        let mut client = tls_client(address, client_config).await;
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("direct TLS rejection response must complete with close_notify");
        server.await.unwrap();
        response
    }

    // -----------------------------------------------------------------------
    // Tests: signalled shutdown
    // -----------------------------------------------------------------------

    /// The route a gated shutdown test serves, and the body it answers with.
    const GATED_ROUTE: &str = "/api/v1/gated";
    const GATED_BODY: &str = "<!doctype html><title>gated</title>";

    /// A listener running the real accept-and-drain loop over a bound socket.
    struct DrainingListener {
        address: SocketAddr,
        client_config: Arc<ClientConfig>,
        /// Completing this trigger is what asks the loop to stop.
        shutdown: oneshot::Sender<()>,
        /// The gate the loop closes on the signal and waits on beside the
        /// drain; a test enters it to stand in for a lifecycle transition.
        transition_gate: Arc<LifecycleTransitionGate>,
        serving: JoinHandle<Result<(), StartupError>>,
    }

    /// Serves `router` through the production accept-and-drain loop.
    ///
    /// The shutdown trigger is injected rather than raised, so a test drives
    /// exactly the path a real signal drives without sending one.
    async fn draining_listener(
        router: Router,
        budget: ShutdownBudget,
        active_database: ActiveDatabase,
    ) -> DrainingListener {
        draining_listener_with_transition_gate(
            router,
            budget,
            active_database,
            Arc::new(LifecycleTransitionGate::new()),
        )
        .await
    }

    /// Serves `router` through the production loop using an existing lifecycle
    /// gate, so an orchestration test can share the listener's real gate.
    async fn draining_listener_with_transition_gate(
        router: Router,
        budget: ShutdownBudget,
        active_database: ActiveDatabase,
        transition_gate: Arc<LifecycleTransitionGate>,
    ) -> DrainingListener {
        let (server_config, client_config) = tls_configs();
        let tcp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = tcp_listener.local_addr().unwrap();
        let (_switch, serving_modes) = ServingModeSwitch::new(ServingMode::FailClosed(
            MountedSurface::without_registrations(router),
        ));
        let (shutdown, signalled) = oneshot::channel();
        let serving = tokio::spawn(accept_and_drain_connections(
            tcp_listener,
            TlsAcceptor::from(server_config),
            serving_modes,
            ShutdownSignal::new(async move {
                let _ = signalled.await;
            }),
            budget,
            Arc::clone(&transition_gate),
            active_database,
        ));

        DrainingListener {
            address,
            client_config,
            shutdown,
            transition_gate,
            serving,
        }
    }

    /// A route that reports when it is serving and returns only when released.
    ///
    /// A test learns the request is in flight from the report rather than from
    /// any elapsed duration. The release is a stored notification, so releasing
    /// before the handler parks still releases it.
    fn gated_route() -> (Router, mpsc::UnboundedReceiver<()>, Arc<Notify>) {
        let (entered, serving) = mpsc::unbounded_channel();
        let release = Arc::new(Notify::new());
        let handler_release = Arc::clone(&release);
        let router = Router::new().route(
            GATED_ROUTE,
            any(move || {
                let entered = entered.clone();
                let release = Arc::clone(&handler_release);
                async move {
                    let _ = entered.send(());
                    release.notified().await;
                    Html(GATED_BODY)
                }
            }),
        );

        (router, serving, release)
    }

    fn gated_request() -> Vec<u8> {
        format!("GET {GATED_ROUTE} HTTP/1.1\r\n\r\n").into_bytes()
    }

    /// The exact bytes a released gated request must put on the wire.
    fn gated_response() -> Vec<u8> {
        format!(
            "HTTP/1.1 200 \r\nContent-Type: text/html; charset=utf-8\r\n{ASSET_SECURITY_HEADERS}\r\n{GATED_BODY}"
        )
        .into_bytes()
    }

    /// Waits until the listener has actually given up its bound socket.
    ///
    /// Rebinding the same address is the observation, and it succeeds only once
    /// the listening socket is closed. It opens no connection the listener
    /// could still accept, and no elapsed duration decides the outcome.
    async fn rebind_released_port(address: SocketAddr) -> TcpListener {
        loop {
            if let Ok(rebound) = TcpListener::bind(address).await {
                return rebound;
            }
            tokio::task::yield_now().await;
        }
    }

    /// Asserts a closed SQLite Application Database left no recovery work.
    pub(crate) fn assert_no_write_ahead_log(state_root: &Path) {
        for sidecar in ["application.sqlite3-wal", "application.sqlite3-shm"] {
            assert!(!state_root.join(sidecar).exists(), "{sidecar}");
        }
    }

    #[tokio::test]
    async fn a_signalled_shutdown_stops_accepting_and_releases_the_bound_port() {
        let DrainingListener {
            address,
            shutdown,
            serving,
            ..
        } = draining_listener(
            fallback_router(),
            ShutdownBudget::DEFAULT,
            ActiveDatabase::default(),
        )
        .await;

        shutdown.send(()).unwrap();
        assert_eq!(serving.await.unwrap(), Ok(()));

        // A connection attempted after the signal reaches no listener at all,
        // and the address is free for the next Server generation.
        assert!(TcpStream::connect(address).await.is_err());
        drop(TcpListener::bind(address).await.unwrap());
    }

    #[tokio::test]
    async fn an_in_flight_request_completes_its_response_write_after_the_signal() {
        let (router, mut serving_route, release) = gated_route();
        let DrainingListener {
            address,
            client_config,
            shutdown,
            serving,
            ..
        } = draining_listener(router, ShutdownBudget::DEFAULT, ActiveDatabase::default()).await;

        let mut client = tls_client(address, client_config).await;
        client.write_all(&gated_request()).await.unwrap();
        // The route reports itself, so the request is provably in flight.
        serving_route
            .recv()
            .await
            .expect("the gated route must be serving");

        shutdown.send(()).unwrap();
        // Rebinding proves the loop already stopped accepting, so the release
        // below cannot be what let the request finish before the signal.
        let _rebound = rebind_released_port(address).await;
        release.notify_one();

        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("the in-flight response must complete with close_notify");
        assert_eq!(response, gated_response());
        assert_eq!(serving.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn a_shutdown_signalled_as_a_connection_arrives_stops_instead_of_serving() {
        let (server_config, _client_config) = tls_configs();
        let tcp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = tcp_listener.local_addr().unwrap();
        let (_switch, serving_modes) = ServingModeSwitch::new(ServingMode::FailClosed(
            MountedSurface::without_registrations(fallback_router()),
        ));
        // Completed into the accept queue before the loop is ever polled, so
        // accepting is ready at the same first poll the shutdown is.
        let mut client = TcpStream::connect(address).await.unwrap();

        let result = accept_and_drain_connections(
            tcp_listener,
            TlsAcceptor::from(server_config),
            serving_modes,
            ShutdownSignal::new(std::future::ready(())),
            ShutdownBudget::DEFAULT,
            Arc::new(LifecycleTransitionGate::new()),
            ActiveDatabase::default(),
        )
        .await;

        assert_eq!(result, Ok(()));
        // The queued connection was never accepted, so it was never served.
        let mut byte = [0_u8; 1];
        assert!(!matches!(client.read(&mut byte).await, Ok(1)));
    }

    #[tokio::test]
    async fn a_drain_that_cannot_finish_reports_an_incomplete_shutdown() {
        let (router, mut serving_route, _release) = gated_route();
        let DrainingListener {
            address,
            client_config,
            shutdown,
            serving,
            ..
        } = draining_listener(
            router,
            // The gated request is never released, so no drain budget could
            // ever be long enough; this value only keeps the test short.
            ShutdownBudget {
                drain: Duration::ZERO,
                ..ShutdownBudget::DEFAULT
            },
            ActiveDatabase::default(),
        )
        .await;

        let mut client = tls_client(address, client_config).await;
        client.write_all(&gated_request()).await.unwrap();
        serving_route
            .recv()
            .await
            .expect("the gated route must be serving");

        shutdown.send(()).unwrap();

        assert_eq!(
            serving.await.unwrap(),
            Err(StartupError::ShutdownIncomplete)
        );
    }

    #[tokio::test]
    async fn the_application_database_is_closed_only_after_the_drain_completes() {
        let (router, mut serving_route, release) = gated_route();
        let (active_database, _database, closes) = test_support::activated();
        let DrainingListener {
            address,
            client_config,
            shutdown,
            serving,
            ..
        } = draining_listener(router, ShutdownBudget::DEFAULT, active_database).await;

        let mut client = tls_client(address, client_config).await;
        client.write_all(&gated_request()).await.unwrap();
        serving_route
            .recv()
            .await
            .expect("the gated route must be serving");
        shutdown.send(()).unwrap();
        let _rebound = rebind_released_port(address).await;

        // The drain cannot have finished while the route is still gated, so
        // the close that follows the drain cannot have happened yet either.
        assert_eq!(closes.load(Ordering::SeqCst), 0);

        release.notify_one();
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("the in-flight response must complete with close_notify");

        assert_eq!(response, gated_response());
        assert_eq!(serving.await.unwrap(), Ok(()));
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failing_database_close_reports_an_incomplete_shutdown() {
        let (active_database, _database, closes) =
            test_support::activated_closing(Err(DatabaseError::Unavailable));
        let DrainingListener {
            shutdown, serving, ..
        } = draining_listener(fallback_router(), ShutdownBudget::DEFAULT, active_database).await;

        shutdown.send(()).unwrap();

        assert_eq!(
            serving.await.unwrap(),
            Err(StartupError::ShutdownIncomplete)
        );
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    /// The gate refuses entry from the moment a stop is observed and keeps
    /// refusing it, so nothing begins an irreversible transition a shutdown
    /// would then have to wait out.
    ///
    /// A refusal leaves the permit free, which is what lets a shutdown that is
    /// already waiting proceed instead of blocking behind an entrant that was
    /// turned away between taking the permit and reading the flag again.
    #[test]
    fn a_closed_transition_gate_refuses_entry_permanently() {
        let gate = LifecycleTransitionGate::new();

        let entered = gate
            .try_enter()
            .expect("an open gate must admit a transition");
        assert!(gate.is_occupied());
        drop(entered);
        assert!(!gate.is_occupied());

        gate.begin_stopping();
        assert!(gate.try_enter().is_none());
        assert!(!gate.is_occupied());
        assert!(gate.try_enter().is_none());
    }

    /// A zero reporting threshold records an overrun without making the gate
    /// quiescent while an admitted transition still owns its permit.
    ///
    /// Polling once is deterministic: the old timeout-based implementation
    /// completed immediately with no guard, whereas the required behavior is
    /// pending until the transition releases the permit.
    #[tokio::test]
    async fn a_zero_threshold_quiescence_waits_for_a_held_transition_gate() {
        let gate = LifecycleTransitionGate::new();
        let transition = gate
            .try_enter()
            .expect("an open gate must admit a transition");
        gate.begin_stopping();

        let mut quiescence = Box::pin(gate.quiesce(Duration::ZERO));
        let waker = std::task::Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(
            matches!(
                std::future::Future::poll(quiescence.as_mut(), &mut context),
                Poll::Pending
            ),
            "a transition overrun must not make shutdown quiescent"
        );

        drop(transition);
        let _quiescence = quiescence.await;
    }

    /// A stop that arrives while an irreversible lifecycle transition is inside
    /// its region waits for that region, instead of exiting the process between
    /// the transition's fail-closed publication and its committed record.
    ///
    /// The drain budget is zero and no connection is open, so nothing but the
    /// transition gate could hold this shutdown open. Completion is unreachable
    /// while the guard is held, so the assertion below is a property of the
    /// gate rather than an observation of elapsed time.
    #[tokio::test]
    async fn a_signalled_shutdown_waits_for_a_lifecycle_transition_to_leave_the_gate() {
        let (active_database, _database, closes) = test_support::activated();
        let DrainingListener {
            address,
            shutdown,
            transition_gate,
            serving,
            ..
        } = draining_listener(
            fallback_router(),
            ShutdownBudget {
                drain: Duration::ZERO,
                ..ShutdownBudget::DEFAULT
            },
            active_database,
        )
        .await;

        // Stands in for the Restore or Init region: both production workflows
        // enter this same gate through this same call.
        let transition = transition_gate
            .try_enter()
            .expect("an open gate must admit a transition");

        shutdown.send(()).unwrap();
        // Rebinding proves the loop already left its accept phase, so what
        // remains is the drain, the wait, and the close.
        let _rebound = rebind_released_port(address).await;

        assert!(!serving.is_finished());
        assert_eq!(closes.load(Ordering::SeqCst), 0);

        drop(transition);
        assert_eq!(serving.await.unwrap(), Ok(()));
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    /// A transition that outlasts the reporting threshold still holds the stop
    /// until it releases the gate. The shutdown then reports the overrun as
    /// unclean only after closing the registered database.
    #[tokio::test]
    async fn a_lifecycle_transition_that_outlasts_its_threshold_reports_an_incomplete_shutdown() {
        let (active_database, _database, closes) = test_support::activated();
        let DrainingListener {
            address,
            shutdown,
            transition_gate,
            serving,
            ..
        } = draining_listener(
            fallback_router(),
            // The guard below is not released until after the overrun is
            // observed, so a zero threshold makes the case deterministic.
            ShutdownBudget {
                transition_threshold: Duration::ZERO,
                ..ShutdownBudget::DEFAULT
            },
            active_database,
        )
        .await;
        let transition = transition_gate
            .try_enter()
            .expect("an open gate must admit a transition");

        shutdown.send(()).unwrap();

        // Rebinding proves accepting has stopped; with the transition still
        // held, only its release can let the listener reach database close.
        let _rebound = rebind_released_port(address).await;
        assert!(!serving.is_finished());
        assert_eq!(closes.load(Ordering::SeqCst), 0);

        drop(transition);
        assert_eq!(
            serving.await.unwrap(),
            Err(StartupError::ShutdownIncomplete)
        );
        // The overdue transition is reported, but the close is still exactly
        // once and happens only after the gate was released.
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        // The gate was closed on the signal and stays closed, so a transition
        // that had not entered can never enter afterwards.
        assert!(transition_gate.try_enter().is_none());
    }

    /// A deployment becomes operational from a sealed startup or from an
    /// in-process Restore, so the close is proved against a real SQLite backend
    /// on both paths rather than on whichever one a test happened to take.
    #[tokio::test]
    async fn a_sealed_startups_close_leaves_no_write_ahead_log_and_restarts() {
        let Surface {
            _root,
            state_root,
            startup,
        } = Surface::new(StartupOutcome::Initialized);
        // Composing the operational surface is what registers the open
        // database with the owner a shutdown closes through.
        let composer = operational_composer(&startup, UNBOUND_LISTENER.parse().unwrap());
        assert!(state_root.join("application.sqlite3-wal").exists());

        assert!(
            close_active_database(startup.active_database().clone(), SHUTDOWN_DATABASE_BUDGET)
                .await
        );

        assert_no_write_ahead_log(&state_root);
        drop(composer);
        // Releases the process-lifetime state-root lock the next start needs.
        drop(startup);
        assert_eq!(
            classify_restricted_startup(&state_root).unwrap().outcome(),
            StartupOutcome::Initialized
        );
    }

    #[tokio::test]
    async fn a_restored_deployments_close_leaves_no_write_ahead_log_and_restarts() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect("the committed valid backup must restore");
        assert!(surface.state_root.join("application.sqlite3-wal").exists());

        assert!(
            close_active_database(
                surface.startup.active_database().clone(),
                SHUTDOWN_DATABASE_BUDGET,
            )
            .await
        );

        assert_no_write_ahead_log(&surface.state_root);
        drop(orchestrator);
        let (_root, state_root) = surface.release();
        assert_eq!(
            classify_restricted_startup(&state_root).unwrap().outcome(),
            StartupOutcome::Initialized
        );
    }

    #[tokio::test]
    async fn direct_tls_request_head_matrix_preserves_public_contract() {
        let (server_config, client_config) = tls_configs();
        let many_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\n{}\r\n",
            (0..65)
                .map(|index| format!("X-{index}: a\r\n"))
                .collect::<String>()
        );
        let oversized_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len() + 1)
        );

        for (name, request, status, body, allow) in [
            (
                "valid GET",
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                None,
            ),
            (
                "valid non-GET",
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                405,
                "{\"error\":\"method_not_allowed\"}",
                Some(AllowedMethod::Get),
            ),
            (
                "invalid method",
                "GE(T /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                400,
                "{\"error\":\"bad_request\"}",
                None,
            ),
            (
                "target over two KiB before malformed content",
                format!(
                    "GET /{} HTTP/1.1\r\nmalformed\r\n\r\n",
                    "a".repeat(super::MAX_REQUEST_TARGET_BYTES)
                ),
                414,
                "{\"error\":\"uri_too_long\"}",
                None,
            ),
            (
                "headers over eight KiB",
                oversized_headers,
                431,
                "{\"error\":\"request_header_fields_too_large\"}",
                None,
            ),
            (
                "more than sixty-four headers",
                many_headers,
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                None,
            ),
        ] {
            let response = direct_tls_response(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                request.as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert_fixed_tls_response(&response, status, body, allow);
            assert!(response.len() <= 128, "{name}");
        }
    }

    #[tokio::test]
    async fn direct_tls_responses_complete_with_close_notify() {
        let (server_config, client_config) = tls_configs();

        let normal = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"GET /api/v1/status HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert!(normal.starts_with(b"HTTP/1.1 200 \r\n"));

        let rejection = direct_tls_rejection_response(server_config, client_config).await;
        assert!(rejection.starts_with(b"HTTP/1.1 503 \r\n"));
    }

    #[tokio::test]
    async fn direct_tls_capacity_reserves_rejection_lane_and_releases_normal_slots() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (server_config, client_config) = tls_configs();
        let server = tokio::spawn(serve_restricted_https_listener(
            listener,
            server_config,
            StartupOutcome::UninitializedWithoutDatabase,
        ));

        let mut normal_connections = Vec::new();
        for _ in 0..15 {
            normal_connections.push(tls_client(address, Arc::clone(&client_config)).await);
        }

        let mut overflow = tls_client(address, Arc::clone(&client_config)).await;
        let mut response = Vec::new();
        let _ = overflow.read_to_end(&mut response).await;
        assert_eq!(
            response,
            b"HTTP/1.1 503 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n{\"error\":\"service_unavailable\"}"
        );

        let busy_rejection = TcpStream::connect(address).await.unwrap();
        let mut further_overflow = TcpStream::connect(address).await.unwrap();
        let mut bytes = [0_u8; 1];
        match tokio::time::timeout(Duration::from_secs(1), further_overflow.read(&mut bytes)).await
        {
            Ok(Ok(0)) | Ok(Err(_)) => {}
            outcome => panic!("further overflow must be transport-rejected, got {outcome:?}"),
        }
        drop(busy_rejection);

        drop(normal_connections.pop());
        let mut released = tls_client(address, client_config).await;
        released
            .write_all(b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let _ = released.read_to_end(&mut response).await;
        assert!(response.starts_with(b"HTTP/1.1 200 \r\n"));

        server.abort();
    }

    #[tokio::test]
    async fn direct_tls_admits_only_exact_loopback_peers() {
        let (server_config, client_config) = tls_configs();

        for listener_address in ["127.0.0.1:0", "[::1]:0"] {
            let listener = TcpListener::bind(listener_address).await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(serve_restricted_https_listener(
                listener,
                Arc::clone(&server_config),
                StartupOutcome::UninitializedWithoutDatabase,
            ));

            let mut client = tls_client(address, Arc::clone(&client_config)).await;
            client
                .write_all(b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            let _ = client.read_to_end(&mut response).await;
            assert!(response.starts_with(b"HTTP/1.1 200 \r\n"));

            server.abort();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve_restricted_https_listener(
            listener,
            server_config,
            StartupOutcome::UninitializedWithoutDatabase,
        ));
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.2:0".parse().unwrap()).unwrap();
        let stream = socket.connect(address).await.unwrap();
        let rejection = tokio::time::timeout(
            Duration::from_secs(1),
            TlsConnector::from(client_config).connect(
                ServerName::try_from("localhost").unwrap().to_owned(),
                stream,
            ),
        )
        .await;
        assert!(matches!(rejection, Ok(Err(_))));

        server.abort();
    }

    #[tokio::test]
    async fn rejection_lane_timeout_releases_its_permit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (server_config, _client_config) = tls_configs();
        let slots = ConnectionSlots::new();
        let permit = slots.rejection.clone().try_acquire_owned().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_rejection_connection_with_timeouts(
                stream,
                TlsAcceptor::from(server_config),
                Duration::ZERO,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            drop(permit);
        });

        let _client = TcpStream::connect(address).await.unwrap();
        server.await.unwrap();
        assert!(slots.rejection.try_acquire_owned().is_ok());
    }

    #[tokio::test]
    async fn direct_tls_rejects_parser_contract_violations() {
        let (server_config, client_config) = tls_configs();

        for (request, status, body) in [
            (
                format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(11 * 1024)),
                b"HTTP/1.1 414 \r\n".as_slice(),
                b"{\"error\":\"uri_too_long\"}".as_slice(),
            ),
            (
                format!(
                    "GET https://{}/api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n",
                    "a".repeat(super::MAX_REQUEST_TARGET_BYTES),
                ),
                b"HTTP/1.1 414 \r\n".as_slice(),
                b"{\"error\":\"uri_too_long\"}".as_slice(),
            ),
            (
                format!("GET /api/v1/status HTTP/1.1\r\nX-Bound: {}value\r\n\r\n", " ".repeat(8 * 1024)),
                b"HTTP/1.1 431 \r\n".as_slice(),
                b"{\"error\":\"request_header_fields_too_large\"}".as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nAccept: application/json\r\nAccept: text/html\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 1\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nContent-Length: 00\r\n\r\n".to_owned(),
                b"HTTP/1.1 200 \r\n".as_slice(),
                b"{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
                    .as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 00\r\n\r\n".to_owned(),
                b"HTTP/1.1 200 \r\n".as_slice(),
                b"{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
                    .as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nContent-Length: 1\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nContent-Length: 00x\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                "GET /api/v1/status HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                format!(
                    "PUT /api/v1/status HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                    super::MAX_REQUEST_BODY_BYTES + 1
                ),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                "PUT /api/v1/status HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
            (
                "PUT /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
            ),
        ] {
            let response = direct_tls_response(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                request.as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert!(
                response.starts_with(status),
                "request {request:?} produced response {response:?}"
            );
            assert!(
                response.ends_with(body),
                "request {request:?} produced response {response:?}"
            );
        }

        let exact_target = format!(
            "https://{}/api/v1/status",
            "a".repeat(super::MAX_REQUEST_TARGET_BYTES - "https:///api/v1/status".len()),
        );
        assert_eq!(exact_target.len(), super::MAX_REQUEST_TARGET_BYTES);
        let exact_target_request =
            format!("GET {exact_target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let response = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            exact_target_request.as_bytes(),
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 200 \r\n"));

        let response = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            server_config,
            client_config,
            b"GET /api/v1/status HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 200 \r\n"));
    }

    #[tokio::test]
    async fn direct_tls_classifies_oversized_request_line_portions() {
        let (server_config, client_config) = tls_configs();

        for (name, request, status, body, allows_get) in [
            (
                "non-GET method",
                format!(
                    "{} /api/v1/status HTTP/1.1\r\n\r\n",
                    "P".repeat(super::MAX_REQUEST_HEAD_BYTES + 1)
                ),
                b"HTTP/1.1 405 \r\n".as_slice(),
                b"{\"error\":\"method_not_allowed\"}".as_slice(),
                true,
            ),
            (
                "malformed method framing after a valid token",
                format!(
                    "{}\t /api/v1/status HTTP/1.1\r\n\r\n",
                    "P".repeat(super::MAX_REQUEST_HEAD_BYTES)
                ),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
                false,
            ),
            (
                "HTTP version",
                format!(
                    "GET /api/v1/status HTTP/{}\r\n\r\n",
                    "1".repeat(super::MAX_REQUEST_HEAD_BYTES)
                ),
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
                false,
            ),
        ] {
            let response = direct_tls_response(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                request.as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert!(response.starts_with(status), "{name}: {response:?}");
            assert_eq!(
                response
                    .windows(b"Allow: GET\r\n".len())
                    .any(|window| window == b"Allow: GET\r\n"),
                allows_get,
                "{name}: {response:?}"
            );
            assert!(response.ends_with(body), "{name}: {response:?}");
        }
    }

    #[tokio::test]
    async fn direct_tls_retains_completed_request_line_classification_at_header_limit() {
        let (server_config, client_config) = tls_configs();
        let exact_headers = |request_line: &str| {
            format!(
                "{request_line}X-Pad: {}\r\n\r\n",
                "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len())
            )
        };
        let valid_non_get = exact_headers(&format!(
            "{} /api/v1/status HTTP/1.1\r\n",
            "P".repeat(11 * 1024 / 5)
        ));
        let malformed_version = exact_headers(&format!(
            "GET /api/v1/status HTTP/{}\r\n",
            "1".repeat(11 * 1024 / 5)
        ));
        let oversized_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len() + 1)
        );

        for (name, request, header_bytes, exceeds_aggregate_limit, status, body, allows_get) in [
            (
                "valid non-GET method",
                valid_non_get,
                super::MAX_REQUEST_HEADER_BYTES,
                true,
                b"HTTP/1.1 405 \r\n".as_slice(),
                b"{\"error\":\"method_not_allowed\"}".as_slice(),
                true,
            ),
            (
                "malformed HTTP version",
                malformed_version,
                super::MAX_REQUEST_HEADER_BYTES,
                true,
                b"HTTP/1.1 400 \r\n".as_slice(),
                b"{\"error\":\"bad_request\"}".as_slice(),
                false,
            ),
            (
                "oversized headers",
                oversized_headers,
                super::MAX_REQUEST_HEADER_BYTES + 1,
                false,
                b"HTTP/1.1 431 \r\n".as_slice(),
                b"{\"error\":\"request_header_fields_too_large\"}".as_slice(),
                false,
            ),
        ] {
            assert_eq!(
                raw_header_section_bytes(request.as_bytes()).unwrap(),
                header_bytes,
                "{name}: raw header section length"
            );
            assert_eq!(
                request.len() > super::MAX_REQUEST_HEAD_BYTES,
                exceeds_aggregate_limit,
                "{name}: aggregate request bound"
            );
            let response = direct_tls_response(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                request.as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert!(response.starts_with(status), "{name}: {response:?}");
            assert_eq!(
                response
                    .windows(b"Allow: GET\r\n".len())
                    .any(|window| window == b"Allow: GET\r\n"),
                allows_get,
                "{name}: {response:?}"
            );
            assert!(response.ends_with(body), "{name}: {response:?}");
        }
    }

    #[tokio::test]
    async fn direct_tls_rejects_lf_only_request_heads_before_read_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (server_config, client_config) = tls_configs();
        let server = tokio::spawn(serve_restricted_https_listener(
            listener,
            server_config,
            StartupOutcome::UninitializedWithoutDatabase,
        ));

        let mut client = tls_client(address, client_config).await;
        client
            .write_all(b"GET /api/v1/status HTTP/1.1\nHost: localhost\n\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut response))
            .await
            .expect("LF-only request head must not wait for the read timeout");
        assert_fixed_tls_response(&response, 400, "{\"error\":\"bad_request\"}", None);

        server.abort();
    }

    #[tokio::test]
    async fn direct_tls_rate_limit_returns_the_fixed_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (server_config, client_config) = tls_configs();
        let server = tokio::spawn(serve_restricted_https_listener(
            listener,
            server_config,
            StartupOutcome::UninitializedWithoutDatabase,
        ));

        for _ in 0..RATE_LIMIT_BURST {
            let mut client = tls_client(address, Arc::clone(&client_config)).await;
            client
                .write_all(b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            let _ = client.read_to_end(&mut response).await;
            assert!(response.starts_with(b"HTTP/1.1 200 \r\n"));
        }

        let mut client = tls_client(address, client_config).await;
        client
            .write_all(b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let _ = client.read_to_end(&mut response).await;
        assert!(response.starts_with(b"HTTP/1.1 429 \r\n"));
        assert!(response.ends_with(b"{\"error\":\"rate_limited\"}"));

        server.abort();
    }

    #[tokio::test]
    async fn direct_tls_rate_limit_head_response_has_empty_body() {
        let (server_config, client_config) = tls_configs();
        let rate_limiter = Arc::new(RateLimiter::new());

        for _ in 0..RATE_LIMIT_BURST {
            let response = direct_tls_response_with_rate_limiter(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                Arc::clone(&rate_limiter),
                b"HEAD /api/v1/status HTTP/1.1\r\n\r\n",
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert!(
                response.starts_with(b"HTTP/1.1 405 \r\n"),
                "burst HEAD should be 405, got: {:?}",
                String::from_utf8_lossy(&response[..response.len().min(64)])
            );
            assert_eq!(
                &response[response.len().saturating_sub(2)..],
                b"\r\n",
                "burst HEAD body must be empty"
            );
        }

        let response = direct_tls_response_with_rate_limiter(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            rate_limiter,
            b"HEAD /api/v1/status HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        // 429 status must be present and body must be empty (RFC 9110 §9.3.2).
        assert_eq!(
            response,
            b"HTTP/1.1 429 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n"
        );
    }

    #[tokio::test]
    async fn direct_tls_rate_limit_admits_two_consecutive_web_ui_page_loads() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (server_config, client_config) = tls_configs();
        let server = tokio::spawn(serve_restricted_https_listener(
            listener,
            server_config,
            StartupOutcome::UninitializedWithoutDatabase,
        ));

        for load in 0..2 {
            for target in [
                "/",
                "/assets/weavelit-application.js",
                "/assets/weavelit-application.css",
                "/api/v1/status",
            ] {
                let mut client = tls_client(address, Arc::clone(&client_config)).await;
                client
                    .write_all(
                        format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut response = Vec::new();
                let _ = client.read_to_end(&mut response).await;
                assert!(
                    response.starts_with(b"HTTP/1.1 200 \r\n"),
                    "load {load} of {target} was not admitted: {:?}",
                    String::from_utf8_lossy(&response[..response.len().min(64)])
                );
            }
        }

        server.abort();
    }

    #[tokio::test]
    async fn direct_tls_rate_limit_admits_completed_request_heads_before_fixed_responses() {
        let (server_config, client_config) = tls_configs();
        let oversized_target = format!(
            "GET /{} HTTP/1.1\r\n\r\n",
            "a".repeat(super::MAX_REQUEST_TARGET_BYTES)
        );
        let oversized_headers = format!(
            "GET /api/v1/status HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(super::MAX_REQUEST_HEADER_BYTES - "X-Pad: \r\n".len() + 1)
        );

        for (name, request, fixed_status, fixed_body, allow) in [
            (
                "valid POST",
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                405,
                "{\"error\":\"method_not_allowed\"}",
                Some(AllowedMethod::Get),
            ),
            (
                "malformed head",
                "GET /api/v1/status HTTP/1.1\r\nnot-a-header\r\n\r\n".to_owned(),
                400,
                "{\"error\":\"bad_request\"}",
                None,
            ),
            (
                "oversized target",
                oversized_target,
                414,
                "{\"error\":\"uri_too_long\"}",
                None,
            ),
            (
                "oversized headers",
                oversized_headers,
                431,
                "{\"error\":\"request_header_fields_too_large\"}",
                None,
            ),
            (
                "valid GET",
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                None,
            ),
        ] {
            let rate_limiter = Arc::new(RateLimiter::new());
            for _ in 0..RATE_LIMIT_BURST {
                let response = direct_tls_response_with_rate_limiter(
                    restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                    Arc::clone(&server_config),
                    Arc::clone(&client_config),
                    Arc::clone(&rate_limiter),
                    request.as_bytes(),
                    REQUEST_READ_TIMEOUT,
                    REQUEST_PROCESSING_TIMEOUT,
                )
                .await;
                assert_fixed_tls_response(&response, fixed_status, fixed_body, allow);
            }

            let response = direct_tls_response_with_rate_limiter(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                rate_limiter,
                request.as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert_fixed_tls_response(&response, 429, "{\"error\":\"rate_limited\"}", None);
            assert!(response.len() <= 128, "{name}");
        }
    }

    #[tokio::test]
    async fn direct_tls_rate_limit_excludes_incomplete_heads_and_read_timeouts() {
        let (server_config, client_config) = tls_configs();

        for (name, request, request_read_timeout, eof, status, body) in [
            (
                "read timeout",
                b"".as_slice(),
                Duration::ZERO,
                false,
                408,
                "{\"error\":\"request_timeout\"}",
            ),
            (
                "incomplete EOF",
                b"GET /api/v1/status HTTP/1.1\r\n".as_slice(),
                REQUEST_READ_TIMEOUT,
                true,
                400,
                "{\"error\":\"bad_request\"}",
            ),
        ] {
            let rate_limiter = Arc::new(RateLimiter::new());
            let response = if eof {
                direct_tls_response_after_eof(
                    restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                    Arc::clone(&server_config),
                    Arc::clone(&client_config),
                    Arc::clone(&rate_limiter),
                    request,
                    request_read_timeout,
                    REQUEST_PROCESSING_TIMEOUT,
                )
                .await
            } else {
                direct_tls_response_with_rate_limiter(
                    restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                    Arc::clone(&server_config),
                    Arc::clone(&client_config),
                    Arc::clone(&rate_limiter),
                    request,
                    request_read_timeout,
                    REQUEST_PROCESSING_TIMEOUT,
                )
                .await
            };
            assert_fixed_tls_response(&response, status, body, None);

            for _ in 0..RATE_LIMIT_BURST {
                let response = direct_tls_response_with_rate_limiter(
                    restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                    Arc::clone(&server_config),
                    Arc::clone(&client_config),
                    Arc::clone(&rate_limiter),
                    b"GET /api/v1/status HTTP/1.1\r\n\r\n",
                    REQUEST_READ_TIMEOUT,
                    REQUEST_PROCESSING_TIMEOUT,
                )
                .await;
                assert!(response.starts_with(b"HTTP/1.1 200 \r\n"), "{name}");
            }
        }
    }

    #[tokio::test]
    async fn direct_tls_maps_read_and_processing_timeouts_to_fixed_responses() {
        let (server_config, client_config) = tls_configs();
        let read_timeout = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"",
            Duration::ZERO,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert!(read_timeout.starts_with(b"HTTP/1.1 408 \r\n"));
        assert!(read_timeout.ends_with(b"{\"error\":\"request_timeout\"}"));

        let processing_timeout = direct_tls_response(
            Router::new().route(
                "/slow",
                any(|| async { std::future::pending::<Response>().await }),
            ),
            server_config,
            client_config,
            b"GET /slow HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            Duration::ZERO,
        )
        .await;
        assert!(processing_timeout.starts_with(b"HTTP/1.1 504 \r\n"));
        assert!(processing_timeout.ends_with(b"{\"error\":\"gateway_timeout\"}"));
    }

    #[tokio::test]
    async fn direct_tls_fixed_responses_fit_the_wire_limit() {
        let (server_config, client_config) = tls_configs();
        let rate_limiter = Arc::new(RateLimiter::new());
        let source = "127.0.0.1".parse().unwrap();
        let now = std::time::Instant::now();
        for _ in 0..RATE_LIMIT_BURST {
            assert!(rate_limiter.allows(source, now));
        }

        let mut maximum_response_bytes = 0;
        for (
            name,
            router,
            request,
            request_read_timeout,
            processing_timeout,
            rate_limiter,
            status,
            body,
            allows_get,
            rejection,
        ) in [
            (
                "uninitialized without database",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                false,
                false,
            ),
            (
                "uninitialized with database",
                restricted_routes(StartupOutcome::UninitializedWithDatabase),
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}",
                false,
                false,
            ),
            (
                "bad request",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                "GET /api/v1/status HTTP/1.1\r\nAccept: text/html\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                400,
                "{\"error\":\"bad_request\"}",
                false,
                false,
            ),
            (
                "method not allowed",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                405,
                "{\"error\":\"method_not_allowed\"}",
                true,
                false,
            ),
            (
                "not found",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                "GET /absent HTTP/1.1\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                404,
                "{\"error\":\"not_found\"}",
                false,
                false,
            ),
            (
                "target too long",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(11 * 1024)),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                414,
                "{\"error\":\"uri_too_long\"}",
                false,
                false,
            ),
            (
                "headers too large",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                format!(
                    "GET /api/v1/status HTTP/1.1\r\nX-Bound: {}value\r\n\r\n",
                    " ".repeat(8 * 1024)
                ),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                431,
                "{\"error\":\"request_header_fields_too_large\"}",
                false,
                false,
            ),
            (
                "rate limited",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::clone(&rate_limiter),
                429,
                "{\"error\":\"rate_limited\"}",
                false,
                false,
            ),
            (
                "request timeout",
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                String::new(),
                Duration::ZERO,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                408,
                "{\"error\":\"request_timeout\"}",
                false,
                false,
            ),
            (
                "processing timeout",
                Router::new().route(
                    "/slow",
                    any(|| async { std::future::pending::<Response>().await }),
                ),
                "GET /slow HTTP/1.1\r\n\r\n".to_owned(),
                REQUEST_READ_TIMEOUT,
                Duration::ZERO,
                Arc::new(RateLimiter::new()),
                504,
                "{\"error\":\"gateway_timeout\"}",
                false,
                false,
            ),
            (
                "service unavailable",
                Router::new(),
                String::new(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
                Arc::new(RateLimiter::new()),
                503,
                "{\"error\":\"service_unavailable\"}",
                false,
                true,
            ),
        ] {
            let response = if rejection {
                direct_tls_rejection_response(
                    Arc::clone(&server_config),
                    Arc::clone(&client_config),
                )
                .await
            } else {
                direct_tls_response_with_rate_limiter(
                    router,
                    Arc::clone(&server_config),
                    Arc::clone(&client_config),
                    rate_limiter,
                    request.as_bytes(),
                    request_read_timeout,
                    processing_timeout,
                )
                .await
            };
            maximum_response_bytes = maximum_response_bytes.max(response.len());
            let response = std::str::from_utf8(&response).unwrap();
            assert!(
                response.len() <= 128,
                "{name} response exceeds 128 bytes: {response:?}"
            );
            assert!(
                response.starts_with(&format!("HTTP/1.1 {status} \r\n")),
                "{name}"
            );
            assert!(response.contains("Content-Type: application/json; charset=utf-8\r\n"));
            assert_eq!(response.contains("Allow: GET\r\n"), allows_get, "{name}");
            assert!(response.ends_with(body), "{name}: {response:?}");
        }
        assert_eq!(maximum_response_bytes, 119);
    }

    #[tokio::test]
    async fn direct_tls_delivers_embedded_assets_with_fixed_security_headers() {
        let (server_config, client_config) = tls_configs();
        for (target, relative, media_type) in EMBEDDED_ASSETS {
            let response = direct_tls_response(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            let head = format!(
                "HTTP/1.1 200 \r\nContent-Type: {media_type}\r\n{ASSET_SECURITY_HEADERS}\r\n"
            );
            let body = generated_asset_bytes(relative);
            assert!(response.starts_with(head.as_bytes()), "{target}");
            assert_eq!(response.len(), head.len() + body.len(), "{target}");
            assert_eq!(&response[head.len()..], body.as_slice(), "{target}");
            let head = std::str::from_utf8(&response[..head.len()]).unwrap();
            assert!(!head.contains("Allow:"), "{target}");
            assert!(!head.contains("Content-Length:"), "{target}");
            for forbidden in [
                "Access-Control",
                "Set-Cookie",
                "Content-Encoding",
                "Server:",
                "Vary:",
            ] {
                assert!(!head.contains(forbidden), "{target}: {forbidden}");
            }
        }
    }

    #[tokio::test]
    async fn direct_tls_asset_paths_reject_non_get_methods_and_unknown_targets() {
        let (server_config, client_config) = tls_configs();
        for (target, _, _) in EMBEDDED_ASSETS {
            let rejected = direct_tls_response(
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                format!("POST {target} HTTP/1.1\r\n\r\n").as_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert_fixed_tls_response(
                &rejected,
                405,
                "{\"error\":\"method_not_allowed\"}",
                Some(AllowedMethod::Get),
            );
        }

        let unknown = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"GET /assets/../assets/weavelit-application.js HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(&unknown, 404, "{\"error\":\"not_found\"}", None);

        let gated = direct_tls_response(
            restricted_routes(StartupOutcome::InitializationPending(
                weavelit_server_lifecycle::WorkflowKind::Init,
            )),
            server_config,
            client_config,
            b"GET / HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(&gated, 404, "{\"error\":\"not_found\"}", None);
    }

    #[tokio::test]
    async fn direct_tls_bounds_and_redacts_module_response_bodies() {
        let (server_config, client_config) = tls_configs();
        let html_router = |byte_length: usize| {
            Router::new().route(
                "/bounded",
                any(move || async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html; charset=utf-8")
                        .body(Body::from(vec![b'a'; byte_length]))
                        .unwrap()
                }),
            )
        };

        let at_limit = direct_tls_response(
            html_router(super::MAX_HTML_BODY_BYTES),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"GET /bounded HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        let head = format!(
            "HTTP/1.1 200 \r\nContent-Type: text/html; charset=utf-8\r\n{ASSET_SECURITY_HEADERS}\r\n"
        );
        assert_eq!(at_limit.len(), head.len() + super::MAX_HTML_BODY_BYTES);

        let over_limit = direct_tls_response(
            html_router(super::MAX_HTML_BODY_BYTES + 1),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"GET /bounded HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(&over_limit, 200, "{\"error\":\"gateway_timeout\"}", None);

        for media_type in ["text/plain; charset=utf-8", "application/octet-stream"] {
            let unknown_media_type = direct_tls_response(
                Router::new().route(
                    "/bounded",
                    any(move || async move {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", media_type)
                            .body(Body::from("secret"))
                            .unwrap()
                    }),
                ),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                b"GET /bounded HTTP/1.1\r\n\r\n",
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert_fixed_tls_response(
                &unknown_media_type,
                200,
                "{\"error\":\"gateway_timeout\"}",
                None,
            );
        }

        let unknown_json = direct_tls_response(
            Router::new().route(
                "/bounded",
                any(|| async {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json; charset=utf-8")
                        .body(Body::from("{\"secret\":true}"))
                        .unwrap()
                }),
            ),
            server_config,
            client_config,
            b"GET /bounded HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(&unknown_json, 200, "{\"error\":\"gateway_timeout\"}", None);
    }

    /// Every database-selection body the Web UI Client Module can emit must be
    /// in the fixed-JSON allowlist, or the bounding step silently replaces it
    /// with the redacted gateway-timeout body.
    #[tokio::test]
    async fn database_selection_bodies_survive_the_fixed_json_allowlist() {
        let success =
            weavelit_module_client::database_selection_response(&LifecycleProjection::new(true));
        let bounded = bounded_response_from_axum(success).await;
        assert_eq!(bounded.status, StatusCode::OK);
        assert_eq!(
            bounded.body.as_ref(),
            b"{\"lifecycle\":\"uninitialized\",\"database_selected\":true}".as_slice()
        );

        for rejection in [
            DatabaseSelectionRejection::BadRequest,
            DatabaseSelectionRejection::RequestOriginDenied,
            DatabaseSelectionRejection::MethodNotAllowed,
            DatabaseSelectionRejection::DatabaseSelectionNotAllowed,
            DatabaseSelectionRejection::ServiceUnavailable,
        ] {
            let expected = rejection.body();
            let bounded = bounded_response_from_axum(rejection.response()).await;
            assert_eq!(bounded.status, rejection.status(), "{rejection:?}");
            assert_eq!(bounded.profile, ResponseProfile::Json, "{rejection:?}");
            assert_eq!(
                bounded.body.as_ref(),
                expected.as_bytes(),
                "{rejection:?} was redacted instead of returned"
            );
        }
    }

    /// Every Restore body a route handler can emit must be in the fixed-JSON
    /// allowlist too.
    ///
    /// Two Restore rejections are answered by the pre-body path, which carries
    /// its own fixed responses and never reaches this step. The rest are
    /// produced by the mounted handler, so an omission here would silently
    /// replace the documented stable code with the redacted gateway-timeout
    /// body and leave a submitting client unable to tell one failure from
    /// another.
    #[tokio::test]
    async fn restore_bodies_survive_the_fixed_json_allowlist() {
        for rejection in [
            RestoreRejection::BadRequest,
            RestoreRejection::RecoveryKeyInvalid,
            RestoreRejection::BackupInvalid,
            RestoreRejection::BackupIncompatible,
            RestoreRejection::RequestOriginDenied,
            RestoreRejection::RestoreTicketInvalid,
            RestoreRejection::MethodNotAllowed,
            RestoreRejection::RestoreNotAllowed,
            RestoreRejection::RestorePending,
            RestoreRejection::RestoreFailed,
            RestoreRejection::ServiceUnavailable,
        ] {
            let expected = rejection.body();
            let bounded = bounded_response_from_axum(rejection.response()).await;
            assert_eq!(bounded.status, rejection.status(), "{rejection:?}");
            assert_eq!(bounded.profile, ResponseProfile::Json, "{rejection:?}");
            assert_eq!(
                bounded.body.as_ref(),
                expected.as_bytes(),
                "{rejection:?} was redacted instead of returned"
            );
        }
    }

    /// Every Init body a route handler can emit must be in the fixed-JSON
    /// allowlist too.
    ///
    /// This walks the whole declared rejection contract rather than a
    /// restatement of it, so a variant added to the contract without an
    /// allowlist entry fails here instead of reaching a submitting client as
    /// the redacted gateway-timeout body. That silent replacement is exactly
    /// what happened to the Restore codes, and it went unnoticed because the
    /// tests of the day only checked that the codes were defined.
    #[tokio::test]
    async fn init_bodies_survive_the_fixed_json_allowlist() {
        assert_eq!(InitRejection::ALL.len(), 8);
        for rejection in InitRejection::ALL {
            let rejection = *rejection;
            let expected = rejection.body();
            let bounded = bounded_response_from_axum(rejection.response()).await;
            assert_eq!(bounded.status, rejection.status(), "{rejection:?}");
            assert_eq!(bounded.profile, ResponseProfile::Json, "{rejection:?}");
            assert_eq!(
                bounded.body.as_ref(),
                expected.as_bytes(),
                "{rejection:?} was redacted instead of returned"
            );
        }
    }

    /// The recovery-key delivery envelope must survive the bounding step.
    ///
    /// It is the only response in the product that ever carries a private
    /// recovery key, and a bound that truncated or redacted it would strand the
    /// deployment with a key it can never recover.
    #[tokio::test]
    async fn the_init_recovery_key_envelope_survives_the_bounding_step() {
        const KEY: &str = "AGE-SECRET-KEY-1QQPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L";

        let delivery = typed_json_response(
            StatusCode::OK,
            TypedJsonEnvelope::Result {
                result: TypedResult::new()
                    .with_field(
                        StableCode::new("recovery_key").unwrap(),
                        TypedValue::RecoveryKey(RecoveryKeyLine::new(KEY).unwrap()),
                    )
                    .unwrap(),
                correlation_id: ResponseCorrelation::new("0123456789abcdef").unwrap(),
            },
        );
        let bounded = bounded_response_from_axum(delivery).await;
        assert_eq!(bounded.status, StatusCode::OK);
        assert_eq!(bounded.profile, ResponseProfile::TypedJson);
        assert!(bounded.body.len() <= MAX_TYPED_JSON_BODY_BYTES);
        assert_eq!(
            bounded.body.as_ref(),
            format!(
                "{{\"result\":{{\"recovery_key\":\"{KEY}\"}},\"correlation_id\":\"0123456789abcdef\"}}"
            )
            .as_bytes(),
            "the delivered recovery key was redacted instead of returned"
        );
    }

    #[tokio::test]
    async fn direct_tls_listener_never_answers_a_cleartext_request() {
        let (server_config, _client_config) = tls_configs();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, source) = listener.accept().await.unwrap();
            serve_normal_connection_with_timeouts(
                stream,
                source.ip(),
                TlsAcceptor::from(server_config),
                MountedSurface::without_registrations(restricted_routes(
                    StartupOutcome::UninitializedWithoutDatabase,
                )),
                Arc::new(RateLimiter::new()),
                ConnectionTimeouts {
                    handshake: TLS_HANDSHAKE_TIMEOUT,
                    request_read: REQUEST_READ_TIMEOUT,
                    processing: REQUEST_PROCESSING_TIMEOUT,
                },
            )
            .await;
        });
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response).await;
        server.await.unwrap();
        assert!(!response.starts_with(b"HTTP/"), "{response:?}");
        assert!(
            !response
                .windows(b"<!doctype".len())
                .any(|window| window.eq_ignore_ascii_case(b"<!doctype")),
            "{response:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Application Database selection route
    // -----------------------------------------------------------------------

    /// Builds a selection wire request, substituting the real bound authority.
    fn selection_wire(method: &str, headers: &str, body: &str, authority: SocketAddr) -> Vec<u8> {
        let headers = headers.replace("{authority}", &authority.to_string());
        format!("{method} {APPLICATION_DATABASE_ROUTE} HTTP/1.1\r\n{headers}\r\n{body}")
            .into_bytes()
    }

    /// The header block a compliant same-origin browser request carries.
    fn valid_selection_headers(body_length: usize) -> String {
        format!(
            "Host: {{authority}}\r\nOrigin: https://{{authority}}\r\nX-Weavelit-CSRF: 1\r\n\
             Content-Type: application/json\r\nContent-Length: {body_length}\r\n"
        )
    }

    /// Serves one direct-TLS request against routes composed for the address the
    /// listener actually bound, so the expected origin is the real authority.
    async fn direct_tls_bound_response(
        surface: &Surface,
        bind: &str,
        configs: &(Arc<ServerConfig>, Arc<ClientConfig>),
        request_read_timeout: Duration,
        build: impl FnOnce(SocketAddr) -> Vec<u8>,
    ) -> Vec<u8> {
        let listener = TcpListener::bind(bind).await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = surface.routes_for(address);
        let request = build(address);
        let server_config = Arc::clone(&configs.0);
        let server = tokio::spawn(async move {
            let (stream, source) = listener.accept().await.unwrap();
            serve_normal_connection_with_timeouts(
                stream,
                source.ip(),
                TlsAcceptor::from(server_config),
                MountedSurface::without_registrations(router),
                Arc::new(RateLimiter::new()),
                ConnectionTimeouts {
                    handshake: TLS_HANDSHAKE_TIMEOUT,
                    request_read: request_read_timeout,
                    processing: REQUEST_PROCESSING_TIMEOUT,
                },
            )
            .await;
        });
        let response = tls_exchange(address, Arc::clone(&configs.1), &request).await;
        server.await.unwrap();
        response
    }

    async fn tls_exchange(
        address: SocketAddr,
        client_config: Arc<ClientConfig>,
        request: &[u8],
    ) -> Vec<u8> {
        let mut client = tls_client(address, client_config).await;
        if !request.is_empty() {
            client.write_all(request).await.unwrap();
        }
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("direct TLS response must complete with close_notify");
        response
    }

    async fn direct_tls_selection(
        surface: &Surface,
        configs: &(Arc<ServerConfig>, Arc<ClientConfig>),
        method: &str,
        headers: &str,
        body: &str,
    ) -> Vec<u8> {
        let headers = headers.to_owned();
        let body = body.to_owned();
        direct_tls_bound_response(
            surface,
            "127.0.0.1:0",
            configs,
            REQUEST_READ_TIMEOUT,
            move |address| selection_wire(method, &headers, &body, address),
        )
        .await
    }

    /// Performs one valid selection and returns the wire response.
    async fn direct_tls_valid_selection(
        surface: &Surface,
        configs: &(Arc<ServerConfig>, Arc<ClientConfig>),
    ) -> Vec<u8> {
        direct_tls_selection(
            surface,
            configs,
            "PUT",
            &valid_selection_headers(SELECTION_BODY.len()),
            SELECTION_BODY,
        )
        .await
    }

    #[test]
    fn selection_body_length_matches_the_documented_contract() {
        assert_eq!(SELECTION_BODY.len(), 34);
        assert_eq!(
            MAX_REQUEST_BODY_BYTES,
            weavelit_module_client::MAX_DATABASE_SELECTION_BODY_BYTES
        );
    }

    /// A successful selection must be observable by a later status read served
    /// by the same process, listener, and arbiter.
    #[tokio::test]
    async fn selection_is_visible_to_a_later_status_read_on_the_same_listener() {
        let (server_config, client_config) = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(
            super::serve_restricted_https_listener(
                listener,
                server_config,
                &surface.startup,
                never_signalled(),
            )
            .unwrap(),
        );

        let before = tls_exchange(
            address,
            Arc::clone(&client_config),
            b"GET /api/v1/status HTTP/1.1\r\n\r\n",
        )
        .await;
        assert_fixed_tls_response(&before, 200, UNSELECTED_STATUS, None);

        let selection = tls_exchange(
            address,
            Arc::clone(&client_config),
            &selection_wire(
                "PUT",
                &valid_selection_headers(SELECTION_BODY.len()),
                SELECTION_BODY,
                address,
            ),
        )
        .await;
        assert_fixed_tls_response(&selection, 200, SELECTED_STATUS, None);

        let after = tls_exchange(
            address,
            client_config,
            b"GET /api/v1/status HTTP/1.1\r\n\r\n",
        )
        .await;
        assert_fixed_tls_response(&after, 200, SELECTED_STATUS, None);

        server.abort();
    }

    /// The status route and the selection route must observe one arbiter even
    /// when each request is served by a separately composed router.
    #[tokio::test]
    async fn selection_and_status_routes_share_one_arbiter_across_routers() {
        let configs = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);

        let selection = direct_tls_valid_selection(&surface, &configs).await;
        assert_fixed_tls_response(&selection, 200, SELECTED_STATUS, None);

        let status = surface
            .routes()
            .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(response_body(status).await, SELECTED_STATUS);

        // The durable projection agrees with what the routes reported.
        assert!(
            surface
                .startup
                .composition
                .adapter
                .arbiter
                .projection()
                .unwrap()
                .database_selected()
        );
    }

    #[tokio::test]
    async fn exact_selection_replay_changes_no_locator_generation_or_bytes() {
        let configs = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);

        let first = direct_tls_valid_selection(&surface, &configs).await;
        assert_fixed_tls_response(&first, 200, SELECTED_STATUS, None);
        let committed = surface.anchor_snapshot();
        assert!(
            committed
                .iter()
                .any(|(name, ..)| name.to_string_lossy().starts_with("database-locator-")),
            "the first selection must publish a locator"
        );

        let replay = direct_tls_valid_selection(&surface, &configs).await;
        assert_fixed_tls_response(&replay, 200, SELECTED_STATUS, None);
        assert_eq!(
            surface.anchor_snapshot(),
            committed,
            "an exact replay must not rotate or rewrite the locator"
        );
    }

    #[tokio::test]
    async fn selection_route_method_matrix_advertises_its_own_allow_value() {
        let configs = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);

        for method in ["GET", "POST", "DELETE"] {
            let response = direct_tls_selection(
                &surface,
                &configs,
                method,
                &valid_selection_headers(0).replace("Content-Length: 0\r\n", ""),
                "",
            )
            .await;
            assert_fixed_tls_response(
                &response,
                405,
                "{\"error\":\"method_not_allowed\"}",
                Some(AllowedMethod::Put),
            );
        }

        // A `PUT` must declare a body, so the status route's own `405` is only
        // reachable through an explicitly empty one.
        let status = direct_tls_bound_response(
            &surface,
            "127.0.0.1:0",
            &configs,
            REQUEST_READ_TIMEOUT,
            |_| b"PUT /api/v1/status HTTP/1.1\r\nContent-Length: 0\r\n\r\n".to_vec(),
        )
        .await;
        assert_fixed_tls_response(
            &status,
            405,
            "{\"error\":\"method_not_allowed\"}",
            Some(AllowedMethod::Get),
        );
    }

    #[tokio::test]
    async fn selection_route_accepts_a_body_at_the_limit_and_rejects_bad_framing() {
        let configs = tls_configs();

        let padded = format!(
            "{SELECTION_BODY}{}",
            " ".repeat(MAX_REQUEST_BODY_BYTES - SELECTION_BODY.len())
        );
        assert_eq!(padded.len(), MAX_REQUEST_BODY_BYTES);
        let at_limit = direct_tls_selection(
            &Surface::new(StartupOutcome::UninitializedWithoutDatabase),
            &configs,
            "PUT",
            &valid_selection_headers(padded.len()),
            &padded,
        )
        .await;
        assert_fixed_tls_response(&at_limit, 200, SELECTED_STATUS, None);

        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let over_limit = format!(
            "{SELECTION_BODY}{}",
            " ".repeat(MAX_REQUEST_BODY_BYTES + 1 - SELECTION_BODY.len())
        );
        for (name, headers, body) in [
            (
                "body over the limit",
                valid_selection_headers(over_limit.len()),
                over_limit.as_str(),
            ),
            (
                "missing content length",
                valid_selection_headers(SELECTION_BODY.len())
                    .replace(&format!("Content-Length: {}\r\n", SELECTION_BODY.len()), ""),
                SELECTION_BODY,
            ),
            (
                "non-canonical content length",
                valid_selection_headers(SELECTION_BODY.len())
                    .replace("Content-Length: 34\r\n", "Content-Length: 034\r\n"),
                SELECTION_BODY,
            ),
            (
                "duplicate content length",
                format!(
                    "{}Content-Length: 34\r\n",
                    valid_selection_headers(SELECTION_BODY.len())
                ),
                SELECTION_BODY,
            ),
            (
                "chunked transfer encoding",
                format!(
                    "{}Transfer-Encoding: chunked\r\n",
                    valid_selection_headers(SELECTION_BODY.len())
                ),
                SELECTION_BODY,
            ),
        ] {
            let response = direct_tls_selection(&surface, &configs, "PUT", &headers, body).await;
            assert_fixed_tls_response(&response, 400, "{\"error\":\"bad_request\"}", None);
            assert!(
                !surface
                    .startup
                    .composition
                    .adapter
                    .arbiter
                    .projection()
                    .unwrap()
                    .database_selected(),
                "{name} must not commit a selection"
            );
        }
    }

    #[tokio::test]
    async fn selection_route_media_matrix_accepts_only_the_documented_types() {
        let configs = tls_configs();
        let valid = valid_selection_headers(SELECTION_BODY.len());

        for accept in ["", "Accept: application/json\r\n"] {
            let response = direct_tls_selection(
                &Surface::new(StartupOutcome::UninitializedWithoutDatabase),
                &configs,
                "PUT",
                &format!("{valid}{accept}"),
                SELECTION_BODY,
            )
            .await;
            assert_fixed_tls_response(&response, 200, SELECTED_STATUS, None);
        }

        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        for headers in [
            valid.replace(
                "Content-Type: application/json\r\n",
                "Content-Type: application/json; charset=utf-8\r\n",
            ),
            valid.replace("Content-Type: application/json\r\n", ""),
            format!("{valid}Content-Type: application/json\r\n"),
            format!("{valid}Accept: text/html\r\n"),
            format!("{valid}Accept: application/json\r\nAccept: application/json\r\n"),
        ] {
            let response =
                direct_tls_selection(&surface, &configs, "PUT", &headers, SELECTION_BODY).await;
            assert_fixed_tls_response(&response, 400, "{\"error\":\"bad_request\"}", None);
        }
    }

    #[tokio::test]
    async fn selection_route_denies_every_failed_origin_or_csrf_precondition() {
        let configs = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let valid = valid_selection_headers(SELECTION_BODY.len());

        for headers in [
            valid.replace("Origin: https://{authority}\r\n", ""),
            format!("{valid}Origin: https://{{authority}}\r\n"),
            valid.replace(
                "Origin: https://{authority}\r\n",
                "Origin: https://127.0.0.1:1\r\n",
            ),
            valid.replace(
                "Origin: https://{authority}\r\n",
                "Origin: http://{authority}\r\n",
            ),
            valid.replace("Origin: https://{authority}\r\n", "Origin: null\r\n"),
            valid.replace("Host: {authority}\r\n", ""),
            format!("{valid}Host: {{authority}}\r\n"),
            valid.replace("Host: {authority}\r\n", "Host: localhost\r\n"),
            valid.replace("X-Weavelit-CSRF: 1\r\n", ""),
            valid.replace("X-Weavelit-CSRF: 1\r\n", "X-Weavelit-CSRF: 0\r\n"),
            format!("{valid}X-Weavelit-CSRF: 1\r\n"),
        ] {
            let response =
                direct_tls_selection(&surface, &configs, "PUT", &headers, SELECTION_BODY).await;
            assert_fixed_tls_response(
                &response,
                403,
                "{\"error\":\"request_origin_denied\"}",
                None,
            );
        }
        assert!(
            !surface
                .startup
                .composition
                .adapter
                .arbiter
                .projection()
                .unwrap()
                .database_selected()
        );
    }

    #[tokio::test]
    async fn selection_route_accepts_ipv6_literals_and_default_port_normalization() {
        let configs = tls_configs();

        let ipv6 = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let headers = valid_selection_headers(SELECTION_BODY.len());
        let response = direct_tls_bound_response(
            &ipv6,
            "[::1]:0",
            &configs,
            REQUEST_READ_TIMEOUT,
            |address| selection_wire("PUT", &headers, SELECTION_BODY, address),
        )
        .await;
        assert_fixed_tls_response(&response, 200, SELECTED_STATUS, None);

        // Port 443 cannot be bound by a test, so the expected origin is composed
        // for it directly while the socket stays on an ephemeral port.
        for authority in ["127.0.0.1", "127.0.0.1:443"] {
            let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
            let router = surface.routes_for("127.0.0.1:443".parse().unwrap());
            let headers =
                valid_selection_headers(SELECTION_BODY.len()).replace("{authority}", authority);
            let response = direct_tls_response(
                router,
                Arc::clone(&configs.0),
                Arc::clone(&configs.1),
                &format!(
                    "PUT {APPLICATION_DATABASE_ROUTE} HTTP/1.1\r\n{headers}\r\n{SELECTION_BODY}"
                )
                .into_bytes(),
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert_fixed_tls_response(&response, 200, SELECTED_STATUS, None);
        }
    }

    #[tokio::test]
    async fn selection_route_rejects_every_schema_deviation() {
        let configs = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);

        for body in [
            "",
            "{}",
            "{\"backend\":\"sqlite\"}",
            "{\"backend\":\"sqlite\",\"settings\":{},\"extra\":1}",
            "{\"backend\":\"sqlite\",\"settings\":{\"path\":\"/etc\"}}",
            "{\"backend\":\"sqlite\",\"backend\":\"sqlite\",\"settings\":{}}",
            "{\"backend\":1,\"settings\":{}}",
            "{\"backend\":\"SQLITE\",\"settings\":{}}",
            "{\"backend\":\"postgres\",\"settings\":{}}",
            "{\"backend\":\"sqlite\",\"settings\":[]}",
            "{\"backend\":\"sqlite\",\"settings\":{}}trailing",
            "{\"backend\":\"sqlite\",\"settings\":{}}{}",
        ] {
            let response = direct_tls_selection(
                &surface,
                &configs,
                "PUT",
                &valid_selection_headers(body.len()),
                body,
            )
            .await;
            assert_fixed_tls_response(&response, 400, "{\"error\":\"bad_request\"}", None);
        }
        assert!(
            !surface
                .startup
                .composition
                .adapter
                .arbiter
                .projection()
                .unwrap()
                .database_selected()
        );
    }

    #[tokio::test]
    async fn selection_route_read_timeout_returns_the_fixed_response() {
        let configs = tls_configs();
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let headers = valid_selection_headers(SELECTION_BODY.len());

        let response = direct_tls_bound_response(
            &surface,
            "127.0.0.1:0",
            &configs,
            Duration::from_millis(50),
            |address| {
                // A declared body that never arrives must not restart the budget.
                selection_wire("PUT", &headers, "{\"backend\":", address)
            },
        )
        .await;
        assert_fixed_tls_response(&response, 408, "{\"error\":\"request_timeout\"}", None);
        assert!(
            !surface
                .startup
                .composition
                .adapter
                .arbiter
                .projection()
                .unwrap()
                .database_selected()
        );
    }

    #[tokio::test]
    async fn selection_route_is_mounted_under_both_uninitialized_gates_only() {
        for outcome in [
            StartupOutcome::UninitializedWithoutDatabase,
            StartupOutcome::UninitializedWithDatabase,
        ] {
            let response = restricted_routes(outcome)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(APPLICATION_DATABASE_ROUTE)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{outcome:?}"
            );
            assert_eq!(response.headers().get("allow").unwrap(), "PUT");
        }

        let gated = restricted_routes(StartupOutcome::InitializationPending(
            weavelit_server_lifecycle::WorkflowKind::Init,
        ))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(APPLICATION_DATABASE_ROUTE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(gated.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_body(gated).await, "{\"error\":\"not_found\"}");
    }

    /// A retained database already satisfies the selection, so the live status
    /// route must report it before any request in this session.
    #[tokio::test]
    async fn a_retained_selection_is_reported_live_at_first_status_read() {
        let surface = Surface::new(StartupOutcome::UninitializedWithDatabase);
        let response = surface
            .routes()
            .oneshot(Request::get("/api/v1/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body(response).await, SELECTED_STATUS);
    }

    #[test]
    fn rate_limiter_allows_the_configured_burst_then_refills_at_the_sustained_rate() {
        let limiter = RateLimiter::new();
        let source = "127.0.0.1".parse().unwrap();
        let now = std::time::Instant::now();
        for _ in 0..RATE_LIMIT_BURST {
            assert!(limiter.allows(source, now));
        }
        assert!(!limiter.allows(source, now));
        let refill = Duration::from_secs(60 / u64::from(RATE_LIMIT_REQUESTS_PER_MINUTE));
        assert!(limiter.allows(source, now + refill));
    }

    #[tokio::test]
    async fn request_and_processing_timeouts_use_fixed_responses() {
        let (mut client, mut server) = tokio::io::duplex(1);
        let read = tokio::spawn(async move {
            read_request_head_until(&mut server, super::Deadline::now()).await
        });
        let _ = client.write_all(b"").await;
        assert!(matches!(
            read.await.unwrap(),
            super::RequestHeadRead::Incomplete(super::RequestReadError::TimedOut)
        ));

        let response = processing_response(
            ProcessingDeadline::new(super::Deadline::now()),
            pending::<BoundedResponse>(),
        )
        .await;
        assert_eq!(response.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(response.body, gateway_timeout_response().body);
        assert_eq!(
            request_timeout_response().body.as_ref(),
            b"{\"error\":\"request_timeout\"}"
        );
    }

    // -----------------------------------------------------------------------
    // Lifecycle adapter: blocking I/O isolation and timeout correctness
    // -----------------------------------------------------------------------

    /// Projection reads must not compete with the mutation semaphore; even while
    /// the single mutation permit is held, `project()` must complete.
    #[tokio::test]
    async fn adapter_projection_does_not_wait_for_mutation_lane() {
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let adapter = &surface.startup.composition.adapter;

        // Hold the single mutation permit so any select() would block.
        let _permit = Arc::clone(&adapter.mutation_lane)
            .acquire_owned()
            .await
            .unwrap();

        // project() is independent of the mutation lane and must complete.
        let projection = adapter.project().await;
        assert!(projection.is_some());
    }

    /// A selection future that is cancelled while awaiting the mutation
    /// semaphore must not commit any durable state.
    #[tokio::test]
    async fn adapter_selection_cancelled_at_semaphore_does_not_commit() {
        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let adapter = Arc::clone(&surface.startup.composition.adapter);
        let catalog = Arc::clone(&surface.startup.composition.catalog);
        let context = Arc::clone(&surface.startup.composition.context);

        // Hold the mutation permit so select() blocks at acquire_owned().
        let permit = Arc::clone(&adapter.mutation_lane)
            .acquire_owned()
            .await
            .unwrap();

        let handle = tokio::spawn(async move {
            adapter
                .select(
                    weavelit_module_client::SelectedBackend::Sqlite,
                    catalog,
                    context,
                )
                .await
        });
        // Yield so the spawned task reaches the semaphore await.
        tokio::task::yield_now().await;

        // Abort cancels the future while it is pending at acquire_owned();
        // spawn_blocking is never entered, so no selection is committed.
        handle.abort();
        let _ = handle.await;

        drop(permit);

        assert!(
            !surface
                .startup
                .composition
                .adapter
                .arbiter
                .projection()
                .unwrap()
                .database_selected(),
            "cancelled selection must not commit durable state"
        );
    }

    /// A `tokio::time::timeout` around a selection that is blocked at the
    /// mutation semaphore must fire correctly because the selection future is
    /// a normal async future that yields at the semaphore, not a blocking call.
    #[tokio::test]
    async fn adapter_selection_timeout_fires_when_mutation_lane_is_full() {
        tokio::time::pause();

        let surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let adapter = Arc::clone(&surface.startup.composition.adapter);
        let catalog = Arc::clone(&surface.startup.composition.catalog);
        let context = Arc::clone(&surface.startup.composition.context);

        // Hold the mutation lane so select() is always blocked at the semaphore.
        let _permit = Arc::clone(&adapter.mutation_lane)
            .acquire_owned()
            .await
            .unwrap();

        let timeout_duration = Duration::from_secs(10);
        let task = tokio::spawn(async move {
            tokio::time::timeout(
                timeout_duration,
                adapter.select(
                    weavelit_module_client::SelectedBackend::Sqlite,
                    catalog,
                    context,
                ),
            )
            .await
        });

        // Let the task reach the semaphore await before advancing time.
        tokio::task::yield_now().await;
        tokio::time::advance(timeout_duration + Duration::from_nanos(1)).await;

        let result = task.await.unwrap();
        assert!(
            result.is_err(),
            "timeout must fire before spawn_blocking starts"
        );

        // spawn_blocking was never entered, so no selection was committed.
        assert!(
            !surface
                .startup
                .composition
                .adapter
                .arbiter
                .projection()
                .unwrap()
                .database_selected(),
            "timed-out selection must not commit durable state"
        );
    }

    /// A commit that has entered `spawn_blocking` must run to completion even
    /// when the Tokio task that spawned it is aborted, leaving no partial state.
    #[tokio::test]
    async fn started_blocking_commit_completes_after_outer_task_is_aborted() {
        use std::sync::Barrier;
        use std::sync::atomic::{AtomicBool, Ordering};

        let committed = Arc::new(AtomicBool::new(false));
        let started_gate = Arc::new(Barrier::new(2));
        let finish_gate = Arc::new(Barrier::new(2));
        // Without this third rendezvous the assertion below races the store it
        // is observing, because releasing `finish_gate` orders nothing after it.
        let committed_gate = Arc::new(Barrier::new(2));

        let committed_clone = Arc::clone(&committed);
        let started_gate_clone = Arc::clone(&started_gate);
        let finish_gate_clone = Arc::clone(&finish_gate);
        let committed_gate_clone = Arc::clone(&committed_gate);

        // A SelectionCommit that signals when spawn_blocking has started, then
        // waits for the test to signal completion.  This simulates a durable
        // write that must not be interrupted once it begins.
        let commit: weavelit_module_client::SelectionCommit = Arc::new(move |_backend| {
            let committed = Arc::clone(&committed_clone);
            let started_gate = Arc::clone(&started_gate_clone);
            let finish_gate = Arc::clone(&finish_gate_clone);
            let committed_gate = Arc::clone(&committed_gate_clone);
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    started_gate.wait(); // signal: spawn_blocking has started
                    finish_gate.wait(); // wait for test to say "go"
                    committed.store(true, Ordering::SeqCst);
                    committed_gate.wait(); // signal: the commit has landed
                    Ok(weavelit_server_lifecycle::LifecycleProjection::new(true))
                })
                .await
                .map_err(|_| DatabaseSelectionRejection::ServiceUnavailable)?
            })
        });

        let expected_origin = weavelit_module_client::ExpectedOrigin::from_listener(
            UNBOUND_LISTENER.parse().unwrap(),
        );
        let router = Router::new().route(
            APPLICATION_DATABASE_ROUTE,
            weavelit_module_client::database_selection_route(expected_origin, commit),
        );

        let request = Request::builder()
            .method("PUT")
            .uri(APPLICATION_DATABASE_ROUTE)
            .header("host", UNBOUND_LISTENER)
            .header("origin", format!("https://{UNBOUND_LISTENER}"))
            .header("x-weavelit-csrf", "1")
            .header("content-type", "application/json")
            .header("content-length", SELECTION_BODY.len().to_string())
            .body(Body::from(SELECTION_BODY))
            .unwrap();

        let task = tokio::spawn(async move { router.oneshot(request).await });

        // Wait for the barrier that signals spawn_blocking has started.
        let gate = Arc::clone(&started_gate);
        tokio::task::spawn_blocking(move || gate.wait())
            .await
            .unwrap();

        // Abort the outer task (simulates REQUEST_PROCESSING_TIMEOUT firing
        // after spawn_blocking has already begun).  The running spawn_blocking
        // task is unaffected.
        task.abort();
        let _ = task.await;

        // Release the blocking work.
        let gate = Arc::clone(&finish_gate);
        tokio::task::spawn_blocking(move || gate.wait())
            .await
            .unwrap();

        // Rendezvous with the commit itself, so the assertion observes it.
        let gate = Arc::clone(&committed_gate);
        tokio::task::spawn_blocking(move || gate.wait())
            .await
            .unwrap();

        assert!(
            committed.load(Ordering::SeqCst),
            "spawn_blocking must complete even after the outer task is aborted"
        );
    }

    // -----------------------------------------------------------------------
    // Restore orchestration
    // -----------------------------------------------------------------------

    /// Reads a backup fixture the Restore validation crate owns and commits.
    ///
    /// The runtime deliberately reuses those fixtures rather than minting its
    /// own, so this orchestration is proven against exactly the artifacts the
    /// validation contract is proven against.
    fn restore_fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../weavelit-server-restore/tests/fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|error| panic!("fixture {name} must exist: {error}"))
    }

    /// Reads a recovery key fixture as submitted text.
    fn recovery_key_fixture(name: &str) -> String {
        String::from_utf8(restore_fixture(name))
            .unwrap()
            .trim()
            .to_owned()
    }

    fn valid_recovery_key() -> String {
        recovery_key_fixture("valid-identity.txt")
    }

    /// The committed backup whose referenced components are exactly the ones
    /// this build compiles in: the Web UI Client Module and the SQLite Log
    /// Module. It is sealed to the same recovery key every valid fixture is.
    const COMPILED_IN_BACKUP: &str = "valid-web-ui-sqlite.wlitbackup";

    /// The correlation identifier the listener would have generated when it
    /// issued this Restore's ticket.
    const RESTORE_CORRELATION: &str = "0123456789abcdef0123456789abcdef";

    /// The component inventory the shared `valid.wlitbackup` fixture references.
    ///
    /// That fixture deliberately names a Service Module and a named Operation
    /// no build in this repository compiles in, so the tests that use it supply
    /// the fuller inventory a deployment offering those components would
    /// report. A Restore judged against what this build actually serves uses
    /// [`server_components`] and the `valid-web-ui-sqlite.wlitbackup` fixture
    /// instead.
    fn fixture_components() -> AvailableComponents {
        fn names(values: &[&str]) -> std::collections::BTreeSet<Name> {
            values
                .iter()
                .map(|value| Name::new(*value).unwrap())
                .collect()
        }

        let totp = weavelit_module_mfa_totp::registration();

        AvailableComponents {
            client_modules: names(&["web-ui"]),
            mfa_modules: [(
                Name::new(totp.identifier()).unwrap(),
                super::MfaFactorFormat {
                    factor_data_bytes: totp.secret_length(),
                },
            )]
            .into_iter()
            .collect(),
            service_modules: names(&["zendesk"]),
            log_modules: [(
                Name::new("sqlite").unwrap(),
                super::LogSettingsFormat::default(),
            )]
            .into_iter()
            .collect(),
            operations: names(&["ticket-search"]),
        }
    }

    /// The shipped binary judges every pre-operational workflow against this
    /// inventory, so it must name each module the build actually compiles in.
    /// A build that compiles in the TOTP MFA Module while reporting no MFA
    /// Module refuses state it can serve, and no test that supplies its own
    /// inventory would notice.
    #[test]
    fn the_runtime_inventory_reports_every_module_this_build_compiles_in() {
        let components = server_components();

        assert!(components.has_client_module(
            &Name::new(weavelit_module_client_webui::MODULE_IDENTIFIER).unwrap()
        ));
        assert!(
            components
                .has_log_module(&Name::new(weavelit_module_log_sqlite::MODULE_IDENTIFIER).unwrap())
        );
        assert!(
            components
                .has_mfa_module(&Name::new(weavelit_module_mfa_totp::MODULE_IDENTIFIER).unwrap())
        );

        assert_eq!(components.client_modules.len(), 1);
        assert_eq!(components.log_modules.len(), 1);
        assert_eq!(components.mfa_modules.len(), 1);
    }

    /// The same inventory must still refuse what this build cannot serve, so
    /// the correction above widened it by exactly the compiled-in MFA Module.
    #[test]
    fn the_runtime_inventory_reports_a_component_this_build_lacks_as_unavailable() {
        let components = server_components();

        assert!(components.service_modules.is_empty());
        assert!(components.operations.is_empty());
        for absent in ["zendesk", "ticket-search", "cli", "mysql", "webauthn"] {
            let name = Name::new(absent).unwrap();
            assert!(!components.has_client_module(&name));
            assert!(!components.has_log_module(&name));
            assert!(!components.has_mfa_module(&name));
            assert!(!components.has_service_module(&name));
            assert!(!components.has_operation(&name));
        }
    }

    /// A Restore composed over a real state root with a selected database.
    struct RestoreSurface {
        /// Retained so the state root outlives the orchestration under test.
        _root: tempfile::TempDir,
        state_root: PathBuf,
        startup: RestrictedStartup,
        switch: Arc<ServingModeSwitch>,
        modes: watch::Receiver<ServingMode>,
    }

    impl RestoreSurface {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let state_root = root.path().canonicalize().unwrap();
            {
                let mut store = LifecycleStore::open_or_create(&state_root).unwrap();
                store
                    .select_database(
                        &sqlite_catalog(),
                        &TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE)),
                        &BackendIdentifier::new("sqlite").unwrap(),
                        Vec::new(),
                    )
                    .unwrap();
            }

            let startup = classify_restricted_startup(&state_root).unwrap();
            assert_eq!(startup.outcome(), StartupOutcome::UninitializedWithDatabase);
            let (switch, modes) =
                published_serving_modes(&startup, UNBOUND_LISTENER.parse().unwrap());

            Self {
                _root: root,
                state_root,
                startup,
                switch,
                modes,
            }
        }

        fn orchestrator(&self) -> Arc<RestoreOrchestrator> {
            RestoreOrchestrator::new(
                &self.startup,
                fixture_components(),
                Arc::clone(&self.switch),
                operational_runtime(&self.startup, UNBOUND_LISTENER.parse().unwrap()),
            )
        }

        /// An orchestration judged against the Server's own compiled-in
        /// inventory, exactly as the composed listener judges one.
        fn compiled_in_orchestrator(&self) -> Arc<RestoreOrchestrator> {
            RestoreOrchestrator::new(
                &self.startup,
                server_components(),
                Arc::clone(&self.switch),
                operational_runtime(&self.startup, UNBOUND_LISTENER.parse().unwrap()),
            )
        }

        /// Acquires the same Restore admission permit the listener holds before
        /// an artifact is allocated, so an orchestration driven directly still
        /// runs under the route's single-permit concurrency bound.
        async fn admission(&self) -> BodyAdmission {
            BodyAdmission::from_permit(
                Arc::clone(&self.startup.composition.adapter.mutation_lane)
                    .acquire_owned()
                    .await
                    .unwrap(),
            )
        }

        /// Runs one Restore exactly as an admitted artifact upload would.
        ///
        /// The listener holds the admission permit, started the request budget
        /// before the recovery key was read, and generated the correlation
        /// identifier when it issued the ticket. All three are supplied here so
        /// the orchestration inherits them instead of minting its own.
        async fn restore(
            &self,
            orchestrator: &Arc<RestoreOrchestrator>,
            artifact: Vec<u8>,
            recovery_key: String,
        ) -> Result<InitializedState, RestoreError> {
            orchestrator
                .restore(
                    self.admission().await,
                    RequestBudget::start(),
                    RESTORE_CORRELATION.to_owned(),
                    artifact,
                    Zeroizing::new(recovery_key),
                )
                .await
        }

        /// Runs one Restore against a supplied deadline.
        ///
        /// The admission permit is held for the whole chain exactly as the
        /// listener holds it, and the chain itself is the blocking one the
        /// public entry point spawns, so only the budget's origin differs.
        async fn restore_against_deadline(
            &self,
            orchestrator: &Arc<RestoreOrchestrator>,
            deadline: &dyn RequestDeadline,
            artifact: Vec<u8>,
            recovery_key: String,
        ) -> Result<InitializedState, RestoreError> {
            let _admission = self.admission().await;
            orchestrator.run_against_deadline(
                deadline,
                RESTORE_CORRELATION,
                Zeroizing::new(artifact),
                Zeroizing::new(recovery_key),
            )
        }

        /// Snapshots the router the next accepted connection would serve.
        fn served_router(&self) -> Router {
            self.modes.borrow().router().clone()
        }

        /// Mounts the two Restore routes with exactly the registrations the
        /// pre-operational composer mounts them with.
        ///
        /// The surface is a snapshot, so a test can keep serving it after the
        /// deployment has moved on, which is what a connection accepted before
        /// a checkpoint existed holds.
        fn restore_routes(&self, orchestrator: &Arc<RestoreOrchestrator>) -> MountedSurface {
            let expected_origin = ExpectedOrigin::from_listener(
                UNBOUND_LISTENER
                    .parse()
                    .expect("the listener authority parses"),
            );
            let declaration = RestoreDeclaration::new(orchestrator.capability(expected_origin));
            let key_route = declaration.key_route();
            let artifact_route = declaration.artifact_route();
            MountedSurface::without_registrations(fallback_router())
                .with_capability(TransportCapability::new(
                    orchestrator.key_registration(expected_origin),
                    move |router| router.route(RESTORE_ROUTE, key_route),
                ))
                .with_capability(TransportCapability::new(
                    orchestrator.artifact_registration(expected_origin),
                    move |router| router.route(RESTORE_ARTIFACT_ROUTE, artifact_route),
                ))
        }

        fn anchor_snapshot(&self) -> Vec<(OsString, Vec<u8>, i64, i64)> {
            anchor_snapshot(&self.state_root)
        }

        /// Releases the process-lifetime state-root lock so startup can re-run.
        fn release(self) -> (tempfile::TempDir, PathBuf) {
            let Self {
                _root,
                state_root,
                startup,
                ..
            } = self;
            drop(startup);
            (_root, state_root)
        }
    }

    /// Asserts a router serves the pre-operational Client Module surface.
    async fn assert_preoperational(router: Router) {
        let response = router
            .oneshot(Request::get(STATUS_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Asserts a router serves no functional route at all.
    async fn assert_fail_closed(router: Router) {
        for target in ["/", STATUS_ROUTE, APPLICATION_DATABASE_ROUTE] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    /// Asserts neither Restore route is mounted on `router`.
    async fn assert_restore_unmounted(router: Router) {
        for target in [RESTORE_ROUTE, RESTORE_ARTIFACT_ROUTE] {
            let response = router
                .clone()
                .oneshot(
                    Request::put(target)
                        .header("host", UNBOUND_LISTENER)
                        .header("origin", format!("https://{UNBOUND_LISTENER}"))
                        .header("x-weavelit-csrf", "1")
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    /// A well-formed ticket this Server never issued.
    const UNISSUED_TICKET: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";

    /// Builds the exact accepted recovery-key submission body.
    fn restore_key_body(recovery_key: &str) -> Vec<u8> {
        format!("{{\"recovery_key\":\"{recovery_key}\"}}").into_bytes()
    }

    /// Reads the ticket out of the one response that may ever carry it.
    fn issued_restore_ticket(body: &[u8]) -> String {
        const FIELD: &str = "\"restore_ticket\":\"";

        let rendered = std::str::from_utf8(body).expect("a typed envelope is UTF-8");
        let start = rendered
            .find(FIELD)
            .expect("an accepted submission renders its ticket")
            + FIELD.len();
        let rest = &rendered[start..];
        rest[..rest
            .find('"')
            .expect("the rendered ticket value is terminated")]
            .to_owned()
    }

    /// Drives one Restore request through the listener's production path.
    ///
    /// The head, the body, and every precondition header are written over a
    /// real stream, so the request passes through head reading, classification
    /// against the mounted registrations, framing, the registered pre-body
    /// check, admission, and the body allocation in that order.
    async fn restore_request(
        surface: MountedSurface,
        target: &str,
        media_type: &str,
        ticket: Option<&str>,
        body: Vec<u8>,
    ) -> BoundedResponse {
        let mut head = format!(
            "PUT {target} HTTP/1.1\r\n\
             Host: {UNBOUND_LISTENER}\r\n\
             Origin: https://{UNBOUND_LISTENER}\r\n\
             X-Weavelit-Csrf: 1\r\n\
             Content-Type: {media_type}\r\n\
             Content-Length: {}\r\n",
            body.len()
        );
        if let Some(ticket) = ticket {
            head.push_str(&format!("{RESTORE_TICKET_HEADER_NAME}: {ticket}\r\n"));
        }
        head.push_str("\r\n");

        process_over_duplex(
            surface,
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: REQUEST_READ_TIMEOUT,
                processing: REQUEST_PROCESSING_TIMEOUT,
            },
            move |stream| {
                tokio::spawn(async move {
                    let mut stream = stream;
                    stream.write_all(head.as_bytes()).await.unwrap();
                    stream.write_all(&body).await.unwrap();
                    pending::<()>().await;
                })
            },
        )
        .await
    }

    /// Submits the recovery key and returns the ticket that was issued.
    async fn submit_recovery_key(surface: MountedSurface, recovery_key: &str) -> String {
        let issued = restore_request(
            surface,
            RESTORE_ROUTE,
            "application/json",
            None,
            restore_key_body(recovery_key),
        )
        .await;
        assert_eq!(issued.status, StatusCode::ACCEPTED);
        issued_restore_ticket(&issued.body)
    }

    #[tokio::test]
    async fn a_restore_activates_normal_operation_without_a_restart() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        assert_preoperational(surface.served_router()).await;

        let state = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect("the committed valid backup must restore");

        assert!(state.completion_acknowledged());
        assert_eq!(
            state
                .state()
                .accounts()
                .iter()
                .map(|account| account.username.as_str())
                .collect::<Vec<_>>(),
            vec!["administrator"]
        );
        assert_eq!(
            state.state().completion_obligation().workflow(),
            WorkflowKind::Restore
        );

        // A newly accepted connection serves the operational surface, and every
        // pre-operational route is gone rather than mounted and denied.
        let router = surface.served_router();
        let asset = router
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        for target in [STATUS_ROUTE, APPLICATION_DATABASE_ROUTE] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
            assert_eq!(
                response_body(response).await,
                "{\"error\":\"not_found\"}",
                "{target}"
            );
        }
    }

    #[tokio::test]
    async fn a_restore_hands_its_open_application_database_to_the_operational_runtime() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        assert!(
            orchestrator.operational_database().is_none(),
            "no operational database exists before a Restore completes"
        );

        let state = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect("the committed valid backup must restore");
        let deployment_identifier = state.deployment_identifier();

        // The database the workflow committed through is retained and handed
        // over, so the operational runtime continues on that same handle.
        assert_eq!(
            orchestrator
                .operational_database()
                .expect("a completed Restore must hand over its open Application Database")
                .with(|database| database.inspect(deployment_identifier))
                .expect("the handed-over database lane must be usable")
                .unwrap(),
            DatabaseInspection::Initialized {
                deployment_identifier
            }
        );
    }

    /// Both routes into normal operation compose through the one operational
    /// composer, so a restored deployment serves exactly what a sealed startup
    /// serves instead of drifting from it.
    #[tokio::test]
    async fn both_operational_publication_paths_serve_the_same_composed_surface() {
        let restored = RestoreSurface::new();
        let orchestrator = restored.orchestrator();
        restored
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect("the committed valid backup must restore");
        let from_restore = restored.served_router();

        let sealed = Surface::new(StartupOutcome::Initialized);
        let from_startup = sealed.routes();

        // Anchor the comparison: an operational surface serves the Web UI and
        // has shed every pre-operational route, so the equality below is not
        // two identically empty surfaces.
        let asset = from_restore
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        let removed = from_startup
            .clone()
            .oneshot(Request::get(STATUS_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NOT_FOUND);

        for target in [
            "/",
            "/assets/weavelit-application.js",
            "/assets/weavelit-application.css",
            STATUS_ROUTE,
            APPLICATION_DATABASE_ROUTE,
            RESTORE_ROUTE,
            RESTORE_ARTIFACT_ROUTE,
            AUTH_LOGIN_ROUTE,
            AUTH_SESSION_ROUTE,
            AUTH_LOGOUT_ROUTE,
            "/unknown",
        ] {
            let restored_response = from_restore
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let started_response = from_startup
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                restored_response.status(),
                started_response.status(),
                "{target}"
            );
            assert_eq!(
                restored_response.headers(),
                started_response.headers(),
                "{target}"
            );
            assert_eq!(
                response_body(restored_response).await,
                response_body(started_response).await,
                "{target}"
            );
        }

        // Neither path can publish a router that has shed its registrations,
        // because each publishes one mounted surface carrying both. The two
        // paths register the same routes under the same methods, so a restored
        // deployment admits bodies exactly as a sealed startup does.
        let restored_registry = restored
            .modes
            .borrow()
            .surface()
            .registry()
            .registered_routes();
        assert_eq!(restored_registry, operational_registrations());
        assert_eq!(
            restored_registry,
            operational_mount(&sealed.startup)
                .surface()
                .registry()
                .registered_routes()
        );
    }

    #[tokio::test]
    async fn a_restore_acknowledges_completion_through_the_restored_log_configuration() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        let state = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .unwrap();
        let obligation = state.state().completion_obligation().clone();

        // The fixture assigns the System Log to the SQLite Log Module, so the
        // acknowledgement must be durable in that module's local storage.
        let log_database = surface.state_root.join("log.sqlite3");
        assert!(log_database.exists(), "the assigned destination must exist");

        let connection = rusqlite::Connection::open(&log_database).unwrap();
        let (record_id, classification, correlation): (Vec<u8>, String, String) = connection
            .query_row(
                "SELECT record_id, classification, correlation_id \
                 FROM weavelit_log_system_records",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("exactly one System Log record must be delivered");

        assert_eq!(record_id, obligation.record_identifier().as_bytes());
        assert_eq!(classification, obligation.classification().as_str());
        assert_eq!(correlation, obligation.correlation_identifier().as_str());
    }

    #[tokio::test]
    async fn a_restored_deployment_is_durable_across_startup_classification() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        let restored = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .unwrap();
        drop(orchestrator);

        let (_root, state_root) = surface.release();
        let reloaded = classify_restricted_startup(&state_root).unwrap();

        assert_eq!(reloaded.outcome(), StartupOutcome::Initialized);
        let loaded = reloaded.initialized_state().unwrap();
        assert_eq!(
            loaded.deployment_identifier(),
            restored.deployment_identifier()
        );
        assert!(loaded.completion_acknowledged());
        assert_eq!(loaded.state(), restored.state());
    }

    #[tokio::test]
    async fn a_wrong_recovery_key_fails_before_the_checkpoint() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let anchors = surface.anchor_snapshot();

        let error = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                recovery_key_fixture("wrong-identity.txt"),
            )
            .await
            .unwrap_err();

        // A syntactically valid key that does not open the envelope is
        // attributed to the backup, not to the submitted key.
        assert_eq!(error, RestoreError::BackupInvalid);
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Uninitialized
        );
        assert_preoperational(surface.served_router()).await;
    }

    #[tokio::test]
    async fn a_malformed_artifact_fails_before_the_checkpoint() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let anchors = surface.anchor_snapshot();

        let error = surface
            .restore(
                &orchestrator,
                restore_fixture("bad-magic.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .unwrap_err();

        assert_eq!(error, RestoreError::BackupInvalid);
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Uninitialized
        );
        assert_preoperational(surface.served_router()).await;
    }

    /// A backup is restorable only into a Server that can serve everything it
    /// names. `valid.wlitbackup` holds a `zendesk` Service Connection and a
    /// `ticket-search` Operation grant, and this build compiles in neither, so
    /// the real inventory must refuse it rather than restore a deployment whose
    /// Groups and connections point at components that would never load.
    #[tokio::test]
    async fn a_backup_naming_components_this_build_lacks_is_incompatible() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.compiled_in_orchestrator();
        let anchors = surface.anchor_snapshot();

        let error = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .unwrap_err();

        assert_eq!(error, RestoreError::BackupIncompatible);
        assert_eq!(
            error.category_reason(),
            ("backup_incompatible", "backup_incompatible")
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Uninitialized
        );
        assert_preoperational(surface.served_router()).await;

        // The same deployment restores the backup that names only what this
        // build compiles in, so the refusal above is the component check and
        // not a Restore that could never have succeeded here.
        let state = surface
            .restore(
                &orchestrator,
                restore_fixture(COMPILED_IN_BACKUP),
                valid_recovery_key(),
            )
            .await
            .expect("the compiled-in backup must restore");
        assert!(state.completion_acknowledged());
    }

    #[tokio::test]
    async fn a_failure_after_the_checkpoint_leaves_the_server_fail_closed() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        // The assigned Log Module cannot open its local storage, which fails
        // only after the checkpoint has already replaced retained state. The
        // file name is one the state root inventory already recognizes, so the
        // anchor set itself stays valid across the restart below.
        std::fs::write(surface.state_root.join("log.sqlite3"), b"not a database").unwrap();

        let error = surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .unwrap_err();

        assert_eq!(error, RestoreError::RestoreFailed);
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::InitializationPending
        );
        assert_fail_closed(surface.served_router()).await;

        // The interrupted deployment stays fail-closed across a restart too,
        // because no automatic rollback is attempted.
        drop(orchestrator);
        let (_root, state_root) = surface.release();
        assert_eq!(
            classify_restricted_startup(&state_root).unwrap_err(),
            StartupError::LifecycleInterruptedRedeployRequired
        );
    }

    /// A request budget that is exhausted after a chosen number of reads.
    ///
    /// Both answers come from real [`RequestBudget`] values, so an overrun is
    /// the rejection the approved total deadline produces rather than one this
    /// test invented, and it is placed at an exact step of the chain without
    /// waiting for real time to pass.
    struct ExhaustedAfter {
        reads: AtomicUsize,
        live_reads: usize,
        live: RequestBudget,
        exhausted: RequestBudget,
    }

    impl ExhaustedAfter {
        fn reads(live_reads: usize) -> Self {
            Self {
                reads: AtomicUsize::new(0),
                live_reads,
                live: RequestBudget::start(),
                exhausted: RequestBudget::already_exhausted(),
            }
        }

        /// Returns how many times the chain observed the budget.
        fn observed(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    impl RequestDeadline for ExhaustedAfter {
        fn check(&self) -> Result<(), RestoreError> {
            if self.reads.fetch_add(1, Ordering::SeqCst) < self.live_reads {
                return self.live.check();
            }
            self.exhausted.check()
        }
    }

    /// The reads validation makes before it returns a normalized backup.
    const READS_THROUGH_VALIDATION: usize = 4;

    /// A request budget that closes the transition gate at a chosen read.
    ///
    /// The chain consults its deadline immediately before it enters the gate,
    /// so closing the gate from inside that read places the stop at exactly the
    /// last moment at which abandoning the Restore is still free. Every answer
    /// the deadline itself gives comes from a real live [`RequestBudget`], so
    /// nothing here stands in for the deadline's own decision.
    struct StoppingAt {
        reads: AtomicUsize,
        stopping_read: usize,
        live: RequestBudget,
        gate: Arc<LifecycleTransitionGate>,
    }

    impl StoppingAt {
        fn read(stopping_read: usize, gate: Arc<LifecycleTransitionGate>) -> Self {
            Self {
                reads: AtomicUsize::new(0),
                stopping_read,
                live: RequestBudget::start(),
                gate,
            }
        }
    }

    impl RequestDeadline for StoppingAt {
        fn check(&self) -> Result<(), RestoreError> {
            if self.reads.fetch_add(1, Ordering::SeqCst) == self.stopping_read {
                self.gate.begin_stopping();
            }
            self.live.check()
        }
    }

    /// A stop signalled before a Restore enters its irreversible region is
    /// refused there, and the deployment is left exactly as an untouched one.
    ///
    /// The refusal is not a new outcome. It renders the same `restore_failed`
    /// an expired budget renders at every earlier step, so no step and no cause
    /// is distinguishable to a submitter.
    #[tokio::test]
    async fn a_restore_refused_at_a_closed_transition_gate_leaves_the_deployment_untouched() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let anchors = surface.anchor_snapshot();
        let deadline = StoppingAt::read(
            READS_THROUGH_VALIDATION,
            Arc::clone(&surface.startup.composition.adapter.transition_gate),
        );

        let error = surface
            .restore_against_deadline(
                &orchestrator,
                &deadline,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect_err("a Restore refused at the transition gate must not be committed");

        // Byte for byte what an overrun at this same step already answered.
        assert_eq!(error, RestoreError::RestoreFailed);
        assert_eq!(
            error.category_reason(),
            ("restore_failed", "restore_failed")
        );

        // The deployment is untouched: the fail-closed surface was never
        // published, no checkpoint was created, and no durable state changed.
        assert_preoperational(surface.served_router()).await;
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Uninitialized
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
    }

    /// A Restore that entered its region before the stop still seals, and the
    /// wait a shutdown performs returns only once that region is empty.
    ///
    /// The deployment state is read at the instant the wait returns, so a
    /// shutdown reaching its database close before the replacement sealed would
    /// be visible here as an unsealed record.
    #[tokio::test]
    async fn a_restore_inside_the_transition_gate_seals_before_the_wait_returns() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let gate = Arc::clone(&surface.startup.composition.adapter.transition_gate);
        let entered = Arc::new(Notify::new());

        // The stop is taken from inside the region itself, after the gate
        // admitted this Restore and before any durable state is replaced. An
        // occupied permit is not that signal: it is taken before entry is
        // finalized, so a stop placed there would cancel the entry it meant to
        // land behind.
        let stopping = Arc::clone(&gate);
        let arrived = Arc::clone(&entered);
        orchestrator.pause_replacement_with(Arc::new(move || {
            stopping.begin_stopping();
            arrived.notify_one();
        }));

        let (restored, sealed_when_released) = tokio::join!(
            surface.restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            ),
            async {
                // Ordered behind the entry above, so the wait can only be
                // satisfied by the Restore leaving the region rather than by
                // taking the permit ahead of it.
                entered.notified().await;
                let quiesced = gate.quiesce(SHUTDOWN_LIFECYCLE_TRANSITION_THRESHOLD).await;
                assert!(!quiesced.overrun, "the region must empty inside its budget");
                surface.startup.composition.adapter.arbiter.record_state()
            },
        );

        assert!(restored.is_ok(), "{restored:?}");
        assert_eq!(sealed_when_released, LifecycleState::Initialized);
    }

    /// A stop signalled while a Restore stands between its sealed record and
    /// the database it hands over waits through its threshold, then closes the
    /// registered database rather than an empty slot.
    ///
    /// Closing an owner nothing has registered yet succeeds while closing
    /// nothing, so a wait that returned inside this window would let the
    /// process exit with the replacement's own SQLite handle still open. The
    /// retained write-ahead log that leaves behind is what the next startup
    /// classifies as a deployment needing redeployment, so the absence of one
    /// is what proves the close reached the real handle.
    ///
    /// The replacement is parked exactly inside the window, so the stop lands
    /// there by construction rather than by an interleaving this test hopes
    /// for.
    #[tokio::test]
    async fn a_shutdown_inside_a_restores_activation_closes_the_database_it_committed_through() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let gate = Arc::clone(&surface.startup.composition.adapter.transition_gate);
        let activating =
            test_support::ActivationBarrier::install(surface.startup.active_database());
        let DrainingListener {
            shutdown, serving, ..
        } = draining_listener_with_transition_gate(
            fallback_router(),
            ShutdownBudget {
                drain: Duration::ZERO,
                transition_threshold: Duration::ZERO,
                ..ShutdownBudget::DEFAULT
            },
            surface.startup.active_database().clone(),
            gate,
        )
        .await;

        let (restored, shutdown_result) = tokio::join!(
            surface.restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            ),
            async {
                let mut activating = activating;
                activating.reached().await;

                // The record is sealed and the database it committed through is
                // not registered yet. A zero threshold must not let the
                // listener close this empty owner or return before activation.
                shutdown.send(()).unwrap();
                assert!(!serving.is_finished());
                activating.release();
                serving.await.unwrap()
            },
        );

        assert!(restored.is_ok(), "{restored:?}");
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Initialized
        );
        assert_eq!(shutdown_result, Err(StartupError::ShutdownIncomplete));
        // The shared owner takes an operational handle exactly once. Its
        // focused unit test counts duplicate closes; this real Restore proves
        // the listener reaches that owner only after activation.
        assert_no_write_ahead_log(&surface.state_root);
    }

    /// A deadline crossed while state is rebuilt stops before any mutation.
    ///
    /// Rebuilding the replacement state reseals every protected value a backup
    /// carries, which for one at the collection limits is substantial work
    /// running after validation's last read and inside the same uncancellable
    /// chain that replaces the deployment. Without a read between that work and
    /// the point of no return, a request the listener already answered as timed
    /// out would go on to publish the fail-closed surface and replace durable
    /// state.
    #[tokio::test]
    async fn a_deadline_crossed_while_rebuilding_state_stops_before_the_point_of_no_return() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let anchors = surface.anchor_snapshot();
        let deadline = ExhaustedAfter::reads(READS_THROUGH_VALIDATION);

        let error = surface
            .restore_against_deadline(
                &orchestrator,
                &deadline,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect_err("a Restore that overran its deadline must not be committed");

        // Validation itself succeeded, so the refusal is the read taken after
        // the replacement state was rebuilt rather than one validation makes.
        assert_eq!(
            deadline.observed(),
            READS_THROUGH_VALIDATION + 1,
            "the budget must be observed once more after state rebuilding"
        );
        // The overrun answers exactly what an expired budget already answered
        // at every earlier step, so no step is distinguishable to an operator.
        assert_eq!(error, RestoreError::RestoreFailed);
        assert_eq!(
            error.category_reason(),
            ("restore_failed", "restore_failed")
        );

        // The deployment is untouched: the fail-closed surface was never
        // published, no checkpoint was created, and no durable state changed.
        assert_preoperational(surface.served_router()).await;
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Uninitialized
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
    }

    #[tokio::test]
    async fn restore_failures_render_no_recovery_material_or_backup_content() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        let key = valid_recovery_key();
        let secrets = [
            key.as_str(),
            "administrator",
            "Site Administrator",
            "Administrators",
            "totp-seed",
            "provider-token",
            "at-rest-value",
        ];

        let failures = [
            surface
                .restore(
                    &orchestrator,
                    restore_fixture("valid.wlitbackup"),
                    recovery_key_fixture("wrong-identity.txt"),
                )
                .await
                .unwrap_err(),
            surface
                .restore(
                    &orchestrator,
                    restore_fixture("bad-magic.wlitbackup"),
                    key.clone(),
                )
                .await
                .unwrap_err(),
            surface
                .restore(
                    &orchestrator,
                    restore_fixture("tampered-tag.wlitbackup"),
                    key.clone(),
                )
                .await
                .unwrap_err(),
            surface
                .restore(
                    &orchestrator,
                    restore_fixture("wrong-source-backend.wlitbackup"),
                    key.clone(),
                )
                .await
                .unwrap_err(),
        ];

        for error in failures {
            let (category, reason) = error.category_reason();
            let rendered = format!("{error} {error:?} {category} {reason}");
            for secret in secrets {
                assert!(
                    !rendered.contains(secret),
                    "rendered failure must not disclose {secret}: {rendered}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The two-request Restore submission protocol at the listener boundary
    // -----------------------------------------------------------------------

    /// The whole protocol, driven exactly as a client drives it: the recovery
    /// key alone, then the artifact against the ticket that submission issued.
    ///
    /// This is the one orchestration judged against [`server_components`]
    /// rather than a supplied inventory, so it proves the composed listener can
    /// actually restore a committed backup instead of proving only that some
    /// inventory would accept one.
    #[tokio::test]
    async fn the_two_request_restore_protocol_activates_normal_operation() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.compiled_in_orchestrator();
        let mounted = surface.restore_routes(&orchestrator);

        assert_preoperational(surface.served_router()).await;

        let ticket = submit_recovery_key(mounted.clone(), &valid_recovery_key()).await;

        let completed = restore_request(
            mounted,
            RESTORE_ARTIFACT_ROUTE,
            "application/octet-stream",
            Some(&ticket),
            restore_fixture(COMPILED_IN_BACKUP),
        )
        .await;
        assert_eq!(completed.status, StatusCode::OK);
        let rendered = String::from_utf8(completed.body.to_vec()).unwrap();
        assert!(
            rendered
                .starts_with("{\"result\":{\"lifecycle\":\"initialized\"},\"correlation_id\":\"")
                && rendered.ends_with("\"}"),
            "{rendered}"
        );

        // The second request drove activation in-process: a newly accepted
        // connection serves the operational surface and neither Restore route
        // nor any other pre-operational route is mounted any more.
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Initialized
        );
        let router = surface.served_router();
        let asset = router
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert_restore_unmounted(router.clone()).await;
        for target in [STATUS_ROUTE, APPLICATION_DATABASE_ROUTE] {
            let response = router
                .clone()
                .oneshot(Request::get(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{target}");
        }
    }

    /// Route absence cannot close this gap. The listener snapshots the whole
    /// surface when it accepts a connection, so a connection accepted while a
    /// Restore was still eligible keeps a router that mounts both routes even
    /// after a checkpoint exists. Only the request-time eligibility re-check
    /// rejects it.
    #[tokio::test]
    async fn a_stale_router_still_mounting_restore_is_rejected_at_request_time() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let stale = surface.restore_routes(&orchestrator);

        // The identical request on the identical surface is admitted while the
        // deployment still permits a Restore, and it leaves a live ticket the
        // artifact request below can present.
        let issued = submit_recovery_key(stale.clone(), &valid_recovery_key()).await;

        surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect("the committed valid backup must restore");

        // The surface is unchanged and still mounts both routes, and the
        // artifact request presents a ticket this Server issued itself, so a
        // rejection here is the lifecycle re-check and nothing else.
        for (target, media_type, ticket, body) in [
            (
                RESTORE_ROUTE,
                "application/json",
                None,
                restore_key_body(&valid_recovery_key()),
            ),
            (
                RESTORE_ARTIFACT_ROUTE,
                "application/octet-stream",
                Some(issued.as_str()),
                restore_fixture("valid.wlitbackup"),
            ),
        ] {
            let rejected = restore_request(stale.clone(), target, media_type, ticket, body).await;
            assert_ne!(rejected.status, StatusCode::NOT_FOUND, "{target}");
            assert_eq!(rejected.status, StatusCode::CONFLICT, "{target}");
            assert_eq!(
                rejected.body.as_ref(),
                b"{\"error\":\"restore_not_allowed\"}",
                "{target}"
            );
        }
    }

    /// Restore is mounted only where it is eligible, so an unselected
    /// deployment serves no Restore route at all.
    #[tokio::test]
    async fn restore_is_not_mounted_before_a_database_is_selected() {
        let unselected = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let (_switch, modes) =
            published_serving_modes(&unselected.startup, UNBOUND_LISTENER.parse().unwrap());
        assert!(modes.borrow().surface().registry().is_empty());
        assert_restore_unmounted(modes.borrow().router().clone()).await;

        // Selecting a database is what mounts them, so the absence above is
        // the eligibility gate and not an unreachable route.
        let selected = Surface::new(StartupOutcome::UninitializedWithDatabase);
        let (_switch, modes) =
            published_serving_modes(&selected.startup, UNBOUND_LISTENER.parse().unwrap());
        let router = modes.borrow().router().clone();
        for target in [RESTORE_ROUTE, RESTORE_ARTIFACT_ROUTE] {
            let response = router
                .clone()
                .oneshot(Request::put(target).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{target}");
        }
    }

    /// Neither mode a Restore moves the listener into may serve a Restore.
    #[tokio::test]
    async fn restore_is_absent_from_the_fail_closed_and_operational_surfaces() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();

        // Mounted while the deployment is still eligible.
        let mounted = surface.served_router();
        let response = mounted
            .oneshot(Request::put(RESTORE_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::NOT_FOUND);

        surface.switch.publish_fail_closed();
        assert!(surface.modes.borrow().surface().registry().is_empty());
        assert_restore_unmounted(surface.served_router()).await;

        surface
            .restore(
                &orchestrator,
                restore_fixture("valid.wlitbackup"),
                valid_recovery_key(),
            )
            .await
            .expect("the committed valid backup must restore");

        // The operational surface registers only its authentication routes, so
        // no Restore registration survives the transition either.
        assert_eq!(
            surface
                .modes
                .borrow()
                .surface()
                .registry()
                .registered_routes(),
            operational_registrations()
        );
        assert_restore_unmounted(surface.served_router()).await;
    }

    /// Every ticket rejection the routes render is payload free, and a failed
    /// claim destroys the outstanding submission rather than leaving it
    /// available for another attempt.
    #[tokio::test]
    async fn no_restore_route_rejection_discloses_its_ticket_or_recovery_key() {
        let surface = RestoreSurface::new();
        let orchestrator = surface.orchestrator();
        let mounted = surface.restore_routes(&orchestrator);

        let key = valid_recovery_key();
        let ticket = submit_recovery_key(mounted.clone(), &key).await;

        // A wrong ticket destroys the outstanding submission, so the ticket
        // this Server itself issued is no longer claimable afterwards.
        let wrong = restore_request(
            mounted.clone(),
            RESTORE_ARTIFACT_ROUTE,
            "application/octet-stream",
            Some(UNISSUED_TICKET),
            restore_fixture("valid.wlitbackup"),
        )
        .await;
        let replayed = restore_request(
            mounted,
            RESTORE_ARTIFACT_ROUTE,
            "application/octet-stream",
            Some(&ticket),
            restore_fixture("valid.wlitbackup"),
        )
        .await;
        for rejection in [&wrong, &replayed] {
            assert_eq!(rejection.status, StatusCode::FORBIDDEN);
            assert_eq!(
                rejection.body.as_ref(),
                b"{\"error\":\"restore_ticket_invalid\"}"
            );
        }

        // The deployment never moved, so the destroyed submission really did
        // stop the Restore rather than merely delaying it.
        assert_eq!(
            surface.startup.composition.adapter.arbiter.record_state(),
            LifecycleState::Uninitialized
        );
        assert_preoperational(surface.served_router()).await;

        let secrets = [
            key.as_str(),
            ticket.as_str(),
            UNISSUED_TICKET,
            "administrator",
            "Site Administrator",
            "Administrators",
            "totp-seed",
            "provider-token",
            "at-rest-value",
        ];
        for rejection in [&wrong, &replayed] {
            let rendered = String::from_utf8_lossy(&rejection.body).into_owned();
            for secret in secrets {
                assert!(
                    !rendered.contains(secret),
                    "a rendered rejection must not disclose {secret}: {rendered}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Route-registered transport profiles at the listener boundary
    // -----------------------------------------------------------------------

    /// Test-only target for an admitted large-body registration.
    const ADMITTED_TARGET: &str = "/api/v1/admitted-large-body";

    /// Body bound a stage-one admitted registration proves it can carry.
    const ADMITTED_BODY_BYTES: usize = 256 * 1024 * 1024;

    /// A registration whose body budget is long enough to outlive the head's.
    fn admitted_capability() -> TransportCapability {
        TransportCapability::new(
            TransportRegistration::new(
                Method::PUT,
                ADMITTED_TARGET,
                TransportProfile::admitted(
                    ADMITTED_BODY_BYTES,
                    Duration::from_secs(120),
                    Duration::from_secs(60),
                ),
            )
            .with_admission(Arc::new(Semaphore::new(1))),
            |router| {
                router.route(
                    ADMITTED_TARGET,
                    axum::routing::put(|| async {
                        typed_json_response(StatusCode::ACCEPTED, admitted_envelope())
                    }),
                )
            },
        )
    }

    /// The admitted test route's typed envelope.
    fn admitted_envelope() -> TypedJsonEnvelope {
        TypedJsonEnvelope::Result {
            result: TypedResult::new()
                .with_field(
                    StableCode::new("accepted").unwrap(),
                    TypedValue::Boolean(true),
                )
                .unwrap(),
            correlation_id: ResponseCorrelation::new("admitted-0123456789").unwrap(),
        }
    }

    /// Drives one request through the production path over an in-memory stream.
    pub(crate) async fn process_over_duplex(
        surface: MountedSurface,
        timeouts: ConnectionTimeouts,
        write: impl FnOnce(tokio::io::DuplexStream) -> tokio::task::JoinHandle<()>,
    ) -> BoundedResponse {
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let writer = write(client);
        let response = super::process_restricted_request(
            &mut server,
            "127.0.0.1".parse().unwrap(),
            surface,
            Arc::new(RateLimiter::new()),
            timeouts,
        )
        .await;
        writer.abort();
        response
    }

    fn short_head_budget() -> ConnectionTimeouts {
        ConnectionTimeouts {
            handshake: TLS_HANDSHAKE_TIMEOUT,
            request_read: Duration::from_millis(400),
            processing: REQUEST_PROCESSING_TIMEOUT,
        }
    }

    /// A registered route may read its body past the head's budget, and the
    /// same schedule under the default profile still times out.
    #[tokio::test]
    async fn an_admitted_route_reads_its_body_on_its_own_budget() {
        let head = format!(
            "PUT {ADMITTED_TARGET} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\n"
        );
        let schedule = |stream: tokio::io::DuplexStream| {
            let head = head.clone();
            tokio::spawn(async move {
                let mut stream = stream;
                stream.write_all(head.as_bytes()).await.unwrap();
                // Arrives well after the 400 ms head budget would have expired.
                tokio::time::sleep(Duration::from_millis(900)).await;
                stream.write_all(b"body").await.unwrap();
                std::future::pending::<()>().await;
            })
        };

        let registered = process_over_duplex(
            MountedSurface::without_registrations(fallback_router())
                .with_capability(admitted_capability()),
            short_head_budget(),
            schedule,
        )
        .await;
        assert_eq!(registered.status, StatusCode::ACCEPTED);
        assert_eq!(
            registered.body.as_ref(),
            b"{\"result\":{\"accepted\":true},\"correlation_id\":\"admitted-0123456789\"}"
        );

        // The identical schedule against a surface that mounted no registration
        // stays on the default profile's shared head-and-body budget.
        let unregistered = process_over_duplex(
            MountedSurface::without_registrations(fallback_router()),
            short_head_budget(),
            schedule,
        )
        .await;
        assert_eq!(unregistered.status, StatusCode::REQUEST_TIMEOUT);
    }

    /// Test-only target that reports the processing deadline it was dispatched
    /// with, so the value the router receives can be compared with the one the
    /// listener bounds the request at.
    const DEADLINE_TARGET: &str = "/api/v1/observed-deadline";

    /// The absolute processing deadline one dispatched request carried.
    ///
    /// The router is dispatched with exactly the value
    /// [`processing_response`] is given, so this is that value, not a
    /// reconstruction of it.
    async fn dispatched_deadline(processing: Duration) -> (Deadline, Deadline, Deadline) {
        let observed = Arc::new(std::sync::Mutex::new(Vec::<ProcessingDeadline>::new()));
        let router = fallback_router().route(
            DEADLINE_TARGET,
            any({
                let observed = Arc::clone(&observed);
                move |request: Request<Body>| {
                    let observed = Arc::clone(&observed);
                    async move {
                        let carried = *request
                            .extensions()
                            .get::<ProcessingDeadline>()
                            .expect("the listener attaches the request's processing deadline");
                        observed.lock().unwrap().push(carried);
                        typed_json_response(StatusCode::ACCEPTED, admitted_envelope())
                    }
                }
            }),
        );

        let head = format!("GET {DEADLINE_TARGET} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let before = Deadline::now();
        let response = process_over_duplex(
            MountedSurface::without_registrations(router),
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: REQUEST_READ_TIMEOUT,
                processing,
            },
            |stream| {
                tokio::spawn(async move {
                    let mut stream = stream;
                    stream.write_all(head.as_bytes()).await.unwrap();
                    pending::<()>().await;
                })
            },
        )
        .await;
        let after = Deadline::now();
        assert_eq!(response.status, StatusCode::ACCEPTED);

        let carried = observed.lock().unwrap();
        assert_eq!(carried.len(), 1, "the route was dispatched exactly once");
        (carried[0].instant(), before, after)
    }

    /// The router is dispatched with the same absolute deadline
    /// [`processing_response`] bounds the same request at.
    ///
    /// Work a route hands to a blocking pool outlives the future that timeout
    /// cancels, so the instant the listener stops waiting at has to be
    /// observable from inside the router rather than inferred from a duration.
    /// The observed value is bracketed by the two instants the request was
    /// started between, and it moves with the connection's processing budget
    /// rather than being a constant, so a deadline computed from anything else
    /// fails this.
    #[tokio::test]
    async fn the_router_is_dispatched_with_the_listener_s_own_processing_deadline() {
        for processing in [Duration::from_secs(3), REQUEST_PROCESSING_TIMEOUT] {
            let (carried, before, after) = dispatched_deadline(processing).await;
            assert!(
                carried >= before + processing && carried <= after + processing,
                "the dispatched deadline must be the request's own start plus {processing:?}"
            );
        }
    }

    /// A registration never lengthens the head's own absolute deadline, so a
    /// slow-loris head still dies inside the connection's read budget.
    #[tokio::test]
    async fn a_registration_never_extends_the_head_deadline() {
        let response = process_over_duplex(
            MountedSurface::without_registrations(fallback_router())
                .with_capability(admitted_capability()),
            short_head_budget(),
            |stream| {
                tokio::spawn(async move {
                    let mut stream = stream;
                    // A head that dribbles forever and never terminates.
                    for byte in format!("PUT {ADMITTED_TARGET} HTTP/1.1\r\nHost: localhost\r\n")
                        .into_bytes()
                    {
                        stream.write_all(&[byte]).await.unwrap();
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    std::future::pending::<()>().await;
                })
            },
        )
        .await;
        assert_eq!(response.status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(response.body, request_timeout_response().body);
    }

    /// A body larger than the default bound is refused on a surface that never
    /// mounted the registration granting it.
    #[tokio::test]
    async fn an_unmounted_registration_grants_no_larger_body() {
        let response = process_over_duplex(
            MountedSurface::without_registrations(fallback_router()),
            ConnectionTimeouts {
                handshake: TLS_HANDSHAKE_TIMEOUT,
                request_read: REQUEST_READ_TIMEOUT,
                processing: REQUEST_PROCESSING_TIMEOUT,
            },
            |stream| {
                tokio::spawn(async move {
                    let mut stream = stream;
                    let oversized = MAX_REQUEST_BODY_BYTES + 1;
                    stream
                        .write_all(
                            format!(
                                "PUT {ADMITTED_TARGET} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {oversized}\r\n\r\n"
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    std::future::pending::<()>().await;
                })
            },
        )
        .await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST);
        assert_eq!(response.body.as_ref(), b"{\"error\":\"bad_request\"}");
    }

    /// Each serving mode carries exactly the registrations its own routes need.
    ///
    /// The fail-closed mode mounts nothing and therefore registers nothing. The
    /// operational mode registers only its authentication routes, and the
    /// pre-operational mode registers Init preparation and Restore only where a
    /// selected Application Database makes them eligible.
    #[test]
    fn each_serving_mode_carries_exactly_the_registrations_its_routes_need() {
        let (modes, _receiver) = watch::channel(ServingMode::FailClosed(
            MountedSurface::without_registrations(fallback_router()),
        ));
        let switch = ServingModeSwitch { modes };

        switch.publish_fail_closed();
        assert!(switch.modes.borrow().surface().registry().is_empty());

        let sealed = Surface::new(StartupOutcome::Initialized);
        switch.publish_operational(operational_mount(&sealed.startup));
        assert_eq!(
            switch
                .modes
                .borrow()
                .surface()
                .registry()
                .registered_routes(),
            operational_registrations()
        );

        for (outcome, expected) in [
            (StartupOutcome::UninitializedWithoutDatabase, Vec::new()),
            (StartupOutcome::Initialized, operational_registrations()),
        ] {
            let surface = Surface::new(outcome);
            let (_switch, modes) =
                published_serving_modes(&surface.startup, UNBOUND_LISTENER.parse().unwrap());
            assert_eq!(
                modes.borrow().surface().registry().registered_routes(),
                expected,
                "{outcome:?}"
            );
        }

        // The only pre-operational mode that carries a registration is the one
        // a selected Application Database makes eligible: the two Restore
        // routes and Init's recovery-key preparation. Init finalization is
        // deliberately absent until a key has actually been delivered.
        let with_restore = Surface::new(StartupOutcome::UninitializedWithDatabase);
        let (_switch, modes) =
            published_serving_modes(&with_restore.startup, UNBOUND_LISTENER.parse().unwrap());
        let registered = modes.borrow().surface().registry().registered_routes();
        assert_eq!(
            registered,
            vec![
                (Method::PUT, RESTORE_ROUTE),
                (Method::PUT, RESTORE_ARTIFACT_ROUTE),
                (Method::PUT, INIT_RECOVERY_KEY_ROUTE),
            ]
        );
        // Named explicitly rather than left to the equality above, so that
        // adding a registration to this surface cannot quietly bring
        // finalization with it. Its body profile comes from its registration,
        // so an unregistered finalization route is also an unadmitted one.
        assert!(
            !registered.iter().any(|(_, route)| *route == INIT_ROUTE),
            "finalization must not be registered before a key is delivered"
        );
    }

    // -----------------------------------------------------------------------
    // Response profiles: frozen routes and the typed envelope
    // -----------------------------------------------------------------------

    /// The frozen routes must answer with exactly the bytes they answered with
    /// before route-registered profiles and the typed profile existed.
    #[tokio::test]
    async fn the_frozen_routes_keep_byte_for_byte_identical_responses() {
        let (server_config, client_config) = tls_configs();

        for (outcome, status_body) in [
            (
                StartupOutcome::UninitializedWithoutDatabase,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
            ),
            (
                StartupOutcome::UninitializedWithDatabase,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}",
            ),
        ] {
            let response = direct_tls_response(
                restricted_routes(outcome),
                Arc::clone(&server_config),
                Arc::clone(&client_config),
                b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n",
                REQUEST_READ_TIMEOUT,
                REQUEST_PROCESSING_TIMEOUT,
            )
            .await;
            assert_eq!(
                response,
                format!(
                    "HTTP/1.1 200 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n{status_body}"
                )
                .as_bytes(),
                "{outcome:?}"
            );
        }

        let selection_surface = Surface::new(StartupOutcome::UninitializedWithoutDatabase);
        let selection =
            direct_tls_valid_selection(&selection_surface, &(server_config, client_config)).await;
        assert_eq!(
            selection,
            format!(
                "HTTP/1.1 200 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n{SELECTED_STATUS}"
            )
            .as_bytes()
        );
    }

    /// The typed profile carries its own derived bound; the fixed profile keeps
    /// the bound and allowlist it already had.
    #[test]
    fn the_typed_profile_does_not_reuse_the_fixed_profile_bound() {
        assert_eq!(ResponseProfile::Json.max_body_bytes(), MAX_JSON_BODY_BYTES);
        assert_eq!(ResponseProfile::Json.max_body_bytes(), 128);
        assert_eq!(
            ResponseProfile::TypedJson.max_body_bytes(),
            MAX_TYPED_JSON_BODY_BYTES
        );
        assert_eq!(ResponseProfile::TypedJson.max_body_bytes(), 512);
        assert_eq!(
            ResponseProfile::TypedJson.media_type(),
            ResponseProfile::Json.media_type()
        );
        assert_eq!(ResponseProfile::TypedJson.security_headers(), "");
        // A request-supplied media type can never select the typed profile.
        assert!(matches!(
            ResponseProfile::from_media_type(&HeaderValue::from_static(
                "application/json; charset=utf-8"
            )),
            Some(ResponseProfile::Json)
        ));
    }

    fn typed_envelope() -> TypedJsonEnvelope {
        TypedJsonEnvelope::Result {
            result: TypedResult::new()
                .with_field(
                    StableCode::new("accepted").unwrap(),
                    TypedValue::Boolean(true),
                )
                .unwrap()
                .with_field(
                    StableCode::new("bytes").unwrap(),
                    TypedValue::Unsigned(268_435_456),
                )
                .unwrap(),
            correlation_id: ResponseCorrelation::new("restore-0123456789").unwrap(),
        }
    }

    /// The listener serializes the envelope itself and discards whatever the
    /// route put in its body and headers.
    #[tokio::test]
    async fn the_typed_profile_serializes_only_its_envelope() {
        let mut response = typed_json_response(StatusCode::ACCEPTED, typed_envelope());
        // A route cannot smuggle bytes or headers past the typed profile.
        *response.body_mut() = Body::from("route supplied bytes");
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
        response
            .headers_mut()
            .insert("set-cookie", HeaderValue::from_static("session=1"));

        let bounded = bounded_response_from_axum(response).await;
        assert_eq!(bounded.status, StatusCode::ACCEPTED);
        assert!(matches!(bounded.profile, ResponseProfile::TypedJson));
        assert_eq!(
            bounded.body.as_ref(),
            b"{\"result\":{\"accepted\":true,\"bytes\":268435456},\"correlation_id\":\"restore-0123456789\"}"
        );
        assert_eq!(bounded.allow, None);
    }

    /// The typed profile's wire bytes carry no cookie, no cross-origin header,
    /// no message, no path, no trace, and no dependency detail.
    #[tokio::test]
    async fn the_typed_profile_emits_no_forbidden_header_on_the_wire() {
        let (server_config, client_config) = tls_configs();
        let router = fallback_router().route(
            "/api/v1/typed",
            any(|| async { typed_json_response(StatusCode::ACCEPTED, typed_envelope()) }),
        );

        let response = direct_tls_response(
            router,
            server_config,
            client_config,
            b"GET /api/v1/typed HTTP/1.1\r\nHost: localhost\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_eq!(
            response,
            b"HTTP/1.1 202 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n{\"result\":{\"accepted\":true,\"bytes\":268435456},\"correlation_id\":\"restore-0123456789\"}"
        );

        let rendered = String::from_utf8(response).unwrap();
        for forbidden in [
            "Set-Cookie",
            "set-cookie",
            "Access-Control-",
            "message",
            "trace",
            "/api/v1/typed",
            "sqlite",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "typed response must not disclose {forbidden}: {rendered}"
            );
        }
    }

    /// A typed envelope larger than the derived bound is redacted rather than
    /// truncated, so the derivation is enforced at the boundary.
    #[tokio::test]
    async fn a_typed_envelope_over_the_derived_bound_is_redacted() {
        let bounded =
            bounded_response_from_axum(typed_json_response(StatusCode::OK, typed_envelope())).await;
        assert!(bounded.body.len() <= MAX_TYPED_JSON_BODY_BYTES);

        // The listener re-checks the serialized length against the profile's
        // bound; a shape that outgrew the derivation would take this branch.
        let redacted = redacted_response(StatusCode::OK, None);
        assert_eq!(redacted.body.as_ref(), b"{\"error\":\"gateway_timeout\"}");
    }

    // -----------------------------------------------------------------------
    // Response-write acknowledgement
    // -----------------------------------------------------------------------

    /// Accepts every byte and shuts the connection down cleanly.
    struct AcceptingWriter;

    impl AsyncWrite for AcceptingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buffer.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// Fails the write itself, as a transport whose peer is already gone does.
    struct FailingWriter;

    impl AsyncWrite for FailingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
        }
    }

    /// Takes every byte but never closes cleanly, as a peer that disconnects
    /// after the body was handed to the transport does.
    struct DisconnectingWriter;

    impl AsyncWrite for DisconnectingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buffer.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::ConnectionReset)))
        }
    }

    /// Never makes progress, so the write outlives its budget.
    struct StalledWriter;

    impl AsyncWrite for StalledWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    /// An acknowledgement that counts its runs, and the counter to read.
    fn counting_acknowledgement() -> (ResponseWriteAcknowledgement, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        (
            ResponseWriteAcknowledgement::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            runs,
        )
    }

    fn acknowledging_response(acknowledgement: &ResponseWriteAcknowledgement) -> BoundedResponse {
        BoundedResponse {
            acknowledgement: Some(acknowledgement.clone()),
            ..BoundedResponse::json(StatusCode::OK, "{\"error\":\"not_found\"}")
        }
    }

    /// A write that completed hands the action to the listener exactly once,
    /// however many written responses carry the same acknowledgement.
    #[tokio::test]
    async fn a_written_response_runs_its_post_write_action_exactly_once() {
        let (acknowledgement, runs) = counting_acknowledgement();

        write_response_and_acknowledge(
            &mut AcceptingWriter,
            acknowledging_response(&acknowledgement),
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        write_response_and_acknowledge(
            &mut AcceptingWriter,
            acknowledging_response(&acknowledgement),
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "the action must be taken once, not once per written response"
        );
    }

    /// A failed write leaves the action unrun, so a workflow gated on it stays
    /// fail closed.
    #[tokio::test]
    async fn a_failed_response_write_runs_no_post_write_action() {
        let (acknowledgement, runs) = counting_acknowledgement();

        write_response_and_acknowledge(
            &mut FailingWriter,
            acknowledging_response(&acknowledgement),
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;

        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    /// A peer that disappears before the connection is shut down cleanly is
    /// not a delivery, even though every body byte was accepted.
    #[tokio::test]
    async fn a_disconnected_peer_runs_no_post_write_action() {
        let (acknowledgement, runs) = counting_acknowledgement();

        write_response_and_acknowledge(
            &mut DisconnectingWriter,
            acknowledging_response(&acknowledgement),
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;

        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    /// A write that outlives its budget is abandoned without acknowledging.
    ///
    /// The clock is paused and advanced by the runtime, so this asserts the
    /// budget rather than waiting for it.
    #[tokio::test(start_paused = true)]
    async fn a_response_write_that_exceeds_its_budget_runs_no_post_write_action() {
        let (acknowledgement, runs) = counting_acknowledgement();

        write_response_and_acknowledge(
            &mut StalledWriter,
            acknowledging_response(&acknowledgement),
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;

        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    /// The typed profile carries the route's post-write action to the listener.
    #[tokio::test]
    async fn the_typed_profile_carries_a_route_supplied_post_write_action() {
        let (acknowledgement, runs) = counting_acknowledgement();
        let mut response = typed_json_response(StatusCode::ACCEPTED, typed_envelope());
        response.extensions_mut().insert(acknowledgement);

        let bounded = bounded_response_from_axum(response).await;
        assert!(matches!(bounded.profile, ResponseProfile::TypedJson));
        assert!(bounded.acknowledgement.is_some());
        assert_eq!(runs.load(Ordering::SeqCst), 0);

        write_response_and_acknowledge(&mut AcceptingWriter, bounded, REQUEST_PROCESSING_TIMEOUT)
            .await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    /// Only the typed profile may carry a post-write action. Any other response
    /// carrying one is an invalid composition and redacts rather than
    /// acknowledging a response the listener did not compose.
    #[tokio::test]
    async fn a_non_typed_response_carrying_a_post_write_action_is_redacted() {
        let (acknowledgement, runs) = counting_acknowledgement();
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"not_found\"}"))
            .unwrap();
        response.extensions_mut().insert(acknowledgement);

        let bounded = bounded_response_from_axum(response).await;
        assert_eq!(bounded.body.as_ref(), b"{\"error\":\"gateway_timeout\"}");
        assert!(bounded.acknowledgement.is_none());

        write_response_and_acknowledge(&mut AcceptingWriter, bounded, REQUEST_PROCESSING_TIMEOUT)
            .await;
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    /// The whole listener path runs the action only after the exact response
    /// bytes have left the Server.
    #[tokio::test]
    async fn the_listener_acknowledges_a_route_supplied_action_after_writing_the_response() {
        let (server_config, client_config) = tls_configs();
        let runs = Arc::new(AtomicUsize::new(0));
        let router = fallback_router().route(
            "/api/v1/typed",
            any({
                let runs = Arc::clone(&runs);
                move || {
                    let runs = Arc::clone(&runs);
                    async move {
                        let mut response =
                            typed_json_response(StatusCode::ACCEPTED, typed_envelope());
                        response
                            .extensions_mut()
                            .insert(ResponseWriteAcknowledgement::new(move || {
                                runs.fetch_add(1, Ordering::SeqCst);
                            }));
                        response
                    }
                }
            }),
        );

        let response = direct_tls_response(
            router,
            server_config,
            client_config,
            b"GET /api/v1/typed HTTP/1.1\r\nHost: localhost\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;

        assert_eq!(
            response,
            b"HTTP/1.1 202 \r\nContent-Type: application/json; charset=utf-8\r\n\r\n{\"result\":{\"accepted\":true,\"bytes\":268435456},\"correlation_id\":\"restore-0123456789\"}"
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }
}

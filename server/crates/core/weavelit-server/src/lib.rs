#![forbid(unsafe_code)]

//! Restricted lifecycle startup composition for the Weavelit Server.

use std::{
    collections::HashMap,
    env, fmt, fs,
    io::{ErrorKind, Read},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
    sync::Semaphore,
    task,
    time::timeout,
};
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;
use weavelit_module_client_webui::{
    DatabaseSelectionRejection, ExpectedOrigin, ProjectionSource, SelectedBackend, SelectionCommit,
};
use weavelit_server_database_sqlite::{RetainedSqliteInspection, SqliteDatabase};
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendIdentifier,
    BackendRegistration, DatabaseError, DeploymentIdentifier, InterruptedLifecycleAction,
    LifecycleClassification, LifecycleError, LifecycleProjection, LifecycleStore,
    RetainedDatabaseInspection, TrustedBackendContext, ValidatedConnectionSettings,
    WorkflowArbiter, WorkflowKind,
};
use weavelit_server_log::LogModuleCatalog;

const STATE_ROOT_ENV: &str = "WEAVELIT_STATE_ROOT";
const HTTPS_LISTENER_ADDRESS_ENV: &str = "WEAVELIT_HTTPS_LISTENER_ADDRESS";
const TLS_CERTIFICATE_PATH_ENV: &str = "WEAVELIT_TLS_CERTIFICATE_PATH";
const TLS_PRIVATE_KEY_PATH_ENV: &str = "WEAVELIT_TLS_PRIVATE_KEY_PATH";
const APPLICATION_DATABASE_FILE: &str = "application.sqlite3";
/// The sole pre-operational route that may change lifecycle state.
const APPLICATION_DATABASE_ROUTE: &str = "/api/v1/application-database";
const STATUS_ROUTE: &str = "/api/v1/status";
const MAX_TLS_MATERIAL_BYTES: u64 = 1024 * 1024;
const MAX_NORMAL_CONNECTIONS: usize = 15;
const MAX_REJECTION_CONNECTIONS: usize = 1;
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_PROCESSING_TIMEOUT: Duration = Duration::from_secs(10);
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
    Json,
    Html,
    JavaScript,
    Css,
}

impl ResponseProfile {
    const fn media_type(self) -> &'static str {
        match self {
            Self::Json => "application/json; charset=utf-8",
            Self::Html => "text/html; charset=utf-8",
            Self::JavaScript => "text/javascript; charset=utf-8",
            Self::Css => "text/css; charset=utf-8",
        }
    }

    const fn max_body_bytes(self) -> usize {
        match self {
            Self::Json => MAX_JSON_BODY_BYTES,
            Self::Html => MAX_HTML_BODY_BYTES,
            Self::JavaScript => MAX_JAVASCRIPT_BODY_BYTES,
            Self::Css => MAX_CSS_BODY_BYTES,
        }
    }

    const fn security_headers(self) -> &'static str {
        match self {
            Self::Json => "",
            Self::Html | Self::JavaScript | Self::Css => ASSET_SECURITY_HEADERS,
        }
    }

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

#[derive(Clone)]
struct BoundedResponse {
    status: StatusCode,
    profile: ResponseProfile,
    body: Bytes,
    allow: Option<AllowedMethod>,
}

impl BoundedResponse {
    fn json(status: StatusCode, body: &'static str) -> Self {
        Self {
            status,
            profile: ResponseProfile::Json,
            body: Bytes::from_static(body.as_bytes()),
            allow: None,
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
}

impl LifecycleAdapter {
    fn new(arbiter: Arc<WorkflowArbiter>) -> Self {
        Self {
            arbiter,
            mutation_lane: Arc::new(Semaphore::new(1)),
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
}

/// Restricted startup state, including the process-lifetime state-root lock.
pub struct RestrictedStartup {
    outcome: StartupOutcome,
    log_catalog: LogModuleCatalog,
    composition: PreoperationalComposition,
}

impl RestrictedStartup {
    /// Returns the lifecycle outcome used to select restricted routes.
    pub fn outcome(&self) -> StartupOutcome {
        self.outcome
    }

    /// Returns the compiled-in Log Module catalog retained for process lifetime.
    pub const fn log_catalog(&self) -> &LogModuleCatalog {
        &self.log_catalog
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
        }
    }
}

// ---------------------------------------------------------------------------
// Restricted HTTPS listener and route composition
// ---------------------------------------------------------------------------

/// Binds and serves the sole direct-TLS pre-operational listener.
pub async fn run_restricted_https_listener(
    listener: TrustedHttpsListener,
    startup: RestrictedStartup,
) -> Result<(), StartupError> {
    let tcp_listener = TcpListener::bind(listener.address())
        .await
        .map_err(|_| StartupError::HttpsListenerUnavailable)?;
    let result = serve_restricted_https_listener(
        tcp_listener,
        listener.tls_config(),
        startup.composition.clone(),
    )
    .await;
    drop(startup);
    result
}

async fn serve_restricted_https_listener(
    tcp_listener: TcpListener,
    tls_config: Arc<ServerConfig>,
    composition: PreoperationalComposition,
) -> Result<(), StartupError> {
    // The expected request origin is the address actually bound, never a value
    // taken from a request header or a certificate subject alternative name.
    let bound_address = tcp_listener
        .local_addr()
        .map_err(|_| StartupError::HttpsListenerUnavailable)?;
    let router = restricted_routes(&composition, bound_address);
    let tls_acceptor = TlsAcceptor::from(tls_config);
    let slots = ConnectionSlots::new();
    let rate_limiter = Arc::new(RateLimiter::new());

    loop {
        let (stream, source) = tcp_listener
            .accept()
            .await
            .map_err(|_| StartupError::HttpsListenerUnavailable)?;
        if !is_trusted_loopback_peer(source.ip()) {
            drop(stream);
            continue;
        }
        if let Ok(connection_permit) = Arc::clone(&slots.normal).try_acquire_owned() {
            let tls_acceptor = tls_acceptor.clone();
            let router = router.clone();
            let rate_limiter = Arc::clone(&rate_limiter);
            tokio::spawn(async move {
                serve_normal_connection(stream, source.ip(), tls_acceptor, router, rate_limiter)
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

        tokio::spawn(async move {
            serve_rejection_connection(stream, tls_acceptor).await;
            drop(rejection_permit);
        });
    }
}

fn is_trusted_loopback_peer(peer: IpAddr) -> bool {
    peer == IpAddr::V4(Ipv4Addr::LOCALHOST) || peer == IpAddr::V6(Ipv6Addr::LOCALHOST)
}

async fn serve_normal_connection(
    stream: TcpStream,
    source: IpAddr,
    tls_acceptor: TlsAcceptor,
    router: Router,
    rate_limiter: Arc<RateLimiter>,
) {
    serve_normal_connection_with_timeouts(
        stream,
        source,
        tls_acceptor,
        router,
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
    router: Router,
    rate_limiter: Arc<RateLimiter>,
    timeouts: ConnectionTimeouts,
) {
    let Ok(Ok(mut tls_stream)) = timeout(timeouts.handshake, tls_acceptor.accept(stream)).await
    else {
        return;
    };

    let response = processing_response(
        timeouts.processing,
        process_restricted_request(
            &mut tls_stream,
            source,
            router,
            rate_limiter,
            timeouts.request_read,
        ),
    )
    .await;
    let _ = timeout(
        timeouts.processing,
        write_bounded_response(&mut tls_stream, response),
    )
    .await;
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

async fn process_restricted_request<S>(
    stream: &mut S,
    source: IpAddr,
    router: Router,
    rate_limiter: Arc<RateLimiter>,
    request_read_timeout: Duration,
) -> BoundedResponse
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = match read_http_request_with_timeout(stream, request_read_timeout).await {
        RequestHeadRead::Completed(request) => {
            if !rate_limiter.allows(source, Instant::now()) {
                let mut response = rate_limited_response();
                // RFC 9110 §9.3.2: HEAD responses must not include a message body.
                if matches!(request.as_ref(), Ok(r) if *r.method() == Method::HEAD) {
                    response.body = Bytes::new();
                }
                return response;
            }
            *request
        }
        RequestHeadRead::Incomplete(error) => return response_for_request_read_error(error),
    };

    let request = match request {
        Ok(request) => request,
        Err(error) => return response_for_request_read_error(error),
    };

    let is_head = *request.method() == Method::HEAD;
    let response = router
        .oneshot(request)
        .await
        .expect("restricted router response is infallible");
    let mut bounded = bounded_response_from_axum(response).await;
    // RFC 9110 §9.3.2: HEAD responses must not include a message body.
    if is_head {
        bounded.body = Bytes::new();
    }
    bounded
}

#[derive(Debug)]
enum RequestReadError {
    TimedOut,
    TargetTooLong,
    MethodNotAllowed,
    HeadersTooLarge,
    Invalid,
}

enum RequestHeadRead {
    Completed(Box<Result<Request, RequestReadError>>),
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

async fn read_http_request_with_timeout<S>(
    stream: &mut S,
    request_read_timeout: Duration,
) -> RequestHeadRead
where
    S: AsyncRead + Unpin,
{
    match timeout(request_read_timeout, read_http_request_outcome(stream)).await {
        Ok(result) => result,
        Err(_) => RequestHeadRead::Incomplete(RequestReadError::TimedOut),
    }
}

#[cfg(test)]
async fn read_http_request<S>(stream: &mut S) -> Result<Request, RequestReadError>
where
    S: AsyncRead + Unpin,
{
    match read_http_request_outcome(stream).await {
        RequestHeadRead::Completed(result) => *result,
        RequestHeadRead::Incomplete(error) => Err(error),
    }
}

async fn read_http_request_outcome<S>(stream: &mut S) -> RequestHeadRead
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
                match parse_http_request(&bytes, request_line.completed_classification()) {
                    Ok(request) => read_request_body(stream, request).await,
                    Err(error) => Err(error),
                }
            };
            return RequestHeadRead::Completed(Box::new(request));
        }
    }
}

/// Reads the bounded request body the completed head declared.
///
/// This runs inside the caller's single request-read budget, so the head and
/// body share one deadline and the body never restarts it.
async fn read_request_body<S>(
    stream: &mut S,
    mut request: Request,
) -> Result<Request, RequestReadError>
where
    S: AsyncRead + Unpin,
{
    let Some(length) = declared_request_body_length(request.method(), request.headers())? else {
        return Ok(request);
    };
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|_| RequestReadError::Invalid)?;
    if stream_has_pending_bytes(stream).await {
        return Err(RequestReadError::Invalid);
    }
    *request.body_mut() = Body::from(body);
    Ok(request)
}

/// Returns the exact number of body bytes a completed head declared.
///
/// Only `PUT` may carry a body, and only through exactly one canonical
/// `Content-Length` within [`MAX_REQUEST_BODY_BYTES`]. Every other method must
/// declare no body at all. Chunked framing and `Expect` are never supported.
fn declared_request_body_length(
    method: &Method,
    headers: &HeaderMap,
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
    if length > MAX_REQUEST_BODY_BYTES {
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
        };
    }
    BoundedResponse {
        status,
        profile,
        body,
        allow,
    }
}

fn fixed_json_body(body: &[u8]) -> Option<&'static str> {
    match body {
        b"{\"error\":\"bad_request\"}" => Some("{\"error\":\"bad_request\"}"),
        b"{\"error\":\"database_selection_not_allowed\"}" => {
            Some("{\"error\":\"database_selection_not_allowed\"}")
        }
        b"{\"error\":\"method_not_allowed\"}" => Some("{\"error\":\"method_not_allowed\"}"),
        b"{\"error\":\"not_found\"}" => Some("{\"error\":\"not_found\"}"),
        b"{\"error\":\"request_header_fields_too_large\"}" => {
            Some("{\"error\":\"request_header_fields_too_large\"}")
        }
        b"{\"error\":\"request_origin_denied\"}" => Some("{\"error\":\"request_origin_denied\"}"),
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
    let head = format!(
        "HTTP/1.1 {} \r\nContent-Type: {}\r\n{}{}\r\n",
        response.status.as_u16(),
        response.profile.media_type(),
        allow,
        response.profile.security_headers(),
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}

async fn processing_response<F>(processing_timeout: Duration, processing: F) -> BoundedResponse
where
    F: Future<Output = BoundedResponse>,
{
    match timeout(processing_timeout, processing).await {
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

fn restricted_routes(composition: &PreoperationalComposition, listener: SocketAddr) -> Router {
    let router = Router::new().fallback(not_found);
    match composition.outcome {
        StartupOutcome::UninitializedWithoutDatabase
        | StartupOutcome::UninitializedWithDatabase => {
            preoperational_routes(router, composition, listener)
        }
        StartupOutcome::InitializationPending(_) => router,
    }
}

/// Composes the Web UI Client Module's declared pre-operational surface.
///
/// The module owns its asset inventory and route paths; the core only mounts
/// them and supplies the trusted lifecycle authority behind them. Every mounted
/// path is exact, so an unknown target, including any `/api/` target, falls
/// through to the fixed not-found response.
fn preoperational_routes(
    router: Router,
    composition: &PreoperationalComposition,
    listener: SocketAddr,
) -> Router {
    router
        .route(
            STATUS_ROUTE,
            weavelit_module_client_webui::preoperational_status_route(
                composition.projection_source(),
            ),
        )
        .route(
            APPLICATION_DATABASE_ROUTE,
            weavelit_module_client_webui::database_selection_route(
                ExpectedOrigin::from_listener(listener),
                composition.selection_commit(),
            ),
        )
        .merge(weavelit_module_client_webui::embedded_asset_routes())
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

/// Composes the lifecycle crate and SQLite backend, opens or creates the anchor
/// set, and classifies startup state.
///
/// Returns a `StartupOutcome` for every supported restricted state.
/// Fails closed for `Initialized` and `PostCommitReconciliationRequired` states
/// since normal operation and sealing are not yet implemented.
pub fn classify_restricted_startup(state_root: &Path) -> Result<RestrictedStartup, StartupError> {
    let store = LifecycleStore::open_or_create(state_root).map_err(map_open_error)?;

    let catalog = Arc::new(sqlite_catalog());
    let context = Arc::new(TrustedBackendContext::new(
        state_root.join(APPLICATION_DATABASE_FILE),
    ));

    let classification = store
        .classify_startup(&catalog, &context)
        .map_err(map_classification_error)?;

    let outcome = match classification {
        LifecycleClassification::UninitializedWithoutDatabase => {
            StartupOutcome::UninitializedWithoutDatabase
        }
        LifecycleClassification::UninitializedWithDatabase => {
            StartupOutcome::UninitializedWithDatabase
        }
        LifecycleClassification::InitializationPending(kind) => {
            StartupOutcome::InitializationPending(kind)
        }
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
        // Fail closed for states not yet handled by this milestone.
        LifecycleClassification::PostCommitReconciliationRequired
        | LifecycleClassification::Initialized => {
            return Err(StartupError::StateCombinationInvalid);
        }
    };
    let arbiter = Arc::new(WorkflowArbiter::new(store));
    Ok(RestrictedStartup {
        outcome,
        log_catalog: sqlite_log_catalog(),
        // The arbiter takes ownership of the store, so it also retains the
        // process-lifetime state-root lock.
        composition: PreoperationalComposition {
            outcome,
            adapter: Arc::new(LifecycleAdapter::new(arbiter)),
            catalog,
            context,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        future::pending,
        net::SocketAddr,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        response::Response,
        routing::any,
    };
    use http_body_util::BodyExt;
    use rustls::{
        ClientConfig, RootCertStore, ServerConfig,
        crypto::aws_lc_rs,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpSocket, TcpStream},
    };
    use tokio_rustls::{TlsAcceptor, TlsConnector};
    use tower::ServiceExt;
    use weavelit_module_client_webui::DatabaseSelectionRejection;
    use weavelit_server_lifecycle::{
        BackendIdentifier, LifecycleProjection, LifecycleStore, TrustedBackendContext,
    };

    use super::{
        APPLICATION_DATABASE_FILE, APPLICATION_DATABASE_ROUTE, ASSET_SECURITY_HEADERS,
        AllowedMethod, BoundedResponse, ConnectionSlots, ConnectionTimeouts,
        MAX_REQUEST_BODY_BYTES, PreoperationalComposition, RATE_LIMIT_BURST,
        RATE_LIMIT_REQUESTS_PER_MINUTE, REQUEST_PROCESSING_TIMEOUT, REQUEST_READ_TIMEOUT,
        RateLimiter, RequestHeadRead, RequestReadError, ResponseProfile, RestrictedStartup,
        StartupError, StartupOutcome, TLS_HANDSHAKE_TIMEOUT, bounded_response_from_axum,
        classify_restricted_startup, gateway_timeout_response, parse_http_request,
        processing_response, raw_header_section_bytes, read_http_request,
        read_http_request_with_timeout, request_timeout_response,
        serve_normal_connection_with_timeouts, serve_rejection_connection_with_timeouts,
        sqlite_catalog,
    };

    /// Listener authority used by router-level tests that bind no socket.
    const UNBOUND_LISTENER: &str = "127.0.0.1:8443";

    /// The exact accepted Application Database selection body.
    const SELECTION_BODY: &str = "{\"backend\":\"sqlite\",\"settings\":{}}";

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

        fn composition(&self) -> PreoperationalComposition {
            self.startup.composition.clone()
        }

        fn routes(&self) -> Router {
            self.routes_for(UNBOUND_LISTENER.parse().unwrap())
        }

        fn routes_for(&self, listener: SocketAddr) -> Router {
            super::restricted_routes(&self.startup.composition, listener)
        }

        /// Snapshots the lifecycle record and every locator file by name, bytes,
        /// and modification time.
        fn anchor_snapshot(&self) -> Vec<(OsString, Vec<u8>, i64, i64)> {
            anchor_snapshot(&self.state_root)
        }
    }

    fn anchor_snapshot(state_root: &Path) -> Vec<(OsString, Vec<u8>, i64, i64)> {
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

    /// Serves the restricted listener over a surface whose state root is removed
    /// before serving begins; the read-only routes these callers exercise are
    /// unaffected.
    async fn serve_restricted_https_listener(
        tcp_listener: TcpListener,
        tls_config: Arc<ServerConfig>,
        outcome: StartupOutcome,
    ) -> Result<(), StartupError> {
        let composition = Surface::new(outcome).composition();
        super::serve_restricted_https_listener(tcp_listener, tls_config, composition).await
    }

    async fn response_body(response: axum::response::Response) -> String {
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
        match read_http_request(&mut server).await {
            Ok(_) => RequestHeadResult::Accepted,
            Err(error) => head_result(error),
        }
    }

    /// Reads a complete request and returns the body bytes it accepted.
    async fn read_request_with_body(bytes: &[u8]) -> Result<Vec<u8>, RequestHeadResult> {
        let (mut client, mut server) = tokio::io::duplex(bytes.len().max(1));
        client.write_all(bytes).await.unwrap();
        client.shutdown().await.unwrap();
        match read_http_request(&mut server).await {
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
    /// by both expires, while a budget restarted at the body would not.
    #[tokio::test]
    async fn request_read_timeout_covers_the_head_and_body_as_one_budget() {
        let schedule = |budget: Duration| async move {
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
            let outcome = read_http_request_with_timeout(&mut server, budget).await;
            writer.abort();
            outcome
        };

        assert!(matches!(
            schedule(Duration::from_millis(500)).await,
            RequestHeadRead::Incomplete(RequestReadError::TimedOut)
        ));
        assert!(matches!(
            schedule(Duration::from_millis(1_500)).await,
            RequestHeadRead::Completed(result) if result.is_ok()
        ));
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
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, source) = listener.accept().await.unwrap();
            serve_normal_connection_with_timeouts(
                stream,
                source.ip(),
                TlsAcceptor::from(server_config),
                router,
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
                router,
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
        let success = weavelit_module_client_webui::database_selection_response(
            &LifecycleProjection::new(true),
        );
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
                restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
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
                router,
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
            weavelit_module_client_webui::MAX_DATABASE_SELECTION_BODY_BYTES
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
        let server = tokio::spawn(super::serve_restricted_https_listener(
            listener,
            server_config,
            surface.composition(),
        ));

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
            read_http_request_with_timeout(&mut server, Duration::ZERO).await
        });
        let _ = client.write_all(b"").await;
        assert!(matches!(
            read.await.unwrap(),
            super::RequestHeadRead::Incomplete(super::RequestReadError::TimedOut)
        ));

        let response = processing_response(Duration::ZERO, pending::<BoundedResponse>()).await;
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
                    weavelit_module_client_webui::SelectedBackend::Sqlite,
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
                    weavelit_module_client_webui::SelectedBackend::Sqlite,
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

        let committed_clone = Arc::clone(&committed);
        let started_gate_clone = Arc::clone(&started_gate);
        let finish_gate_clone = Arc::clone(&finish_gate);

        // A SelectionCommit that signals when spawn_blocking has started, then
        // waits for the test to signal completion.  This simulates a durable
        // write that must not be interrupted once it begins.
        let commit: weavelit_module_client_webui::SelectionCommit = Arc::new(move |_backend| {
            let committed = Arc::clone(&committed_clone);
            let started_gate = Arc::clone(&started_gate_clone);
            let finish_gate = Arc::clone(&finish_gate_clone);
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    started_gate.wait(); // signal: spawn_blocking has started
                    finish_gate.wait(); // wait for test to say "go"
                    committed.store(true, Ordering::SeqCst);
                    Ok(weavelit_server_lifecycle::LifecycleProjection::new(true))
                })
                .await
                .map_err(|_| DatabaseSelectionRejection::ServiceUnavailable)?
            })
        });

        let expected_origin = weavelit_module_client_webui::ExpectedOrigin::from_listener(
            UNBOUND_LISTENER.parse().unwrap(),
        );
        let router = Router::new().route(
            APPLICATION_DATABASE_ROUTE,
            weavelit_module_client_webui::database_selection_route(expected_origin, commit),
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

        assert!(
            committed.load(Ordering::SeqCst),
            "spawn_blocking must complete even after the outer task is aborted"
        );
    }
}

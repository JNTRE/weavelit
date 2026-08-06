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
        header::{ALLOW, CONTENT_TYPE},
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
    time::timeout,
};
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;
use weavelit_server_database_sqlite::{RetainedSqliteInspection, SqliteDatabase};
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendRegistration,
    DatabaseError, DeploymentIdentifier, InterruptedLifecycleAction, LifecycleClassification,
    LifecycleError, LifecycleStore, RetainedDatabaseInspection, TrustedBackendContext,
    ValidatedConnectionSettings, WorkflowKind,
};
use weavelit_server_log::LogModuleCatalog;

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
const MAX_REQUEST_TARGET_BYTES: usize = 2 * 1024;
const MAX_REQUEST_HEADER_BYTES: usize = 8 * 1024;
const MAX_REQUEST_HEAD_BYTES: usize = MAX_REQUEST_TARGET_BYTES + MAX_REQUEST_HEADER_BYTES + 128;
const RATE_LIMIT_REQUESTS_PER_MINUTE: u32 = 20;
const RATE_LIMIT_BURST: u32 = 5;
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

#[derive(Clone)]
struct BoundedResponse {
    status: StatusCode,
    profile: ResponseProfile,
    body: Bytes,
    allow_get: bool,
}

impl BoundedResponse {
    fn json(status: StatusCode, body: &'static str) -> Self {
        Self {
            status,
            profile: ResponseProfile::Json,
            body: Bytes::from_static(body.as_bytes()),
            allow_get: false,
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
    _store: LifecycleStore,
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
    let result =
        serve_restricted_https_listener(tcp_listener, listener.tls_config(), startup.outcome())
            .await;
    drop(startup);
    result
}

async fn serve_restricted_https_listener(
    tcp_listener: TcpListener,
    tls_config: Arc<ServerConfig>,
    outcome: StartupOutcome,
) -> Result<(), StartupError> {
    let router = restricted_routes(outcome);
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
                return rate_limited_response();
            }
            *request
        }
        RequestHeadRead::Incomplete(error) => return response_for_request_read_error(error),
    };

    let request = match request {
        Ok(request) => request,
        Err(error) => return response_for_request_read_error(error),
    };

    let response = router
        .oneshot(request)
        .await
        .expect("restricted router response is infallible");
    bounded_response_from_axum(response).await
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
    MethodNotAllowed,
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
            Self::Headers(RequestLineClassification::MethodNotAllowed) => {
                RequestReadError::MethodNotAllowed
            }
            Self::Headers(RequestLineClassification::Get) => RequestReadError::HeadersTooLarge,
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
        Ok(_) => RequestLineClassification::MethodNotAllowed,
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
                parse_http_request(&bytes, request_line.completed_classification())
            };
            return RequestHeadRead::Completed(Box::new(request));
        }
    }
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
        RequestLineClassification::Get => {}
        RequestLineClassification::MethodNotAllowed => {
            return Err(RequestReadError::MethodNotAllowed);
        }
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
    let allow_get = response
        .headers()
        .get(ALLOW)
        .is_some_and(|value| value == "GET");
    let Some(profile) = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(ResponseProfile::from_media_type)
    else {
        return redacted_response(status, allow_get);
    };
    let Ok(body) = to_bytes(response.into_body(), profile.max_body_bytes()).await else {
        return redacted_response(status, allow_get);
    };
    if profile == ResponseProfile::Json {
        let Some(body) = fixed_json_body(&body) else {
            return redacted_response(status, allow_get);
        };
        return BoundedResponse {
            status,
            profile,
            body: Bytes::from_static(body.as_bytes()),
            allow_get,
        };
    }
    BoundedResponse {
        status,
        profile,
        body,
        allow_get,
    }
}

fn fixed_json_body(body: &[u8]) -> Option<&'static str> {
    match body {
        b"{\"error\":\"bad_request\"}" => Some("{\"error\":\"bad_request\"}"),
        b"{\"error\":\"method_not_allowed\"}" => Some("{\"error\":\"method_not_allowed\"}"),
        b"{\"error\":\"not_found\"}" => Some("{\"error\":\"not_found\"}"),
        b"{\"error\":\"request_header_fields_too_large\"}" => {
            Some("{\"error\":\"request_header_fields_too_large\"}")
        }
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
fn redacted_response(status: StatusCode, allow_get: bool) -> BoundedResponse {
    BoundedResponse {
        allow_get,
        ..BoundedResponse::json(status, "{\"error\":\"gateway_timeout\"}")
    }
}

async fn write_bounded_response<S>(stream: &mut S, response: BoundedResponse) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let allow = if response.allow_get {
        "Allow: GET\r\n"
    } else {
        ""
    };
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
        allow_get: true,
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

fn restricted_routes(outcome: StartupOutcome) -> Router {
    let router = Router::new().fallback(not_found);
    match outcome {
        StartupOutcome::UninitializedWithoutDatabase => preoperational_routes(router, false),
        StartupOutcome::UninitializedWithDatabase => preoperational_routes(router, true),
        StartupOutcome::InitializationPending(_) => router,
    }
}

/// Composes the Web UI Client Module's declared pre-operational surface.
///
/// The module owns its asset inventory and route paths; the core only mounts
/// them. Every mounted path is exact, so an unknown target, including any
/// `/api/` target, falls through to the fixed not-found response.
fn preoperational_routes(router: Router, database_selected: bool) -> Router {
    router
        .route(
            "/api/v1/status",
            weavelit_module_client_webui::preoperational_status_route(database_selected),
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

    let catalog = sqlite_catalog();
    let context = TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE));

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
    Ok(RestrictedStartup {
        outcome,
        log_catalog: sqlite_log_catalog(),
        _store: store,
    })
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

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

    use super::{
        ASSET_SECURITY_HEADERS, BoundedResponse, ConnectionSlots, ConnectionTimeouts,
        REQUEST_PROCESSING_TIMEOUT, REQUEST_READ_TIMEOUT, RateLimiter, ResponseProfile,
        StartupOutcome, TLS_HANDSHAKE_TIMEOUT, gateway_timeout_response, parse_http_request,
        processing_response, raw_header_section_bytes, read_http_request,
        read_http_request_with_timeout, request_timeout_response, restricted_routes,
        serve_normal_connection_with_timeouts, serve_rejection_connection_with_timeouts,
        serve_restricted_https_listener,
    };

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
            Err(super::RequestReadError::Invalid) => RequestHeadResult::Invalid,
            Err(super::RequestReadError::MethodNotAllowed) => RequestHeadResult::MethodNotAllowed,
            Err(super::RequestReadError::TargetTooLong) => RequestHeadResult::TargetTooLong,
            Err(super::RequestReadError::HeadersTooLarge) => RequestHeadResult::HeadersTooLarge,
            Err(super::RequestReadError::TimedOut) => panic!("reader test must not time out"),
        }
    }

    fn assert_fixed_tls_response(response: &[u8], status: u16, body: &str, allow_get: bool) {
        let allow = if allow_get { "Allow: GET\r\n" } else { "" };
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
            "/assets/application.js",
            "assets/application.js",
            "text/javascript; charset=utf-8",
        ),
        (
            "/assets/application.css",
            "assets/application.css",
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
            "/api/v1/assets/application.js",
            "/api/v1/unknown",
            "/index.html",
            "/assets/",
            "/assets/application.js/",
            "/ASSETS/application.js",
            "/assets/%61pplication.js",
            "/assets/../assets/application.js",
            "/../assets/application.js",
            "/assets/application.js.map",
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
                "valid non-GET method",
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                RequestHeadResult::MethodNotAllowed,
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

        for (name, request, status, body, allow_get) in [
            (
                "valid GET",
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                false,
            ),
            (
                "valid non-GET",
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                405,
                "{\"error\":\"method_not_allowed\"}",
                true,
            ),
            (
                "invalid method",
                "GE(T /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                400,
                "{\"error\":\"bad_request\"}",
                false,
            ),
            (
                "target over two KiB before malformed content",
                format!(
                    "GET /{} HTTP/1.1\r\nmalformed\r\n\r\n",
                    "a".repeat(super::MAX_REQUEST_TARGET_BYTES)
                ),
                414,
                "{\"error\":\"uri_too_long\"}",
                false,
            ),
            (
                "headers over eight KiB",
                oversized_headers,
                431,
                "{\"error\":\"request_header_fields_too_large\"}",
                false,
            ),
            (
                "more than sixty-four headers",
                many_headers,
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
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
            assert_fixed_tls_response(&response, status, body, allow_get);
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
        assert_fixed_tls_response(&response, 400, "{\"error\":\"bad_request\"}", false);

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

        for _ in 0..5 {
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

        for (name, request, fixed_status, fixed_body, allow_get) in [
            (
                "valid POST",
                "POST /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                405,
                "{\"error\":\"method_not_allowed\"}",
                true,
            ),
            (
                "malformed head",
                "GET /api/v1/status HTTP/1.1\r\nnot-a-header\r\n\r\n".to_owned(),
                400,
                "{\"error\":\"bad_request\"}",
                false,
            ),
            (
                "oversized target",
                oversized_target,
                414,
                "{\"error\":\"uri_too_long\"}",
                false,
            ),
            (
                "oversized headers",
                oversized_headers,
                431,
                "{\"error\":\"request_header_fields_too_large\"}",
                false,
            ),
            (
                "valid GET",
                "GET /api/v1/status HTTP/1.1\r\n\r\n".to_owned(),
                200,
                "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}",
                false,
            ),
        ] {
            let rate_limiter = Arc::new(RateLimiter::new());
            for _ in 0..5 {
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
                assert_fixed_tls_response(&response, fixed_status, fixed_body, allow_get);
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
            assert_fixed_tls_response(&response, 429, "{\"error\":\"rate_limited\"}", false);
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
            assert_fixed_tls_response(&response, status, body, false);

            for _ in 0..5 {
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
        for _ in 0..5 {
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
            assert_fixed_tls_response(&rejected, 405, "{\"error\":\"method_not_allowed\"}", true);
        }

        let unknown = direct_tls_response(
            restricted_routes(StartupOutcome::UninitializedWithoutDatabase),
            Arc::clone(&server_config),
            Arc::clone(&client_config),
            b"GET /assets/../assets/application.js HTTP/1.1\r\n\r\n",
            REQUEST_READ_TIMEOUT,
            REQUEST_PROCESSING_TIMEOUT,
        )
        .await;
        assert_fixed_tls_response(&unknown, 404, "{\"error\":\"not_found\"}", false);

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
        assert_fixed_tls_response(&gated, 404, "{\"error\":\"not_found\"}", false);
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
        assert_fixed_tls_response(&over_limit, 200, "{\"error\":\"gateway_timeout\"}", false);

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
                false,
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
        assert_fixed_tls_response(&unknown_json, 200, "{\"error\":\"gateway_timeout\"}", false);
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

    #[test]
    fn rate_limiter_allows_burst_five_then_refills_at_twenty_per_minute() {
        let limiter = RateLimiter::new();
        let source = "127.0.0.1".parse().unwrap();
        let now = std::time::Instant::now();
        for _ in 0..5 {
            assert!(limiter.allows(source, now));
        }
        assert!(!limiter.allows(source, now));
        assert!(limiter.allows(source, now + Duration::from_secs(3)));
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
}

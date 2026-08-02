#![forbid(unsafe_code)]

//! Restricted lifecycle startup composition for the Weavelit Server.

use std::{
    env, fs,
    io::Read,
    net::SocketAddr,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{
        HeaderMap, Method, StatusCode,
        header::{ACCEPT, ALLOW, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
    },
    response::Response,
    routing::any,
};
use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use rustls::{
    ServerConfig,
    crypto::aws_lc_rs,
    pki_types::{
        CertificateDer, PrivateKeyDer,
        pem::{PemObject, SectionKind},
    },
};
use tokio::{net::TcpListener, sync::Semaphore, time::timeout};
use tokio_rustls::TlsAcceptor;
use weavelit_server_database_sqlite::SqliteDatabase;
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendRegistration,
    LifecycleClassification, LifecycleError, LifecycleStore, TrustedBackendContext,
    ValidatedConnectionSettings, WorkflowKind,
};

const STATE_ROOT_ENV: &str = "WEAVELIT_STATE_ROOT";
const HTTPS_LISTENER_ADDRESS_ENV: &str = "WEAVELIT_HTTPS_LISTENER_ADDRESS";
const TLS_CERTIFICATE_PATH_ENV: &str = "WEAVELIT_TLS_CERTIFICATE_PATH";
const TLS_PRIVATE_KEY_PATH_ENV: &str = "WEAVELIT_TLS_PRIVATE_KEY_PATH";
const APPLICATION_DATABASE_FILE: &str = "application.sqlite3";
const MAX_TLS_MATERIAL_BYTES: u64 = 1024 * 1024;
const MAX_CONNECTIONS: usize = 16;
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_PROCESSING_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST_TARGET_BYTES: usize = 2 * 1024;
const MAX_REQUEST_HEADER_BYTES: usize = 8 * 1024;

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
    if !has_safe_absolute_path(path) {
        return Err(StartupError::TlsMaterialInvalid);
    }

    let metadata = fs::symlink_metadata(path).map_err(|_| StartupError::TlsMaterialInvalid)?;
    let mode = metadata.mode();
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || mode & 0o022 != 0
        || (private_key && mode & 0o007 != 0)
        || metadata.len() == 0
        || metadata.len() > MAX_TLS_MATERIAL_BYTES
    {
        return Err(StartupError::TlsMaterialInvalid);
    }

    let file = fs::File::open(path).map_err(|_| StartupError::TlsMaterialInvalid)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_TLS_MATERIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StartupError::TlsMaterialInvalid)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_TLS_MATERIAL_BYTES {
        return Err(StartupError::TlsMaterialInvalid);
    }
    Ok(bytes)
}

fn has_safe_absolute_path(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }

    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(component) => {
                current.push(component);
                let Ok(metadata) = fs::symlink_metadata(&current) else {
                    return false;
                };
                if metadata.file_type().is_symlink() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn parse_certificates(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, StartupError> {
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
        }
    }
}

// ---------------------------------------------------------------------------
// Restricted HTTPS listener and route composition
// ---------------------------------------------------------------------------

/// Binds and serves the sole direct-TLS pre-operational listener.
pub async fn run_restricted_https_listener(
    listener: TrustedHttpsListener,
    outcome: StartupOutcome,
) -> Result<(), StartupError> {
    let tcp_listener = TcpListener::bind(listener.address())
        .await
        .map_err(|_| StartupError::HttpsListenerUnavailable)?;
    let router = restricted_routes(outcome);
    let tls_acceptor = TlsAcceptor::from(listener.tls_config());
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    loop {
        let (stream, _) = tcp_listener
            .accept()
            .await
            .map_err(|_| StartupError::HttpsListenerUnavailable)?;
        let Ok(connection_permit) = Arc::clone(&connections).try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let tls_acceptor = tls_acceptor.clone();
        let router = router.clone();

        tokio::spawn(async move {
            let Ok(Ok(tls_stream)) =
                timeout(TLS_HANDSHAKE_TIMEOUT, tls_acceptor.accept(stream)).await
            else {
                return;
            };

            let mut builder = http1::Builder::new();
            builder.keep_alive(false);
            builder.max_headers(64);
            builder.max_buf_size(MAX_REQUEST_HEADER_BYTES + MAX_REQUEST_TARGET_BYTES);
            let service = TowerToHyperService::new(router.into_service());
            let connection = builder.serve_connection(TokioIo::new(tls_stream), service);
            let _ = timeout(REQUEST_PROCESSING_TIMEOUT, connection).await;
            drop(connection_permit);
        });
    }
}

fn restricted_routes(outcome: StartupOutcome) -> Router {
    let router = Router::new().fallback(not_found);
    match outcome {
        StartupOutcome::UninitializedWithoutDatabase => router.route(
            "/api/v1/status",
            any(|request| status_response(request, false)),
        ),
        StartupOutcome::UninitializedWithDatabase => router.route(
            "/api/v1/status",
            any(|request| status_response(request, true)),
        ),
        StartupOutcome::InitializationPending(_) => router,
    }
}

async fn status_response(request: Request, database_selected: bool) -> Response {
    let (parts, _body) = request.into_parts();
    if request_target_bytes(&parts.uri) > MAX_REQUEST_TARGET_BYTES {
        return json_response(StatusCode::URI_TOO_LONG, "uri_too_long");
    }
    if request_header_bytes(&parts.headers) > MAX_REQUEST_HEADER_BYTES {
        return json_response(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "request_header_fields_too_large",
        );
    }
    if parts.method != Method::GET {
        return json_response_with_allow(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    }
    if has_request_body(&parts.headers) || !accepts_json(&parts.headers) {
        return json_response(StatusCode::BAD_REQUEST, "bad_request");
    }

    let body = if database_selected {
        "{\"lifecycle\":\"uninitialized\",\"database_selected\":true}"
    } else {
        "{\"lifecycle\":\"uninitialized\",\"database_selected\":false}"
    };
    json_response_body(StatusCode::OK, body)
}

async fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, "not_found")
}

fn request_target_bytes(uri: &axum::http::Uri) -> usize {
    uri.path_and_query()
        .map_or(0, |target| target.as_str().len())
}

fn request_header_bytes(headers: &HeaderMap) -> usize {
    headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
        .sum()
}

fn has_request_body(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_LENGTH)
        .is_some_and(|value| value.as_bytes() != b"0")
        || headers.contains_key(TRANSFER_ENCODING)
}

fn accepts_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .is_none_or(|value| value.as_bytes() == b"application/json")
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

fn json_response_with_allow(status: StatusCode, error: &'static str) -> Response {
    let mut response = json_response(status, error);
    response.headers_mut().insert(ALLOW, "GET".parse().unwrap());
    response
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
pub fn classify_restricted_startup(state_root: &Path) -> Result<StartupOutcome, StartupError> {
    let mut store = LifecycleStore::open_or_create(state_root).map_err(map_open_error)?;

    let catalog = sqlite_catalog();
    let context = TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE));

    let classification = store
        .classify_startup(&catalog, &context)
        .map_err(map_classification_error)?;

    match classification {
        LifecycleClassification::UninitializedWithoutDatabase => {
            Ok(StartupOutcome::UninitializedWithoutDatabase)
        }
        LifecycleClassification::UninitializedWithDatabase => {
            Ok(StartupOutcome::UninitializedWithDatabase)
        }
        LifecycleClassification::InitializationPending(kind) => {
            Ok(StartupOutcome::InitializationPending(kind))
        }
        // Fail closed for states not yet handled by this milestone.
        LifecycleClassification::PostCommitReconciliationRequired
        | LifecycleClassification::Initialized => Err(StartupError::StateCombinationInvalid),
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode, header::HeaderValue},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::{
        StartupOutcome, accepts_json, has_request_body, request_header_bytes, request_target_bytes,
        restricted_routes,
    };

    async fn response_body(response: axum::response::Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
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

    #[test]
    fn request_bounds_and_content_negotiation_are_strict() {
        let target = "/api/v1/status?".to_owned() + &"a".repeat(2 * 1024);
        assert!(request_target_bytes(&target.parse().unwrap()) > 2 * 1024);

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("accept", HeaderValue::from_static("application/json"));
        assert!(accepts_json(&headers));
        assert!(!has_request_body(&headers));
        headers.insert("content-length", HeaderValue::from_static("1"));
        assert!(has_request_body(&headers));
        headers.insert(
            "x-bound",
            HeaderValue::from_str(&"a".repeat(8 * 1024)).unwrap(),
        );
        assert!(request_header_bytes(&headers) > 8 * 1024);
    }
}

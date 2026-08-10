//! Route-registered transport profiles and ordered request admission.
//!
//! The listener's body bound and read budgets are selected per request from the
//! transport registrations that travel with the mounted router, never from the
//! request target alone. A surface that did not mount a route also carries no
//! registration for it, so an unmounted capability cannot grant a larger body.
//!
//! Body allocation is reachable only through the ordered admission chain below.
//! Each stage consumes the previous stage's value and no stage has a public
//! constructor, so skipping or reordering a stage is a compile error rather
//! than a review finding.

use std::{
    any::Any,
    future::Future,
    net::IpAddr,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderMap, Method, StatusCode, Uri},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant as Deadline,
};

use crate::{
    BoundedResponse, MAX_REQUEST_BODY_BYTES, RateLimiter, RequestReadError,
    declared_request_body_length, json_fixed_response, stream_has_pending_bytes,
};

// ---------------------------------------------------------------------------
// Transport profiles
// ---------------------------------------------------------------------------

/// Time a transport profile grants after the request head has been read.
///
/// The head always keeps its own absolute request-read deadline. Only the
/// [`TransportBudget::Admitted`] variant, which no request can select without a
/// mounted registration, grants any additional time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportBudget {
    /// The body shares the head's single request-read deadline and the
    /// connection's own processing deadline. Nothing is extended.
    SharedWithHead,
    /// An admitted body receives its own read budget and its own processing
    /// budget, each starting only once the body has been admitted.
    Admitted {
        /// Budget for reading the admitted body.
        body_read: Duration,
        /// Budget for handling the request after its body has been read.
        processing: Duration,
    },
}

/// Body bound and read budgets the listener applies to one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportProfile {
    max_body_bytes: usize,
    budget: TransportBudget,
}

impl TransportProfile {
    /// The profile every request receives unless a mounted registration selects
    /// another one. It is exactly the listener's historical uniform behavior.
    pub const DEFAULT: Self = Self {
        max_body_bytes: MAX_REQUEST_BODY_BYTES,
        budget: TransportBudget::SharedWithHead,
    };

    /// Builds the profile a route registration grants an admitted body.
    #[must_use]
    pub const fn admitted(
        max_body_bytes: usize,
        body_read: Duration,
        processing: Duration,
    ) -> Self {
        Self {
            max_body_bytes,
            budget: TransportBudget::Admitted {
                body_read,
                processing,
            },
        }
    }

    /// Returns the largest body this profile admits.
    #[must_use]
    pub const fn max_body_bytes(self) -> usize {
        self.max_body_bytes
    }

    /// Returns the budget this profile grants after the head.
    #[must_use]
    pub const fn budget(self) -> TransportBudget {
        self.budget
    }

    /// Returns the absolute deadline the body read must complete within.
    pub(crate) fn body_deadline(self, head_deadline: Deadline, admitted_at: Deadline) -> Deadline {
        match self.budget {
            TransportBudget::SharedWithHead => head_deadline,
            TransportBudget::Admitted { body_read, .. } => admitted_at + body_read,
        }
    }

    /// Returns the absolute deadline request handling must complete within.
    pub(crate) fn processing_deadline(
        self,
        started: Deadline,
        connection_processing: Duration,
        body_deadline: Deadline,
    ) -> Deadline {
        match self.budget {
            TransportBudget::SharedWithHead => started + connection_processing,
            TransportBudget::Admitted { processing, .. } => body_deadline + processing,
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-body validation seam
// ---------------------------------------------------------------------------

/// Closed set of rejections a registered pre-body check may return.
///
/// A check selects one of these fixed outcomes and can never supply a body,
/// header, message, or detail of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreBodyRejection {
    /// The request head is not acceptable for the route it targets.
    BadRequest,
    /// The request failed the route's same-origin or cross-site request
    /// forgery precondition.
    RequestOriginDenied,
    /// The request presented no usable one-time submission ticket.
    RestoreTicketInvalid,
    /// The deployment lifecycle no longer permits the workflow the route
    /// serves, even though the router that accepted the connection mounts it.
    RestoreNotAllowed,
}

impl PreBodyRejection {
    /// Returns the fixed response this rejection emits.
    pub(crate) fn response(self) -> BoundedResponse {
        match self {
            Self::BadRequest => {
                json_fixed_response(StatusCode::BAD_REQUEST, "{\"error\":\"bad_request\"}")
            }
            Self::RequestOriginDenied => json_fixed_response(
                StatusCode::FORBIDDEN,
                "{\"error\":\"request_origin_denied\"}",
            ),
            Self::RestoreTicketInvalid => json_fixed_response(
                StatusCode::FORBIDDEN,
                "{\"error\":\"restore_ticket_invalid\"}",
            ),
            Self::RestoreNotAllowed => {
                json_fixed_response(StatusCode::CONFLICT, "{\"error\":\"restore_not_allowed\"}")
            }
        }
    }
}

/// An opaque value a pre-body check hands to the route that admitted it.
///
/// The transport carries it without knowing what it is, so a route can pass a
/// claimed submission, a retained secret, or any other admission product to
/// its own handler without the listener naming that type. The value is dropped
/// with the request, so a client disconnect, an expired deadline, an
/// unreadable body, or any later failure destroys whatever it held.
#[derive(Clone)]
pub struct PreBodyGrantValue(Arc<dyn Any + Send + Sync>);

impl PreBodyGrantValue {
    /// Wraps a value for the route that produced it.
    #[must_use]
    pub fn new<T: Send + Sync + 'static>(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// Returns the wrapped value when it has the requested type.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

/// What a pre-body check returns when it accepts the request.
///
/// A grant may narrow the request's remaining time, but it can never widen it:
/// the listener takes the earlier of the profile's deadline and the granted
/// remainder, so an inherited budget bounds a later request without ever
/// extending the one the profile already allowed.
#[derive(Clone, Default)]
pub struct PreBodyGrant {
    value: Option<PreBodyGrantValue>,
    remaining: Option<Duration>,
}

impl PreBodyGrant {
    /// Accepts the request with nothing to carry forward.
    #[must_use]
    pub fn accepted() -> Self {
        Self::default()
    }

    /// Carries an opaque admission product to the route's handler.
    #[must_use]
    pub fn with_value(mut self, value: PreBodyGrantValue) -> Self {
        self.value = Some(value);
        self
    }

    /// Caps everything after this check at `remaining`.
    #[must_use]
    pub fn with_remaining_budget(mut self, remaining: Duration) -> Self {
        self.remaining = Some(remaining);
        self
    }

    fn remaining(&self) -> Option<Duration> {
        self.remaining
    }
}

/// Validation a route runs from the request head alone, before its body is
/// allocated.
pub trait PreBodyCheck: Send + Sync + 'static {
    /// Accepts or rejects the request without reading any body byte.
    fn check(
        &self,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection>;
}

/// Validation a route runs while already holding its admission permit.
///
/// The listener runs this after the permit is acquired and before any body is
/// allocated, so a route that must observe authoritative state under its own
/// mutation lane observes it exactly once, serialized, and still ahead of the
/// allocation it would otherwise have to pay for first.
pub trait AdmittedCheck: Send + Sync + 'static {
    /// Accepts or rejects the request under the acquired permit.
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<(), PreBodyRejection>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// Route registration and the mounted surface
// ---------------------------------------------------------------------------

/// Transport profile one mounted capability registers for one exact route.
#[derive(Clone)]
pub struct TransportRegistration {
    method: Method,
    target: &'static str,
    profile: TransportProfile,
    pre_body: Option<Arc<dyn PreBodyCheck>>,
    admission: Option<Arc<Semaphore>>,
    admitted: Option<Arc<dyn AdmittedCheck>>,
}

impl TransportRegistration {
    /// Registers `profile` for exactly one method and one canonical target.
    #[must_use]
    pub fn new(method: Method, target: &'static str, profile: TransportProfile) -> Self {
        Self {
            method,
            target,
            profile,
            pre_body: None,
            admission: None,
            admitted: None,
        }
    }

    /// Declares validation that runs before the body is allocated.
    #[must_use]
    pub fn with_pre_body_check(mut self, check: Arc<dyn PreBodyCheck>) -> Self {
        self.pre_body = Some(check);
        self
    }

    /// Declares the permit an admitted body must hold, bounding how many of
    /// this route's bodies may be resident at once.
    #[must_use]
    pub fn with_admission(mut self, permits: Arc<Semaphore>) -> Self {
        self.admission = Some(permits);
        self
    }

    /// Declares validation that runs under the acquired permit, still before
    /// the body is allocated.
    #[must_use]
    pub fn with_admitted_check(mut self, check: Arc<dyn AdmittedCheck>) -> Self {
        self.admitted = Some(check);
        self
    }
}

/// A capability's router mount and its transport registration as one value.
///
/// A registration reaches a surface only together with the mount that serves
/// it, so a registration cannot describe a route the surface did not mount.
pub struct TransportCapability {
    registration: TransportRegistration,
    mount: Box<dyn FnOnce(Router) -> Router>,
}

impl TransportCapability {
    /// Pairs a registration with the mount that serves the same route.
    #[must_use]
    pub fn new<M>(registration: TransportRegistration, mount: M) -> Self
    where
        M: FnOnce(Router) -> Router + 'static,
    {
        Self {
            registration,
            mount: Box::new(mount),
        }
    }
}

/// The transport registrations one mounted surface serves.
#[derive(Clone, Default)]
pub struct TransportRegistry {
    registrations: Vec<TransportRegistration>,
}

impl TransportRegistry {
    /// Reports whether this surface registered any non-default profile.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registrations.is_empty()
    }

    /// Selects the registration for an exact canonical target and method.
    ///
    /// A query string, an absolute-form request target, a percent-encoded
    /// separator, a dot segment, a trailing slash, a prefix, a longer target,
    /// and any other method all fail to match and therefore receive the
    /// default profile.
    fn selected_for(&self, method: &Method, uri: &Uri) -> Selection {
        if uri.scheme().is_some() || uri.authority().is_some() || uri.query().is_some() {
            return Selection::default();
        }
        let target = uri.path();
        self.registrations
            .iter()
            .find(|registration| registration.target == target && registration.method == *method)
            .map_or_else(Selection::default, Selection::from_registration)
    }
}

/// The router and the transport registrations the listener serves together.
///
/// The listener snapshots this whole value when it accepts a connection, so the
/// router and the registrations are swapped as one unit and can never disagree.
#[derive(Clone)]
pub struct MountedSurface {
    router: Router,
    registry: TransportRegistry,
}

impl MountedSurface {
    /// Serves `router` with no registration, so every request receives the
    /// default profile.
    #[must_use]
    pub fn without_registrations(router: Router) -> Self {
        Self {
            router,
            registry: TransportRegistry::default(),
        }
    }

    /// Mounts a capability's route and adds its registration in one step.
    #[must_use]
    pub fn with_capability(mut self, capability: TransportCapability) -> Self {
        self.router = (capability.mount)(self.router);
        self.registry.registrations.push(capability.registration);
        self
    }

    /// Returns the registrations that travel with this surface's router.
    pub(crate) fn registry(&self) -> &TransportRegistry {
        &self.registry
    }

    /// Consumes the surface and returns the router it mounted.
    pub(crate) fn into_router(self) -> Router {
        self.router
    }

    #[cfg(test)]
    pub(crate) fn router(&self) -> &Router {
        &self.router
    }
}

/// The registration values one classified request resolved to.
#[derive(Clone)]
struct Selection {
    profile: TransportProfile,
    pre_body: Option<Arc<dyn PreBodyCheck>>,
    admission: Option<Arc<Semaphore>>,
    admitted: Option<Arc<dyn AdmittedCheck>>,
}

impl Selection {
    fn from_registration(registration: &TransportRegistration) -> Self {
        Self {
            profile: registration.profile,
            pre_body: registration.pre_body.clone(),
            admission: registration.admission.clone(),
            admitted: registration.admitted.clone(),
        }
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            profile: TransportProfile::DEFAULT,
            pre_body: None,
            admission: None,
            admitted: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Admitted body permit
// ---------------------------------------------------------------------------

/// The route admission permit an admitted request carries to its handler.
///
/// The listener inserts it into the request extensions, so downstream work runs
/// under the permit admission already acquired and never reacquires it. The
/// permit is released when the request, and every clone of this handle, is
/// dropped.
#[derive(Clone)]
pub struct BodyAdmission(Arc<OwnedSemaphorePermit>);

impl BodyAdmission {
    /// Wraps a permit already acquired from a route's admission lane.
    ///
    /// In production only [`Validated::acquire`] produces this. A test that
    /// drives an admitted handler directly acquires the same lane first, so it
    /// runs under the route's real concurrency bound rather than beside it.
    #[cfg(test)]
    pub(crate) fn from_permit(permit: OwnedSemaphorePermit) -> Self {
        Self(Arc::new(permit))
    }

    /// Returns the permit this request was admitted under.
    ///
    /// Downstream work takes this handle from the request extensions instead of
    /// acquiring the route's permit again. The permit is released once this
    /// handle and every clone of it have been dropped.
    pub fn permit(&self) -> &OwnedSemaphorePermit {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Ordered admission chain
// ---------------------------------------------------------------------------

/// Stage 1: a request head read within its own absolute deadline.
///
/// Only the listener's head reader produces this value, so no later stage can
/// run against a head that was never completely read.
pub(crate) struct HeadRead {
    request: Request,
}

impl HeadRead {
    pub(crate) fn new(request: Request) -> Self {
        Self { request }
    }

    pub(crate) fn method(&self) -> &Method {
        self.request.method()
    }

    /// Stage 2: per-source rate admission.
    ///
    /// Returns the unadmitted head on rejection so the caller can still frame
    /// its fixed response correctly.
    pub(crate) fn admit_rate(
        self,
        limiter: &RateLimiter,
        source: IpAddr,
        now: Instant,
    ) -> Result<RateAdmitted, Box<Self>> {
        if limiter.allows(source, now) {
            Ok(RateAdmitted {
                request: self.request,
            })
        } else {
            Err(Box::new(self))
        }
    }
}

/// Stage 2 output: rate-admitted and not yet classified.
pub(crate) struct RateAdmitted {
    request: Request,
}

impl RateAdmitted {
    /// Stage 3: exact-target, exact-method classification against the mounted
    /// registry that travels with the router serving this connection.
    pub(crate) fn classify(self, registry: &TransportRegistry) -> Classified {
        let selection = registry.selected_for(self.request.method(), self.request.uri());
        Classified {
            request: self.request,
            selection,
        }
    }
}

/// Stage 3 output: the request and the profile its route registered.
pub(crate) struct Classified {
    request: Request,
    selection: Selection,
}

impl Classified {
    #[cfg(test)]
    pub(crate) fn profile(&self) -> TransportProfile {
        self.selection.profile
    }

    /// Stage 4: request framing, bounded by the classified profile.
    pub(crate) fn check_framing(self) -> Result<Framed, RequestReadError> {
        let body_length = declared_request_body_length(
            self.request.method(),
            self.request.headers(),
            self.selection.profile.max_body_bytes(),
        )?;
        Ok(Framed {
            request: self.request,
            selection: self.selection,
            body_length,
        })
    }
}

/// Stage 4 output: framing accepted and the exact declared body length known.
pub(crate) struct Framed {
    request: Request,
    selection: Selection,
    body_length: Option<usize>,
}

impl Framed {
    /// Stage 5: the route's registered pre-body validation.
    pub(crate) fn validate(self) -> Result<Validated, PreBodyRejection> {
        let grant = match &self.selection.pre_body {
            Some(check) => check.check(
                self.request.method(),
                self.request.uri(),
                self.request.headers(),
            )?,
            None => PreBodyGrant::accepted(),
        };
        Ok(Validated {
            request: self.request,
            selection: self.selection,
            body_length: self.body_length,
            grant,
        })
    }
}

/// Stage 5 output: every cheap rejection has already run.
pub(crate) struct Validated {
    request: Request,
    selection: Selection,
    body_length: Option<usize>,
    grant: PreBodyGrant,
}

impl Validated {
    /// Returns the time the pre-body check capped this request at, if any.
    pub(crate) fn remaining_budget(&self) -> Option<Duration> {
        self.grant.remaining()
    }

    /// Stage 6: acquire the route's admission permit, if it declared one, and
    /// run its registered admitted validation while holding it.
    ///
    /// A rejection here drops the permit and the pre-body grant together, so a
    /// request denied under the permit releases the lane and destroys whatever
    /// its pre-body check had claimed.
    pub(crate) async fn acquire(self) -> Result<Admitted, PreBodyRejection> {
        let admission = match &self.selection.admission {
            Some(permits) => Arc::clone(permits)
                .acquire_owned()
                .await
                .ok()
                .map(|permit| BodyAdmission(Arc::new(permit))),
            None => None,
        };
        if let Some(check) = &self.selection.admitted {
            check.check().await?;
        }
        Ok(Admitted {
            request: self.request,
            profile: self.selection.profile,
            body_length: self.body_length,
            admission,
            grant: self.grant,
        })
    }
}

/// Stage 6 output: the only value from which a request body may be allocated.
pub(crate) struct Admitted {
    request: Request,
    profile: TransportProfile,
    body_length: Option<usize>,
    admission: Option<BodyAdmission>,
    grant: PreBodyGrant,
}

impl Admitted {
    pub(crate) fn profile(&self) -> TransportProfile {
        self.profile
    }

    /// Stage 7: allocate the declared body fallibly and read exactly it.
    ///
    /// The allocation happens here and nowhere else, after every stage that can
    /// reject the request cheaply has already run.
    pub(crate) async fn read_body<S>(mut self, stream: &mut S) -> Result<Request, RequestReadError>
    where
        S: AsyncRead + Unpin,
    {
        if let Some(admission) = self.admission {
            self.request.extensions_mut().insert(admission);
        }
        if let Some(value) = self.grant.value {
            self.request.extensions_mut().insert(value);
        }
        let Some(length) = self.body_length else {
            return Ok(self.request);
        };
        let mut body = allocate_body(length)?;
        stream
            .read_exact(&mut body)
            .await
            .map_err(|_| RequestReadError::Invalid)?;
        if stream_has_pending_bytes(stream).await {
            return Err(RequestReadError::Invalid);
        }
        *self.request.body_mut() = Body::from(body);
        Ok(self.request)
    }
}

/// Reserves the declared body without aborting the process on failure.
///
/// A large admitted body must answer with a bounded response when the host
/// cannot supply the memory, rather than terminating the Server.
pub(crate) fn allocate_body(length: usize) -> Result<Vec<u8>, RequestReadError> {
    let mut body = Vec::new();
    body.try_reserve_exact(length)
        .map_err(|_| RequestReadError::BodyUnavailable)?;
    body.resize(length, 0);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{
        http::{HeaderValue, header::CONTENT_LENGTH},
        response::Response,
        routing::put,
    };
    use tokio::{io::AsyncWriteExt, time::timeout};

    use super::{
        Admitted, BodyAdmission, Duration, HeadRead, Instant, IpAddr, MAX_REQUEST_BODY_BYTES,
        Method, MountedSurface, PreBodyCheck, PreBodyGrant, PreBodyRejection, RateLimiter, Request,
        RequestReadError, Router, Semaphore, TransportBudget, TransportCapability,
        TransportProfile, TransportRegistration, TransportRegistry, Uri, allocate_body,
    };

    /// The test-only capability every registration test mounts.
    const ADMITTED_TARGET: &str = "/api/v1/admitted";

    /// Large enough to prove the bound is route-selected rather than uniform.
    const ADMITTED_BODY_BYTES: usize = 256 * 1024 * 1024;

    fn admitted_profile() -> TransportProfile {
        TransportProfile::admitted(
            ADMITTED_BODY_BYTES,
            Duration::from_secs(120),
            Duration::from_secs(60),
        )
    }

    async fn admitted_handler() -> Response {
        Response::new(axum::body::Body::empty())
    }

    fn mount_admitted(router: Router) -> Router {
        router.route(ADMITTED_TARGET, put(admitted_handler))
    }

    fn capability(registration: TransportRegistration) -> TransportCapability {
        TransportCapability::new(registration, mount_admitted)
    }

    fn registration() -> TransportRegistration {
        TransportRegistration::new(Method::PUT, ADMITTED_TARGET, admitted_profile())
    }

    fn surface_with(registration: TransportRegistration) -> MountedSurface {
        MountedSurface::without_registrations(Router::new())
            .with_capability(capability(registration))
    }

    fn head(method: &str, target: &str, content_length: Option<usize>) -> HeadRead {
        let mut request = Request::new(axum::body::Body::empty());
        *request.method_mut() = Method::from_bytes(method.as_bytes()).unwrap();
        *request.uri_mut() = target.parse::<Uri>().unwrap();
        if let Some(length) = content_length {
            request.headers_mut().insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&length.to_string()).unwrap(),
            );
        }
        HeadRead::new(request)
    }

    /// Rate admission is a real stage of the chain, so tests pass through a
    /// fresh limiter rather than bypassing it.
    fn rate_admitted(head: HeadRead) -> super::RateAdmitted {
        head.admit_rate(
            &RateLimiter::new(),
            IpAddr::from([127, 0, 0, 1]),
            Instant::now(),
        )
        .unwrap_or_else(|_| panic!("a fresh limiter must admit the first request"))
    }

    fn profile_for(registry: &TransportRegistry, method: &str, target: &str) -> TransportProfile {
        rate_admitted(head(method, target, None))
            .classify(registry)
            .profile()
    }

    #[test]
    fn the_default_profile_preserves_the_listener_wide_bounds() {
        assert_eq!(
            TransportProfile::DEFAULT.max_body_bytes(),
            MAX_REQUEST_BODY_BYTES
        );
        assert_eq!(
            TransportProfile::DEFAULT.budget(),
            TransportBudget::SharedWithHead
        );
    }

    #[test]
    fn classification_requires_the_exact_canonical_target_and_method() {
        let surface = surface_with(registration());
        let registry = surface.registry();

        assert_eq!(
            profile_for(registry, "PUT", ADMITTED_TARGET),
            admitted_profile(),
            "the exact canonical target and method"
        );

        for (name, method, target) in [
            ("query string", "PUT", "/api/v1/admitted?upload=1"),
            ("empty query string", "PUT", "/api/v1/admitted?"),
            (
                "absolute-form request target",
                "PUT",
                "https://127.0.0.1:8443/api/v1/admitted",
            ),
            ("percent-encoded separator", "PUT", "/api/v1%2Fadmitted"),
            ("percent-encoded segment", "PUT", "/api/v1/%61dmitted"),
            ("dot segment", "PUT", "/api/v1/../api/v1/admitted"),
            ("single dot segment", "PUT", "/api/v1/./admitted"),
            ("trailing slash", "PUT", "/api/v1/admitted/"),
            ("prefix", "PUT", "/api/v1/admitte"),
            ("longer target", "PUT", "/api/v1/admitted/part"),
            ("double slash", "PUT", "//api/v1/admitted"),
            ("other method", "GET", ADMITTED_TARGET),
            ("head method", "HEAD", ADMITTED_TARGET),
            ("post method", "POST", ADMITTED_TARGET),
        ] {
            assert_eq!(
                profile_for(registry, method, target),
                TransportProfile::DEFAULT,
                "{name}"
            );
        }
    }

    #[test]
    fn an_unmounted_registration_cannot_grant_the_admitted_profile() {
        let unmounted = MountedSurface::without_registrations(mount_admitted(Router::new()));
        assert!(unmounted.registry().is_empty());
        assert_eq!(
            profile_for(unmounted.registry(), "PUT", ADMITTED_TARGET),
            TransportProfile::DEFAULT,
        );
    }

    #[test]
    fn framing_bounds_the_body_by_the_classified_profile() {
        let surface = surface_with(registration());
        let default_surface = MountedSurface::without_registrations(Router::new());

        let admitted = rate_admitted(head(
            "PUT",
            ADMITTED_TARGET,
            Some(MAX_REQUEST_BODY_BYTES + 1),
        ))
        .classify(surface.registry())
        .check_framing();
        assert!(admitted.is_ok(), "the admitted profile bounds far higher");

        let default = rate_admitted(head(
            "PUT",
            ADMITTED_TARGET,
            Some(MAX_REQUEST_BODY_BYTES + 1),
        ))
        .classify(default_surface.registry())
        .check_framing();
        assert!(
            matches!(default, Err(RequestReadError::Invalid)),
            "the default profile keeps the one KiB bound"
        );
    }

    /// A check that records whether it ran and always rejects.
    struct RecordingCheck {
        calls: Arc<AtomicUsize>,
        rejection: PreBodyRejection,
    }

    impl PreBodyCheck for RecordingCheck {
        fn check(
            &self,
            _method: &Method,
            _uri: &Uri,
            _headers: &super::HeaderMap,
        ) -> Result<PreBodyGrant, PreBodyRejection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(self.rejection)
        }
    }

    use std::sync::Arc;

    #[tokio::test]
    async fn every_pre_body_failure_rejects_before_the_permit_and_the_allocation() {
        for rejection in [
            PreBodyRejection::BadRequest,
            PreBodyRejection::RequestOriginDenied,
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let permits = Arc::new(Semaphore::new(1));
            let surface = surface_with(
                registration()
                    .with_pre_body_check(Arc::new(RecordingCheck {
                        calls: Arc::clone(&calls),
                        rejection,
                    }))
                    .with_admission(Arc::clone(&permits)),
            );

            let outcome = rate_admitted(head("PUT", ADMITTED_TARGET, Some(64)))
                .classify(surface.registry())
                .check_framing()
                .expect("framing accepts the declared length")
                .validate();

            assert!(matches!(outcome, Err(returned) if returned == rejection));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                permits.available_permits(),
                1,
                "a rejected request must never take the route's permit"
            );
        }
    }

    #[tokio::test]
    async fn a_framing_failure_never_reaches_the_pre_body_check_or_the_permit() {
        let calls = Arc::new(AtomicUsize::new(0));
        let permits = Arc::new(Semaphore::new(1));
        let surface = surface_with(
            registration()
                .with_pre_body_check(Arc::new(RecordingCheck {
                    calls: Arc::clone(&calls),
                    rejection: PreBodyRejection::BadRequest,
                }))
                .with_admission(Arc::clone(&permits)),
        );

        let mut request = Request::new(axum::body::Body::empty());
        *request.method_mut() = Method::PUT;
        *request.uri_mut() = ADMITTED_TARGET.parse::<Uri>().unwrap();
        request
            .headers_mut()
            .insert("transfer-encoding", HeaderValue::from_static("chunked"));

        let outcome = rate_admitted(HeadRead::new(request))
            .classify(surface.registry())
            .check_framing();

        assert!(matches!(outcome, Err(RequestReadError::Invalid)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(permits.available_permits(), 1);
    }

    #[tokio::test]
    async fn a_declared_body_is_allocated_only_after_the_permit_is_held() {
        let permits = Arc::new(Semaphore::new(1));
        let surface = surface_with(registration().with_admission(Arc::clone(&permits)));

        let validated = rate_admitted(head("PUT", ADMITTED_TARGET, Some(4)))
            .classify(surface.registry())
            .check_framing()
            .unwrap()
            .validate()
            .unwrap_or_else(|_| panic!("no pre-body check is registered"));
        assert_eq!(permits.available_permits(), 1);

        let admitted = validated
            .acquire()
            .await
            .unwrap_or_else(|_| panic!("no admitted check is registered"));
        assert_eq!(
            permits.available_permits(),
            0,
            "the permit is held before any allocation"
        );

        let (mut client, mut server) = tokio::io::duplex(8);
        client.write_all(b"body").await.unwrap();
        client.shutdown().await.unwrap();
        let request = admitted.read_body(&mut server).await.unwrap();
        assert!(
            request.extensions().get::<BodyAdmission>().is_some(),
            "the handler receives the permit instead of reacquiring it"
        );
        assert_eq!(permits.available_permits(), 0);

        drop(request);
        assert_eq!(permits.available_permits(), 1);
    }

    #[tokio::test]
    async fn concurrent_admitted_bodies_are_bounded_to_the_declared_permit_count() {
        let permits = Arc::new(Semaphore::new(1));
        let surface = surface_with(registration().with_admission(Arc::clone(&permits)));

        let admit = |registry: &TransportRegistry| {
            rate_admitted(head("PUT", ADMITTED_TARGET, Some(4)))
                .classify(registry)
                .check_framing()
                .unwrap()
                .validate()
                .unwrap_or_else(|_| panic!("no pre-body check is registered"))
        };

        let first = admit(surface.registry())
            .acquire()
            .await
            .unwrap_or_else(|_| panic!("no admitted check is registered"));
        let second = admit(surface.registry());
        let third = admit(surface.registry());

        let mut pending = tokio::spawn(async move { second.acquire().await });
        assert!(
            timeout(Duration::from_millis(100), &mut pending)
                .await
                .is_err(),
            "a second admitted body must wait for the first"
        );
        assert_eq!(permits.available_permits(), 0);

        drop(first);
        let second: Admitted = pending
            .await
            .unwrap()
            .unwrap_or_else(|_| panic!("no admitted check is registered"));
        assert_eq!(permits.available_permits(), 0);

        let mut pending = tokio::spawn(async move { third.acquire().await });
        assert!(
            timeout(Duration::from_millis(100), &mut pending)
                .await
                .is_err(),
            "only one admitted body is resident at a time"
        );
        drop(second);
        let third = pending.await.unwrap();
        drop(third);
        assert_eq!(permits.available_permits(), 1);
    }

    #[test]
    fn an_unsatisfiable_allocation_returns_a_bounded_error() {
        assert!(matches!(
            allocate_body(usize::MAX),
            Err(RequestReadError::BodyUnavailable)
        ));
        assert_eq!(allocate_body(4).unwrap().len(), 4);
        assert!(allocate_body(0).unwrap().is_empty());
    }

    #[test]
    fn an_admitted_body_never_extends_the_head_deadline() {
        let started = tokio::time::Instant::now();
        let head_deadline = started + Duration::from_secs(5);
        let admitted_at = started + Duration::from_secs(1);

        assert_eq!(
            TransportProfile::DEFAULT.body_deadline(head_deadline, admitted_at),
            head_deadline
        );
        assert_eq!(
            TransportProfile::DEFAULT.processing_deadline(
                started,
                Duration::from_secs(10),
                head_deadline
            ),
            started + Duration::from_secs(10)
        );

        let profile = admitted_profile();
        assert_eq!(
            profile.body_deadline(head_deadline, admitted_at),
            admitted_at + Duration::from_secs(120),
            "an admitted body measures its own budget from admission"
        );
        assert_eq!(
            profile.processing_deadline(
                started,
                Duration::from_secs(10),
                admitted_at + Duration::from_secs(120)
            ),
            admitted_at + Duration::from_secs(180)
        );
    }
}

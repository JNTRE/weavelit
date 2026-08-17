#![forbid(unsafe_code)]

//! Group-based authorization for Human Users.
//!
//! This crate turns the Application Database's narrow authorization projection
//! into effective grants and decides one request against the catalogued
//! component enablement. It owns nothing else: it does not authenticate, read
//! or write persisted state, select a Service Connection, build a log record,
//! or touch transport.
//!
//! Default-deny is structural rather than conventional. A decision returns a
//! proof value whose fields and constructor are private to this crate, so the
//! single successful branch of each evaluator is the only place a proof can
//! come from and a caller cannot mint one. The two decisions are also
//! deliberately separate: the User Plane evaluator receives only
//! [`OperationalGrants`] and therefore cannot read the Server Administration
//! Permission at all, so "an Administrator implies Operation grants" is not a
//! statement this crate can express.

mod catalog;
mod decision;
mod grants;

pub use catalog::{
    AuthorizationCatalog, CatalogError, ClientModuleDeclaration, OperationDeclaration, Plane,
    ServiceModuleDeclaration,
};
pub use decision::{
    AdministrationRequest, AuthorizationDenied, AuthorizedAdministration, AuthorizedOperation,
    UserOperationRequest, authorize_administration, authorize_user_operation,
};
pub use grants::{EffectiveHumanGrants, OperationalGrants, ServerAdministrationPermission};

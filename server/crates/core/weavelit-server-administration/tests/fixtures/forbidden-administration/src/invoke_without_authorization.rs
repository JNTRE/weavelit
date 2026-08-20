use std::time::Duration;

use weavelit_server_administration::{
    AccountAdministrationRead, AdministrationAction, AdministrationClock, AdministrationPlane,
    AdministrationRequest, ComponentEnablementSource,
};
use weavelit_server_authorization::AuthorizationDenied;
use weavelit_server_database::ComponentEnablement;

struct Clock;

impl AdministrationClock for Clock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

struct Enablement;

impl ComponentEnablementSource for Enablement {
    fn load_component_enablement(
        &mut self,
    ) -> Result<ComponentEnablement, AuthorizationDenied> {
        Ok(ComponentEnablement::default())
    }
}

fn bypass(plane: &mut AdministrationPlane<Clock, Enablement>) {
    let _ = plane.authorize(
        (),
        AdministrationRequest::new(AdministrationAction::Account(
            AccountAdministrationRead::List,
        )),
    );
}

fn main() {}
import type { JSX } from 'react';

import { useDeploymentStatus, type StatusViewState } from './use-deployment-status';

const LOADING_MESSAGE = 'Checking the deployment status.';
const SELECTED_MESSAGE = 'An Application Database is selected for this deployment.';
const UNSELECTED_MESSAGE = 'No Application Database is selected for this deployment.';
const UNAVAILABLE_MESSAGE = 'The deployment status is unavailable.';

function statusMessage(state: StatusViewState): string {
  switch (state.kind) {
    case 'loading':
      return LOADING_MESSAGE;
    case 'available':
      return state.status.databaseSelected ? SELECTED_MESSAGE : UNSELECTED_MESSAGE;
    case 'unavailable':
      return UNAVAILABLE_MESSAGE;
  }
}

/** Root application shell for the restricted pre-operational Web UI. */
export function ApplicationShell(): JSX.Element {
  const { state } = useDeploymentStatus();

  return (
    <main className="shell">
      <h1 className="shell__title">Weavelit Server</h1>
      <p className="shell__subtitle">Setup</p>
      <p className="shell__status" data-status-state={state.kind} role="status" aria-live="polite">
        {statusMessage(state)}
      </p>
    </main>
  );
}

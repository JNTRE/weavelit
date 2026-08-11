import { useCallback, useState, type JSX } from "react";

import { selectSqliteDatabase } from "../api/weavelit-init-database-selection";
// Design system tokens for component styling integration
import type {} from "../design-system/tokens";
import {
  useDeploymentStatus,
  type StatusViewState,
} from "../hooks/weavelit-init-deployment-status";
import { RestoreSubmissionForm } from "./weavelit-init-restore-form";
import { LoginPanel } from "./weavelit-login-form";

const LOADING_MESSAGE = "Checking the deployment status.";
const SELECTED_MESSAGE = "An Application Database is selected for this deployment.";
const UNSELECTED_MESSAGE = "No Application Database is selected for this deployment.";
const UNAVAILABLE_MESSAGE = "The deployment status is unavailable.";

const SELECTION_HEADING = "Application Database";
const SELECTION_DESCRIPTION =
  "Select SQLite to store this deployment in a Server-managed SQLite database.";
const SELECTION_ACTION_LABEL = "Select SQLite";
const SELECTION_FAILED_MESSAGE = "The Application Database was not selected. Try again.";

/** Presentation state of an Application Database selection submission. */
type SelectionViewState = "idle" | "submitting" | "failed";

function statusMessage(state: StatusViewState): string {
  switch (state.kind) {
    case "loading":
      return LOADING_MESSAGE;
    case "available":
      return state.status.databaseSelected ? SELECTED_MESSAGE : UNSELECTED_MESSAGE;
    case "unavailable":
      return UNAVAILABLE_MESSAGE;
  }
}

/** Root application shell for the restricted pre-operational Web UI. */
export function ApplicationShell(): JSX.Element {
  const { state, applyStatus } = useDeploymentStatus();
  const [selection, setSelection] = useState<SelectionViewState>("idle");

  const submit = useCallback(() => {
    setSelection("submitting");
    void selectSqliteDatabase().then(
      (status) => {
        setSelection("idle");
        // The response carries the authoritative projection, so the shell
        // adopts it instead of issuing a second status request.
        applyStatus(status);
      },
      () => {
        setSelection("failed");
      },
    );
  }, [applyStatus]);

  const offerSelection = state.kind === "available" && !state.status.databaseSelected;
  // Restore becomes eligible exactly when a database has been selected, and the
  // status projection stops being served at all once the deployment is sealed,
  // so the same condition withdraws the form after activation.
  const offerRestore = state.kind === "available" && state.status.databaseSelected;
  // The pre-operational status route is served only before sealing, so its
  // absence is the signal that this deployment may now be operational. The
  // panel confirms that against the Server's own authentication surface and
  // renders nothing when that surface is absent, so an unreachable Server does
  // not produce a sign-in form that could never succeed.
  const offerLogin = state.kind === "unavailable";

  return (
    <main className="shell">
      <h1 className="shell__title">Weavelit Server</h1>
      <p className="shell__subtitle">Setup</p>
      <p className="shell__status" data-status-state={state.kind} role="status" aria-live="polite">
        {statusMessage(state)}
      </p>
      {offerSelection ? (
        <section className="shell__selection" data-selection-state={selection}>
          <h2 className="shell__selection-title">{SELECTION_HEADING}</h2>
          <p className="shell__selection-description">{SELECTION_DESCRIPTION}</p>
          <button
            type="button"
            className="shell__selection-action"
            onClick={submit}
            disabled={selection === "submitting"}
          >
            {SELECTION_ACTION_LABEL}
          </button>
          {selection === "failed" ? (
            <p className="shell__selection-failure" role="alert">
              {SELECTION_FAILED_MESSAGE}
            </p>
          ) : null}
        </section>
      ) : null}
      {offerRestore ? <RestoreSubmissionForm /> : null}
      {offerLogin ? <LoginPanel /> : null}
    </main>
  );
}

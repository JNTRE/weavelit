import { useCallback, useState, type JSX } from "react";

import { selectSqliteDatabase } from "../api/weavelit-init-database-selection";
// Design system tokens for component styling integration
import type {} from "../design-system/tokens";
import {
  useDeploymentStatus,
  type StatusViewState,
} from "../hooks/weavelit-init-deployment-status";
import { RestoreSubmissionForm } from "./weavelit-init-restore-form";
import { InitWorkflow } from "./weavelit-init-workflow";
import { LoginPanel } from "./weavelit-login-form";

const LOADING_MESSAGE = "Checking the deployment status.";
const SELECTED_MESSAGE = "An Application Database is selected for this deployment.";
const UNSELECTED_MESSAGE = "No Application Database is selected for this deployment.";
const UNAVAILABLE_MESSAGE = "The deployment status is unavailable.";
const INITIALIZED_MESSAGE = "This deployment is initialized and now runs in normal operation.";

const CHOICE_HEADING = "First launch";
const CHOICE_DESCRIPTION =
  "Choose how this deployment becomes operational. These are mutually exclusive: only the path you choose is offered from here.";
const INIT_CHOICE_LABEL = "Set up a new deployment";
const INIT_CHOICE_DESCRIPTION =
  "Create the first Administrator and issue this deployment's one-time recovery key.";
const RESTORE_CHOICE_LABEL = "Restore from a backup";
const RESTORE_CHOICE_DESCRIPTION =
  "Replace this deployment's state from an encrypted backup and its existing recovery key.";

const SELECTION_HEADING = "Application Database";
const SELECTION_DESCRIPTION =
  "Select SQLite to store this deployment in a Server-managed SQLite database.";
const SELECTION_ACTION_LABEL = "Select SQLite";
const SELECTION_FAILED_MESSAGE = "The Application Database was not selected. Try again.";

/** Presentation state of an Application Database selection submission. */
type SelectionViewState = "idle" | "submitting" | "failed";

/** The mutually exclusive first-launch path a person has chosen, if any. */
type SetupChoice = "init" | "restore";

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
  const [choice, setChoice] = useState<SetupChoice | null>(null);
  const [initialized, setInitialized] = useState(false);

  const chooseInit = useCallback(() => {
    setChoice("init");
  }, []);
  const chooseRestore = useCallback(() => {
    setChoice("restore");
  }, []);
  // Init seals this deployment and withdraws the pre-operational surface, so
  // completion is adopted from the finalization response the page already holds
  // rather than from a status request that can no longer be served.
  const completeInit = useCallback(() => {
    setInitialized(true);
  }, []);

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

  const status = state.kind === "available" ? state.status : null;
  const databaseSelected = status?.databaseSelected === true;
  // Once finalization is confirmed the pre-operational surface is gone, so every
  // setup control is withdrawn regardless of the status this page last read.
  const settingUp = status !== null && !initialized;
  const offerChoice = settingUp && choice === null;
  // Both paths need an Application Database, so the existing selection surface
  // is reused after either choice rather than duplicated inside them.
  const offerSelection = settingUp && choice !== null && !databaseSelected;
  // Restore becomes eligible exactly when a database has been selected, and the
  // status projection stops being served at all once the deployment is sealed,
  // so the same condition withdraws the form after activation.
  const offerRestore = settingUp && choice === "restore" && databaseSelected;
  const offerInit = settingUp && choice === "init" && databaseSelected;
  // The pre-operational status route is served only before sealing, so its
  // absence is the signal that this deployment may now be operational. The
  // panel confirms that against the Server's own authentication surface and
  // renders nothing when that surface is absent, so an unreachable Server does
  // not produce a sign-in form that could never succeed.
  const offerLogin = state.kind === "unavailable" || initialized;

  return (
    <main className="shell">
      <h1 className="shell__title">Weavelit Server</h1>
      <p className="shell__subtitle">Setup</p>
      <p
        className="shell__status"
        data-status-state={initialized ? "initialized" : state.kind}
        role="status"
        aria-live="polite"
      >
        {initialized ? INITIALIZED_MESSAGE : statusMessage(state)}
      </p>
      {offerChoice ? (
        <section className="shell__choice" data-setup-choice="unchosen">
          <h2 className="shell__choice-title">{CHOICE_HEADING}</h2>
          <p className="shell__choice-description">{CHOICE_DESCRIPTION}</p>
          <button
            type="button"
            className="shell__choice-action"
            data-setup-path="init"
            onClick={chooseInit}
          >
            {INIT_CHOICE_LABEL}
          </button>
          <p className="shell__choice-detail">{INIT_CHOICE_DESCRIPTION}</p>
          <button
            type="button"
            className="shell__choice-action"
            data-setup-path="restore"
            onClick={chooseRestore}
          >
            {RESTORE_CHOICE_LABEL}
          </button>
          <p className="shell__choice-detail">{RESTORE_CHOICE_DESCRIPTION}</p>
        </section>
      ) : null}
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
      {offerInit ? <InitWorkflow onCompleted={completeInit} /> : null}
      {offerLogin ? <LoginPanel /> : null}
    </main>
  );
}

import { useCallback, useState, type ComponentType, type JSX } from "react";

import { selectSqliteDatabase } from "../api/weavelit-init-database-selection";
// Design system tokens for component styling integration
import type {} from "../design-system/tokens";
import {
  useDeploymentStatus,
  type StatusViewState,
} from "../hooks/weavelit-init-deployment-status";
import type { CredentialIssued } from "../api/weavelit-credential-issuance";
import { RestoreSubmissionForm } from "./weavelit-init-restore-form";
import { AccountsWorkspace, TemporaryPasswordDisclosure } from "./weavelit-accounts-workspace";
import { InitWorkflow } from "./weavelit-init-workflow";
import { LoginPanel } from "./weavelit-login-form";
import { PasswordChangeForm } from "./weavelit-password-change-form";

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

interface GroupsWorkspaceModule {
  GroupsWorkspace: ComponentType;
}

interface ConfigurationWorkspaceModule {
  ConfigurationWorkspace: ComponentType<{ readonly onSessionEnded?: () => void }>;
}

export interface ApplicationShellProps {
  loadGroupsWorkspace?: () => Promise<GroupsWorkspaceModule>;
  loadConfigurationWorkspace?: () => Promise<ConfigurationWorkspaceModule>;
}

function defaultGroupsWorkspaceLoader(): Promise<GroupsWorkspaceModule> {
  return import("./weavelit-groups-workspace");
}

function defaultConfigurationWorkspaceLoader(): Promise<ConfigurationWorkspaceModule> {
  return import("./weavelit-configuration-workspace");
}

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
export function ApplicationShell({
  loadGroupsWorkspace = defaultGroupsWorkspaceLoader,
  loadConfigurationWorkspace = defaultConfigurationWorkspaceLoader,
}: ApplicationShellProps = {}): JSX.Element {
  const { state, applyStatus } = useDeploymentStatus();
  const [selection, setSelection] = useState<SelectionViewState>("idle");
  const [choice, setChoice] = useState<SetupChoice | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [authenticatedPublicId, setAuthenticatedPublicId] = useState<string | null>(null);
  const [passwordChangeRequired, setPasswordChangeRequired] = useState(false);
  const [endedSessionDisclosure, setEndedSessionDisclosure] = useState<CredentialIssued | null>(
    null,
  );
  const [administrationView, setAdministrationView] = useState<
    "accounts" | "groups" | "configuration"
  >("accounts");
  const [groupsLoadState, setGroupsLoadState] = useState<"idle" | "loading" | "failed">("idle");
  const [GroupsWorkspace, setGroupsWorkspace] = useState<ComponentType | null>(null);
  const [configurationLoadState, setConfigurationLoadState] = useState<
    "idle" | "loading" | "failed"
  >("idle");
  const [ConfigurationWorkspace, setConfigurationWorkspace] = useState<ComponentType<{
    readonly onSessionEnded?: () => void;
  }> | null>(null);

  const chooseInit = useCallback(() => {
    setChoice("init");
  }, []);
  const chooseRestore = useCallback(() => {
    setChoice("restore");
  }, []);
  // Either first-launch path seals this deployment and withdraws the
  // pre-operational surface, so completion is adopted from the response the
  // page already holds rather than from a status request that can no longer be
  // served.
  const completeSetup = useCallback(() => {
    setInitialized(true);
  }, []);
  const completeAuthentication = useCallback((required: boolean, publicId: string) => {
    setEndedSessionDisclosure(null);
    setAuthenticatedPublicId(publicId);
    setPasswordChangeRequired(required);
    setAuthenticated(true);
  }, []);

  const completePasswordChange = useCallback(() => {
    setPasswordChangeRequired(false);
    setAuthenticated(true);
  }, []);

  const endAuthenticatedSession = useCallback((disclosure?: CredentialIssued) => {
    setEndedSessionDisclosure(disclosure ?? null);
    setAuthenticatedPublicId(null);
    setPasswordChangeRequired(false);
    setAuthenticated(false);
  }, []);

  const loadGroups = useCallback(() => {
    setGroupsLoadState("loading");
    void loadGroupsWorkspace().then(
      (module) => {
        setGroupsWorkspace(() => module.GroupsWorkspace);
      },
      () => {
        setGroupsLoadState("failed");
      },
    );
  }, [loadGroupsWorkspace]);

  const loadConfiguration = useCallback(() => {
    setConfigurationLoadState("loading");
    void loadConfigurationWorkspace().then(
      (module) => {
        setConfigurationWorkspace(() => module.ConfigurationWorkspace);
      },
      () => {
        setConfigurationLoadState("failed");
      },
    );
  }, [loadConfigurationWorkspace]);

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
  // Restore becomes eligible exactly when a database has been selected, and a
  // confirmed Restore withdraws the form through the same completion signal
  // Init uses rather than through a status projection that is no longer served.
  const offerRestore = settingUp && choice === "restore" && databaseSelected;
  const offerInit = settingUp && choice === "init" && databaseSelected;
  // The pre-operational status route is served only before sealing, so its
  // absence is the signal that this deployment may now be operational. The
  // panel confirms that against the Server's own authentication surface and
  // renders nothing when that surface is absent, so an unreachable Server does
  // not produce a sign-in form that could never succeed.
  const offerLogin = state.kind === "unavailable" || initialized;

  if (authenticated && authenticatedPublicId !== null) {
    return (
      <main className="shell shell--administration">
        <h1 className="shell__title">Weavelit Server</h1>
        <p className="shell__subtitle">Administration</p>
        {passwordChangeRequired ? (
          <PasswordChangeForm onCompleted={completePasswordChange} />
        ) : (
          <>
            <nav className="administration-nav" aria-label="Administration">
              <button
                type="button"
                aria-current={administrationView === "accounts" ? "page" : undefined}
                onClick={() => {
                  setAdministrationView("accounts");
                }}
              >
                Accounts
              </button>
              <button
                type="button"
                aria-current={administrationView === "groups" ? "page" : undefined}
                onClick={() => {
                  setAdministrationView("groups");
                  if (groupsLoadState === "idle") loadGroups();
                }}
              >
                Groups
              </button>
              <button
                type="button"
                aria-current={administrationView === "configuration" ? "page" : undefined}
                onClick={() => {
                  setAdministrationView("configuration");
                  if (configurationLoadState === "idle") loadConfiguration();
                }}
              >
                Configuration
              </button>
            </nav>
            {administrationView === "accounts" ? (
              <AccountsWorkspace
                currentAccountPublicId={authenticatedPublicId}
                onSessionEnded={endAuthenticatedSession}
              />
            ) : administrationView === "groups" ? (
              GroupsWorkspace !== null ? (
                <GroupsWorkspace />
              ) : (
                <section className="groups" aria-labelledby="groups-loading-title">
                  <h2 id="groups-loading-title" className="groups__title">
                    Groups
                  </h2>
                  {groupsLoadState === "failed" ? (
                    <>
                      <p role="alert">Groups could not be loaded.</p>
                      <button type="button" onClick={loadGroups}>
                        Retry
                      </button>
                    </>
                  ) : (
                    <p role="status" aria-live="polite">
                      Loading Groups.
                    </p>
                  )}
                </section>
              )
            ) : ConfigurationWorkspace !== null ? (
              <ConfigurationWorkspace onSessionEnded={endAuthenticatedSession} />
            ) : (
              <section className="configuration" aria-labelledby="configuration-loading-title">
                <h2 id="configuration-loading-title" className="accounts__title">
                  Configuration
                </h2>
                {configurationLoadState === "failed" ? (
                  <>
                    <p role="alert">Configuration could not be loaded.</p>
                    <button type="button" onClick={loadConfiguration}>
                      Retry
                    </button>
                  </>
                ) : (
                  <p role="status" aria-live="polite">
                    Loading Configuration.
                  </p>
                )}
              </section>
            )}
          </>
        )}
      </main>
    );
  }

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
      {offerRestore ? <RestoreSubmissionForm onCompleted={completeSetup} /> : null}
      {offerInit ? <InitWorkflow onCompleted={completeSetup} /> : null}
      {endedSessionDisclosure !== null ? (
        <TemporaryPasswordDisclosure disclosure={endedSessionDisclosure} />
      ) : null}
      {offerLogin ? <LoginPanel onAuthenticated={completeAuthentication} /> : null}
    </main>
  );
}

import { useCallback, useState, type ChangeEvent, type JSX } from "react";

import { probeSession } from "../api/weavelit-authentication";
import { deriveRecoveryKeyProof } from "../api/weavelit-init-recovery-proof";
import {
  InitFailedError,
  SQLITE_LOG_MODULE,
  UNREPORTED_FAILURE_CODE,
  finalizeInit,
  prepareRecoveryKey,
  type DeliveredRecoveryKey,
  type InitDetails,
} from "../api/weavelit-init-setup";

const INIT_HEADING = "Set up this deployment";
const INIT_DESCRIPTION =
  "Assign the System Log and the Audit Log, name the first Administrator, then save the one-time recovery key this deployment issues.";

const LOG_ASSIGNMENT_HEADING = "Log assignments";
const LOG_ASSIGNMENT_DESCRIPTION =
  "The System Log and the Audit Log are assigned separately. This build compiles in the SQLite Log Module only.";
const SYSTEM_LOG_LABEL = "System Log module";
const AUDIT_LOG_LABEL = "Audit Log module";
const UNASSIGNED_OPTION_LABEL = "Not assigned";
const SQLITE_OPTION_LABEL = "SQLite";

const ADMINISTRATOR_HEADING = "First Administrator";
const ADMINISTRATOR_DESCRIPTION =
  "This identity is the deployment's first Administrator. Its password is submitted with the request and is never stored by this page.";
const USERNAME_LABEL = "Username";
const DISPLAY_NAME_LABEL = "Display name (optional)";
const PASSWORD_LABEL = "Password";

const PREPARE_ACTION_LABEL = "Prepare recovery key";
const RETURN_ACTION_LABEL = "Return to review";
const PREPARING_MESSAGE = "Preparing the recovery key.";
const FINALIZING_MESSAGE = "Completing setup.";

const KEY_HEADING = "Recovery key";
const KEY_ONCE_WARNING =
  "This is the only time this recovery key is ever shown. Weavelit cannot display it again, cannot recover it, and cannot issue a replacement. Save it outside Weavelit now, before you continue.";
const KEY_PURPOSE_DESCRIPTION =
  "This key is required to restore this deployment from a backup. Without it, a backup of this deployment cannot be restored.";
const KEY_LABEL = "Recovery key for this deployment";
const COPY_ACTION_LABEL = "Copy recovery key";
const COPIED_MESSAGE = "The recovery key was copied to the clipboard.";
const COPY_FAILED_MESSAGE = "The recovery key could not be copied. Select it and copy it manually.";
const ACKNOWLEDGEMENT_LABEL = "I have saved this recovery key.";
const KEY_CONTINUE_LABEL = "Continue to review";

const REVIEW_HEADING = "Review";
const REVIEW_DESCRIPTION =
  "Confirm these details, then complete setup. The password and the recovery key are deliberately not repeated here.";
const REVIEW_USERNAME_LABEL = "Username";
const REVIEW_DISPLAY_NAME_LABEL = "Display name";
const REVIEW_SYSTEM_LOG_LABEL = "System Log";
const REVIEW_AUDIT_LOG_LABEL = "Audit Log";
const REVIEW_UNSET_VALUE = "Not set";
const REVIEW_BACK_LABEL = "Change details";
const FINALIZE_ACTION_LABEL = "Complete setup";

const CORRECTABLE_HEADING = "Setup was not accepted";
const CORRECTABLE_BEFORE_DELIVERY_MESSAGE =
  "Weavelit did not accept these details and no recovery key was issued. Correct the details below and try again.";
const CORRECTABLE_AFTER_DELIVERY_MESSAGE =
  "Weavelit did not accept this attempt. The recovery key you saved is still the key this deployment expects, so you do not need a new one and none will be issued. Correct the details below and try again with the same key.";
const CLOSED_HEADING = "This Server can no longer set up this deployment";
const CLOSED_MESSAGE =
  "Setup stopped and this Server will not resume it. No further attempt against this Server can succeed, this deployment was not initialized, and the recovery key shown earlier is not usable. Stop this Server, discard its state, and deploy a new Weavelit Server to start again.";
const INDETERMINATE_HEADING = "Setup did not report an outcome";
const INDETERMINATE_MESSAGE =
  "This attempt reported no outcome, so whether this deployment was initialized is not yet known. Keep the recovery key you saved: it is still the only key this deployment can be restored with, and none will be issued again. Check again to see whether setup completed, or try again with the same key.";
const INDETERMINATE_CHECKING_MESSAGE = "Checking whether setup completed.";
const RECHECK_ACTION_LABEL = "Check again";
const INDETERMINATE_RETRY_LABEL = "Try setup again";
const FAILURE_CODE_LABEL = "Reported code";

const SYSTEM_LOG_SELECT_ID = "weavelit-init-system-log";
const AUDIT_LOG_SELECT_ID = "weavelit-init-audit-log";
const USERNAME_INPUT_ID = "weavelit-init-username";
const DISPLAY_NAME_INPUT_ID = "weavelit-init-display-name";
const PASSWORD_INPUT_ID = "weavelit-init-password";
const RECOVERY_KEY_OUTPUT_ID = "weavelit-init-recovery-key";
const ACKNOWLEDGEMENT_INPUT_ID = "weavelit-init-acknowledgement";

/**
 * Field bounds that keep a serialized request inside the route's body bound.
 *
 * These are presentation bounds only. The Server enforces its own bounds and
 * nothing here is relied upon to keep an oversized or malformed request out.
 */
const USERNAME_MAX_LENGTH = 64;
const DISPLAY_NAME_MAX_LENGTH = 64;
const PASSWORD_MAX_LENGTH = 192;

/**
 * Rejections of the preparation route that leave this Server able to try again.
 *
 * The preparation route does not seal anything, so a rejection it reports has
 * not consumed the deployment. Any other code is treated as permanent, because
 * a route that has become absent or a deployment that is already initialized
 * cannot be corrected from this page.
 */
const CORRECTABLE_PREPARATION_CODES = ["bad_request", "service_unavailable"];

/**
 * Rejections of the finalization route that preserve the delivered key.
 *
 * The Server distinguishes an actionable failure, which releases the pending
 * attempt and leaves the same key valid for another try, from an internal
 * failure, which permanently fails this Server closed. `initialization_failed`
 * is reported for both, so it is treated as correctable: a retry either
 * succeeds, reports a correctable code again, or finds the route absent, and
 * that absence is what resolves it to the permanent case.
 */
const CORRECTABLE_FINALIZATION_CODES = [
  "bad_request",
  "recovery_key_confirmation_required",
  "recovery_key_confirmation_invalid",
  "initialization_failed",
];

/**
 * Codes a finalization reports when it establishes no outcome at all.
 *
 * `gateway_timeout` is written by the listener when it stops waiting for the
 * route, not by the route. Finalization work that had already reached a
 * blocking boundary is not cancelled by that, so the deployment may have gone
 * on to become operational after this page was answered. An attempt like this
 * is neither correctable nor permanently failed: it is unresolved, and the
 * delivered key must survive it until an operational surface settles it.
 *
 * A code is not the only way an attempt reports nothing, so this list is one
 * input to {@link indeterminateFinalization} rather than the whole test.
 */
const INDETERMINATE_FINALIZATION_CODES = ["gateway_timeout"];

/** Presentation state of the Init workflow. */
type InitViewState =
  | { readonly kind: "details"; readonly code: string | null }
  | { readonly kind: "preparing" }
  | { readonly kind: "key" }
  | { readonly kind: "review" }
  | { readonly kind: "finalizing" }
  | { readonly kind: "indeterminate"; readonly code: string; readonly checking: boolean }
  | { readonly kind: "closed"; readonly code: string };

function failureCode(reason: unknown): string {
  return reason instanceof InitFailedError ? reason.code : UNREPORTED_FAILURE_CODE;
}

/** Reports whether a failed attempt may be corrected and submitted again. */
function correctable(code: string, codes: readonly string[]): boolean {
  return codes.includes(code);
}

/**
 * Reports whether a finalization failure established no outcome whatsoever.
 *
 * The listener's timeout is one such result, and a finalization this page never
 * read an answer to is another: a rejected `fetch` may still have delivered the
 * request, and a completion body that was truncated before it arrived may still
 * have been written by a deployment that had already been sealed. The blocking
 * commit chain is not cancelled by any of them, so none of them proves this
 * deployment was not initialized with the delivered key.
 *
 * A rejection the route itself reported is excluded, however it is presented,
 * so a determinate failure is never dressed up as an unresolved one. So is a
 * failure raised before any request was issued, such as a proof that could not
 * be derived, which reports the same code without ever reaching the Server.
 */
function indeterminateFinalization(reason: unknown, code: string): boolean {
  return (
    INDETERMINATE_FINALIZATION_CODES.includes(code) ||
    (reason instanceof InitFailedError && reason.indeterminate)
  );
}

/** Props of the Init workflow. */
export interface InitWorkflowProps {
  /** Invoked once, after finalization has been confirmed by the Server. */
  readonly onCompleted: () => void;
}

/**
 * The first-launch Init workflow of the pre-operational Web UI.
 *
 * The password and the delivered recovery key are held in component state only.
 * Neither is written to a URL, a cookie, `localStorage`, `sessionStorage`, a
 * log, or any rendered failure message, and neither survives the submission it
 * drives: the password is cleared as soon as a finalization attempt settles, and
 * the key is dropped on completion and on any permanent failure. An attempt
 * that reported no outcome keeps the key in that same component memory until an
 * operational surface settles it, because dropping a key a deployment may still
 * be sealed with would destroy the only copy of it.
 *
 * Between key delivery and finalization the Server serves the finalization
 * route alone, so this component never reloads, re-requests status, or fetches
 * any further asset across that boundary. The already-loaded page is the only
 * thing that can complete the workflow.
 */
export function InitWorkflow({ onCompleted }: InitWorkflowProps): JSX.Element {
  const [systemLogModule, setSystemLogModule] = useState("");
  const [auditLogModule, setAuditLogModule] = useState("");
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [delivered, setDelivered] = useState<DeliveredRecoveryKey | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const [state, setState] = useState<InitViewState>({ kind: "details", code: null });

  const changeSystemLog = useCallback((event: ChangeEvent<HTMLSelectElement>) => {
    setSystemLogModule(event.target.value);
  }, []);
  const changeAuditLog = useCallback((event: ChangeEvent<HTMLSelectElement>) => {
    setAuditLogModule(event.target.value);
  }, []);
  const changeUsername = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setUsername(event.target.value);
  }, []);
  const changeDisplayName = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setDisplayName(event.target.value);
  }, []);
  const changePassword = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setPassword(event.target.value);
  }, []);
  const changeAcknowledgement = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setAcknowledged(event.target.checked);
  }, []);

  const complete =
    systemLogModule !== "" && auditLogModule !== "" && username !== "" && password !== "";

  const prepare = useCallback(() => {
    if (!complete) {
      return;
    }
    const submission: InitDetails = {
      username,
      displayName,
      password,
      systemLogModule,
      auditLogModule,
    };
    setState({ kind: "preparing" });
    void prepareRecoveryKey(submission).then(
      (issued) => {
        setDelivered(issued);
        setAcknowledged(false);
        setCopy("idle");
        setState({ kind: "key" });
      },
      (reason: unknown) => {
        const code = failureCode(reason);
        setState(
          correctable(code, CORRECTABLE_PREPARATION_CODES)
            ? { kind: "details", code }
            : { kind: "closed", code },
        );
      },
    );
  }, [complete, username, displayName, password, systemLogModule, auditLogModule]);

  const reconcile = useCallback(
    (code: string) => {
      setState({ kind: "indeterminate", code, checking: true });
      void probeSession().then((probe) => {
        if (probe.kind === "absent") {
          // No authentication surface is served, which distinguishes nothing:
          // finalization may still be running, or may never have committed.
          // The key is kept and the operator decides when to look again.
          setState({ kind: "indeterminate", code, checking: false });
          return;
        }
        // An identity or an unauthenticated challenge both prove the same
        // thing: normal operation was published, so this deployment was
        // initialized and sealed with the key that was delivered here.
        setDelivered(null);
        onCompleted();
      });
    },
    [onCompleted],
  );

  const finalize = useCallback(() => {
    if (delivered === null || !complete) {
      return;
    }
    const submission: InitDetails = {
      username,
      displayName,
      password,
      systemLogModule,
      auditLogModule,
    };
    setState({ kind: "finalizing" });
    void deriveRecoveryKeyProof(delivered.recoveryKey, delivered.deliveryNonce)
      .then((proof) => finalizeInit(submission, proof))
      .then(
        () => {
          setPassword("");
          setDelivered(null);
          onCompleted();
        },
        (reason: unknown) => {
          // The password does not outlive the attempt it drove, whichever way
          // that attempt settled.
          setPassword("");
          const code = failureCode(reason);
          if (indeterminateFinalization(reason, code)) {
            // Deliberately keeps the delivered key: this attempt may already
            // have initialized and sealed this deployment with it.
            reconcile(code);
            return;
          }
          if (correctable(code, CORRECTABLE_FINALIZATION_CODES)) {
            // The Server released the attempt and still expects the same key,
            // so the delivered key is deliberately retained for the retry.
            setState({ kind: "details", code });
            return;
          }
          setDelivered(null);
          setState({ kind: "closed", code });
        },
      );
  }, [
    complete,
    delivered,
    onCompleted,
    reconcile,
    username,
    displayName,
    password,
    systemLogModule,
    auditLogModule,
  ]);

  const copyKey = useCallback(() => {
    if (delivered === null) {
      return;
    }
    // Clipboard access is absent in some browsing contexts, so its absence is
    // presented rather than assumed away. The DOM types declare it as always
    // present, which is why the narrowing is written explicitly here.
    const clipboard = navigator.clipboard as Clipboard | undefined;
    if (clipboard === undefined) {
      setCopy("failed");
      return;
    }
    void clipboard.writeText(delivered.recoveryKey).then(
      () => {
        setCopy("copied");
      },
      () => {
        setCopy("failed");
      },
    );
  }, [delivered]);

  const review = useCallback(() => {
    setState({ kind: "review" });
  }, []);
  const edit = useCallback(() => {
    setState({ kind: "details", code: null });
  }, []);
  const recheck = useCallback(() => {
    if (state.kind === "indeterminate") {
      reconcile(state.code);
    }
  }, [reconcile, state]);

  const busy = state.kind === "preparing" || state.kind === "finalizing";
  const editing = state.kind === "details" || state.kind === "preparing";

  if (state.kind === "indeterminate") {
    return (
      <section className="shell__init" data-init-state={state.kind}>
        <h2 className="shell__init-title">{INIT_HEADING}</h2>
        <div className="shell__init-indeterminate" role="alert" data-init-error={state.code}>
          <h3 className="shell__init-failure-title">{INDETERMINATE_HEADING}</h3>
          <p className="shell__init-failure-message">{INDETERMINATE_MESSAGE}</p>
          <p className="shell__init-failure-code">
            {FAILURE_CODE_LABEL}: {state.code}
          </p>
          {state.checking ? (
            <p className="shell__init-progress" role="status">
              {INDETERMINATE_CHECKING_MESSAGE}
            </p>
          ) : (
            <>
              <button type="button" className="shell__init-action" onClick={recheck}>
                {RECHECK_ACTION_LABEL}
              </button>
              <button type="button" className="shell__init-secondary" onClick={edit}>
                {INDETERMINATE_RETRY_LABEL}
              </button>
            </>
          )}
        </div>
      </section>
    );
  }

  if (state.kind === "closed") {
    return (
      <section className="shell__init" data-init-state={state.kind}>
        <h2 className="shell__init-title">{INIT_HEADING}</h2>
        <div className="shell__init-closed" role="alert" data-init-error={state.code}>
          <h3 className="shell__init-failure-title">{CLOSED_HEADING}</h3>
          <p className="shell__init-failure-message">{CLOSED_MESSAGE}</p>
          <p className="shell__init-failure-code">
            {FAILURE_CODE_LABEL}: {state.code}
          </p>
        </div>
      </section>
    );
  }

  return (
    <section className="shell__init" data-init-state={state.kind}>
      <h2 className="shell__init-title">{INIT_HEADING}</h2>
      <p className="shell__init-description">{INIT_DESCRIPTION}</p>

      {editing ? (
        <div className="shell__init-step" data-init-step="details">
          {state.kind === "details" && state.code !== null ? (
            <div className="shell__init-failure" role="alert" data-init-error={state.code}>
              <h3 className="shell__init-failure-title">{CORRECTABLE_HEADING}</h3>
              <p className="shell__init-failure-message">
                {delivered === null
                  ? CORRECTABLE_BEFORE_DELIVERY_MESSAGE
                  : CORRECTABLE_AFTER_DELIVERY_MESSAGE}
              </p>
              <p className="shell__init-failure-code">
                {FAILURE_CODE_LABEL}: {state.code}
              </p>
            </div>
          ) : null}

          <h3 className="shell__init-section-title">{LOG_ASSIGNMENT_HEADING}</h3>
          <p className="shell__init-section-description">{LOG_ASSIGNMENT_DESCRIPTION}</p>
          <label className="shell__init-label" htmlFor={SYSTEM_LOG_SELECT_ID}>
            {SYSTEM_LOG_LABEL}
          </label>
          <select
            id={SYSTEM_LOG_SELECT_ID}
            className="shell__init-select"
            value={systemLogModule}
            onChange={changeSystemLog}
            disabled={busy}
          >
            <option value="">{UNASSIGNED_OPTION_LABEL}</option>
            <option value={SQLITE_LOG_MODULE}>{SQLITE_OPTION_LABEL}</option>
          </select>
          <label className="shell__init-label" htmlFor={AUDIT_LOG_SELECT_ID}>
            {AUDIT_LOG_LABEL}
          </label>
          <select
            id={AUDIT_LOG_SELECT_ID}
            className="shell__init-select"
            value={auditLogModule}
            onChange={changeAuditLog}
            disabled={busy}
          >
            <option value="">{UNASSIGNED_OPTION_LABEL}</option>
            <option value={SQLITE_LOG_MODULE}>{SQLITE_OPTION_LABEL}</option>
          </select>

          <h3 className="shell__init-section-title">{ADMINISTRATOR_HEADING}</h3>
          <p className="shell__init-section-description">{ADMINISTRATOR_DESCRIPTION}</p>
          <label className="shell__init-label" htmlFor={USERNAME_INPUT_ID}>
            {USERNAME_LABEL}
          </label>
          <input
            id={USERNAME_INPUT_ID}
            className="shell__init-input"
            type="text"
            autoComplete="off"
            spellCheck={false}
            maxLength={USERNAME_MAX_LENGTH}
            value={username}
            onChange={changeUsername}
            disabled={busy}
          />
          <label className="shell__init-label" htmlFor={DISPLAY_NAME_INPUT_ID}>
            {DISPLAY_NAME_LABEL}
          </label>
          <input
            id={DISPLAY_NAME_INPUT_ID}
            className="shell__init-input"
            type="text"
            autoComplete="off"
            spellCheck={false}
            maxLength={DISPLAY_NAME_MAX_LENGTH}
            value={displayName}
            onChange={changeDisplayName}
            disabled={busy}
          />
          <label className="shell__init-label" htmlFor={PASSWORD_INPUT_ID}>
            {PASSWORD_LABEL}
          </label>
          <input
            id={PASSWORD_INPUT_ID}
            className="shell__init-input"
            type="password"
            autoComplete="new-password"
            spellCheck={false}
            maxLength={PASSWORD_MAX_LENGTH}
            value={password}
            onChange={changePassword}
            disabled={busy}
          />

          {delivered === null ? (
            <button
              type="button"
              className="shell__init-action"
              onClick={prepare}
              disabled={busy || !complete}
            >
              {PREPARE_ACTION_LABEL}
            </button>
          ) : (
            <button
              type="button"
              className="shell__init-action"
              onClick={review}
              disabled={busy || !complete}
            >
              {RETURN_ACTION_LABEL}
            </button>
          )}
          {state.kind === "preparing" ? (
            <p className="shell__init-progress" role="status">
              {PREPARING_MESSAGE}
            </p>
          ) : null}
        </div>
      ) : null}

      {state.kind === "key" && delivered !== null ? (
        <div className="shell__init-step" data-init-step="key">
          <h3 className="shell__init-section-title">{KEY_HEADING}</h3>
          <p className="shell__init-key-warning" role="alert">
            {KEY_ONCE_WARNING}
          </p>
          <p className="shell__init-section-description">{KEY_PURPOSE_DESCRIPTION}</p>
          <label className="shell__init-label" htmlFor={RECOVERY_KEY_OUTPUT_ID}>
            {KEY_LABEL}
          </label>
          <input
            id={RECOVERY_KEY_OUTPUT_ID}
            className="shell__init-key"
            type="text"
            readOnly
            spellCheck={false}
            value={delivered.recoveryKey}
          />
          <button type="button" className="shell__init-copy" onClick={copyKey}>
            {COPY_ACTION_LABEL}
          </button>
          {copy === "copied" ? (
            <p className="shell__init-copy-result" role="status">
              {COPIED_MESSAGE}
            </p>
          ) : null}
          {copy === "failed" ? (
            <p className="shell__init-copy-result" role="alert">
              {COPY_FAILED_MESSAGE}
            </p>
          ) : null}
          <label className="shell__init-acknowledgement" htmlFor={ACKNOWLEDGEMENT_INPUT_ID}>
            <input
              id={ACKNOWLEDGEMENT_INPUT_ID}
              type="checkbox"
              checked={acknowledged}
              onChange={changeAcknowledgement}
            />
            {ACKNOWLEDGEMENT_LABEL}
          </label>
          <button
            type="button"
            className="shell__init-action"
            onClick={review}
            disabled={!acknowledged}
          >
            {KEY_CONTINUE_LABEL}
          </button>
        </div>
      ) : null}

      {state.kind === "review" || state.kind === "finalizing" ? (
        <div className="shell__init-step" data-init-step="review">
          <h3 className="shell__init-section-title">{REVIEW_HEADING}</h3>
          <p className="shell__init-section-description">{REVIEW_DESCRIPTION}</p>
          <dl className="shell__init-review">
            <dt className="shell__init-review-term">{REVIEW_USERNAME_LABEL}</dt>
            <dd className="shell__init-review-value" data-init-review="username">
              {username}
            </dd>
            <dt className="shell__init-review-term">{REVIEW_DISPLAY_NAME_LABEL}</dt>
            <dd className="shell__init-review-value" data-init-review="display-name">
              {displayName === "" ? REVIEW_UNSET_VALUE : displayName}
            </dd>
            <dt className="shell__init-review-term">{REVIEW_SYSTEM_LOG_LABEL}</dt>
            <dd className="shell__init-review-value" data-init-review="system-log">
              {SQLITE_OPTION_LABEL}
            </dd>
            <dt className="shell__init-review-term">{REVIEW_AUDIT_LOG_LABEL}</dt>
            <dd className="shell__init-review-value" data-init-review="audit-log">
              {SQLITE_OPTION_LABEL}
            </dd>
          </dl>
          <button type="button" className="shell__init-secondary" onClick={edit} disabled={busy}>
            {REVIEW_BACK_LABEL}
          </button>
          <button
            type="button"
            className="shell__init-action"
            onClick={finalize}
            disabled={busy || !complete}
          >
            {FINALIZE_ACTION_LABEL}
          </button>
          {state.kind === "finalizing" ? (
            <p className="shell__init-progress" role="status">
              {FINALIZING_MESSAGE}
            </p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

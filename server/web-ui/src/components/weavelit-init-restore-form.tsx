import { useCallback, useState, type ChangeEvent, type JSX } from "react";

import { probeSession } from "../api/weavelit-authentication";
import {
  RestoreFailedError,
  UNREPORTED_FAILURE_CODE,
  submitRestore,
} from "../api/weavelit-init-restore";

const RESTORE_HEADING = "Restore";
const RESTORE_DESCRIPTION =
  "Submit an encrypted backup and its recovery key to replace this deployment's state.";
const ARTIFACT_LABEL = "Backup file";
const RECOVERY_KEY_LABEL = "Recovery key";
const RESTORE_ACTION_LABEL = "Restore backup";
const RESTORE_COMPLETED_MESSAGE =
  "The backup was restored and this deployment now runs in normal operation.";

const INDETERMINATE_HEADING = "The Restore did not report an outcome";
const INDETERMINATE_MESSAGE =
  "This attempt reported no outcome, so whether this backup was restored is not yet known. Keep the recovery key you submitted: it is still the key this backup is encrypted with. Check again to see whether the Restore completed, or try again with the same key.";
const INDETERMINATE_CHECKING_MESSAGE = "Checking whether the Restore completed.";
const RECHECK_ACTION_LABEL = "Check again";
const FAILURE_CODE_LABEL = "Reported code";

const ARTIFACT_INPUT_ID = "weavelit-restore-artifact";
const RECOVERY_KEY_INPUT_ID = "weavelit-restore-recovery-key";

/**
 * Codes a Restore reports when it establishes no outcome at all.
 *
 * `gateway_timeout` is written by the listener when it stops waiting for the
 * route, not by the route. A commit chain that had already reached a blocking
 * boundary is not cancelled by that, so the deployment may have gone on to
 * become operational after this page was answered.
 */
const INDETERMINATE_RESTORE_CODES = ["gateway_timeout"];

/**
 * Codes a retry reports that an unsettled original attempt could itself cause.
 *
 * `restore_not_allowed` is answered whenever the deployment record has left
 * `Uninitialized`, which is exactly what an original Restore that committed
 * leaves behind. `not_found` is answered by every route of a published normal
 * operation, which no longer mounts Restore at all.
 *
 * Neither proves the original committed, because both also have determinate
 * causes: a lifecycle that is pending some other workflow answers the first,
 * and a Server serving nothing at all answers the second. They are therefore
 * reconciled through the authentication surface rather than believed, and they
 * are only read this way once an attempt has already reported no outcome. A
 * first attempt answered by either of them was answered about itself.
 */
const RECONCILED_RETRY_CODES = ["restore_not_allowed", "not_found"];

/** Presentation state of a Restore submission. */
type RestoreViewState =
  | { readonly kind: "idle" }
  | { readonly kind: "submitting" }
  | { readonly kind: "indeterminate"; readonly code: string; readonly checking: boolean }
  | { readonly kind: "failed"; readonly code: string }
  | { readonly kind: "completed" };

function failureCode(reason: unknown): string {
  return reason instanceof RestoreFailedError ? reason.code : UNREPORTED_FAILURE_CODE;
}

/**
 * Reports whether a Restore failure established no outcome whatsoever.
 *
 * The listener's timeout is one such result, and a Restore this page never read
 * an answer to is another: an accepted artifact upload whose connection failed
 * before its response arrived may still have sealed and published this
 * deployment. None of them proves the Restore did not commit.
 *
 * A rejection the route itself reported is excluded, so a determinate failure
 * is never dressed up as an unresolved one. That exclusion is why the code
 * alone cannot decide this: the Server reports {@link UNREPORTED_FAILURE_CODE}
 * for a determinate internal failure of its own.
 */
function indeterminateRestore(reason: unknown, code: string): boolean {
  return (
    INDETERMINATE_RESTORE_CODES.includes(code) ||
    (reason instanceof RestoreFailedError && reason.indeterminate)
  );
}

/** Props of the Restore submission control. */
export interface RestoreSubmissionFormProps {
  /** Invoked once, after the Restore has been confirmed by the Server. */
  readonly onCompleted: () => void;
}

/**
 * The minimal Restore submission control of the pre-operational Web UI.
 *
 * The selected backup is held as a `File` handle and the recovery key as
 * component state alone. Neither is written to a URL, a cookie, or any browser
 * storage, and the key is cleared as soon as the attempt it drove settles,
 * whether that attempt succeeded or failed.
 *
 * An outcome is settled only by evidence. A completed Restore is reported to
 * the shell from the completion response this component already holds, so the
 * surface it belongs to is withdrawn without a reload or a status request the
 * sealed deployment no longer serves. An attempt that reported no outcome is
 * settled the same way the Init workflow settles one, against the Server's own
 * authentication surface, because a Restore that sealed this deployment while
 * its answer was lost would otherwise leave this page offering retries against
 * routes that no longer exist.
 *
 * An absent authentication surface is not that evidence: it is what a Restore
 * still running and a Restore that never committed both look like. Such an
 * attempt stays unsettled, and its key stays with it, until something proves
 * what happened.
 */
export function RestoreSubmissionForm({ onCompleted }: RestoreSubmissionFormProps): JSX.Element {
  const [artifact, setArtifact] = useState<File | null>(null);
  const [recoveryKey, setRecoveryKey] = useState("");
  const [state, setState] = useState<RestoreViewState>({ kind: "idle" });
  /**
   * Whether some attempt has already reported no outcome.
   *
   * Once one has, this page can never again read {@link RECONCILED_RETRY_CODES}
   * as a determinate answer, so this is never cleared: only completion, or
   * loading the page again, ends it.
   */
  const [unsettled, setUnsettled] = useState(false);

  const chooseArtifact = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    // Retained as a handle; the file's bytes are never read here.
    setArtifact(event.target.files?.[0] ?? null);
  }, []);

  const changeRecoveryKey = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setRecoveryKey(event.target.value);
  }, []);

  const reconcile = useCallback(
    (code: string) => {
      setUnsettled(true);
      setState({ kind: "indeterminate", code, checking: true });
      void probeSession().then((probe) => {
        if (probe.kind === "absent") {
          // No authentication surface is served, which distinguishes nothing:
          // the Restore may still be running, or may never have committed. The
          // attempt stays unsettled, the key is kept, and the operator decides
          // when to look again.
          setState({ kind: "indeterminate", code, checking: false });
          return;
        }
        // An identity or an unauthenticated challenge both prove the same
        // thing: normal operation was published, so this Restore committed.
        setRecoveryKey("");
        setState({ kind: "completed" });
        onCompleted();
      });
    },
    [onCompleted],
  );

  const submit = useCallback(() => {
    if (artifact === null || recoveryKey === "") {
      return;
    }
    setState({ kind: "submitting" });
    void submitRestore(recoveryKey, artifact).then(
      () => {
        setRecoveryKey("");
        setState({ kind: "completed" });
        onCompleted();
      },
      (reason: unknown) => {
        const code = failureCode(reason);
        // An attempt that reported nothing, and a retry answered by a code an
        // already unsettled original could itself have caused, are both taken
        // to the authentication surface rather than presented. Neither has
        // settled, so neither releases the key.
        if (
          indeterminateRestore(reason, code) ||
          (unsettled && RECONCILED_RETRY_CODES.includes(code))
        ) {
          // Deliberately keeps the recovery key: this attempt, or the
          // unsettled one it retried, may already have restored and sealed
          // this deployment with it.
          reconcile(code);
          return;
        }
        setRecoveryKey("");
        setState({ kind: "failed", code });
      },
    );
  }, [artifact, onCompleted, reconcile, recoveryKey, unsettled]);

  const recheck = useCallback(() => {
    if (state.kind === "indeterminate") {
      reconcile(state.code);
    }
  }, [reconcile, state]);

  // An unresolved attempt may still commit with this exact artifact and key,
  // so a retry cannot replace either payload while reconciliation is pending.
  const payloadLocked = state.kind === "indeterminate";
  // A probe in flight holds the controls too, so no retry is submitted against
  // a deployment that may already have stopped serving this route.
  const busy = state.kind === "submitting" || (state.kind === "indeterminate" && state.checking);

  return (
    <section className="shell__restore" data-restore-state={state.kind}>
      <h2 className="shell__restore-title">{RESTORE_HEADING}</h2>
      <p className="shell__restore-description">{RESTORE_DESCRIPTION}</p>
      {state.kind === "completed" ? (
        <p className="shell__restore-completion" role="status">
          {RESTORE_COMPLETED_MESSAGE}
        </p>
      ) : (
        <>
          <label className="shell__restore-label" htmlFor={ARTIFACT_INPUT_ID}>
            {ARTIFACT_LABEL}
          </label>
          <input
            id={ARTIFACT_INPUT_ID}
            className="shell__restore-artifact"
            type="file"
            onChange={chooseArtifact}
            disabled={busy || payloadLocked}
          />
          <label className="shell__restore-label" htmlFor={RECOVERY_KEY_INPUT_ID}>
            {RECOVERY_KEY_LABEL}
          </label>
          <input
            id={RECOVERY_KEY_INPUT_ID}
            className="shell__restore-recovery-key"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={recoveryKey}
            onChange={changeRecoveryKey}
            disabled={busy || payloadLocked}
          />
          <button
            type="button"
            className="shell__restore-action"
            onClick={submit}
            disabled={busy || artifact === null || recoveryKey === ""}
          >
            {RESTORE_ACTION_LABEL}
          </button>
        </>
      )}
      {state.kind === "indeterminate" ? (
        <div className="shell__restore-indeterminate" role="alert" data-restore-error={state.code}>
          <h3 className="shell__restore-failure-title">{INDETERMINATE_HEADING}</h3>
          <p className="shell__restore-failure-message">{INDETERMINATE_MESSAGE}</p>
          <p className="shell__restore-failure-code">
            {FAILURE_CODE_LABEL}: {state.code}
          </p>
          {state.checking ? (
            <p className="shell__restore-progress" role="status">
              {INDETERMINATE_CHECKING_MESSAGE}
            </p>
          ) : (
            <button type="button" className="shell__restore-recheck" onClick={recheck}>
              {RECHECK_ACTION_LABEL}
            </button>
          )}
        </div>
      ) : null}
      {state.kind === "failed" ? (
        <p className="shell__restore-failure" role="alert" data-restore-error={state.code}>
          {state.code}
        </p>
      ) : null}
    </section>
  );
}

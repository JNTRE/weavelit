import { useCallback, useState, type ChangeEvent, type JSX } from "react";

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

const ARTIFACT_INPUT_ID = "weavelit-restore-artifact";
const RECOVERY_KEY_INPUT_ID = "weavelit-restore-recovery-key";

/** Presentation state of a Restore submission. */
type RestoreViewState =
  | { readonly kind: "idle" }
  | { readonly kind: "submitting" }
  | { readonly kind: "failed"; readonly code: string }
  | { readonly kind: "completed" };

function failureCode(reason: unknown): string {
  return reason instanceof RestoreFailedError ? reason.code : UNREPORTED_FAILURE_CODE;
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
 * A completed Restore is reported to the shell from the completion response
 * this component already holds, so the surface it belongs to is withdrawn
 * without a reload or a status request the sealed deployment no longer serves.
 */
export function RestoreSubmissionForm({ onCompleted }: RestoreSubmissionFormProps): JSX.Element {
  const [artifact, setArtifact] = useState<File | null>(null);
  const [recoveryKey, setRecoveryKey] = useState("");
  const [state, setState] = useState<RestoreViewState>({ kind: "idle" });

  const chooseArtifact = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    // Retained as a handle; the file's bytes are never read here.
    setArtifact(event.target.files?.[0] ?? null);
  }, []);

  const changeRecoveryKey = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setRecoveryKey(event.target.value);
  }, []);

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
        setRecoveryKey("");
        setState({ kind: "failed", code: failureCode(reason) });
      },
    );
  }, [artifact, onCompleted, recoveryKey]);

  const submitting = state.kind === "submitting";

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
            disabled={submitting}
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
            disabled={submitting}
          />
          <button
            type="button"
            className="shell__restore-action"
            onClick={submit}
            disabled={submitting || artifact === null || recoveryKey === ""}
          >
            {RESTORE_ACTION_LABEL}
          </button>
        </>
      )}
      {state.kind === "failed" ? (
        <p className="shell__restore-failure" role="alert" data-restore-error={state.code}>
          {state.code}
        </p>
      ) : null}
    </section>
  );
}

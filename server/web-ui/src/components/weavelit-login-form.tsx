import { useCallback, useEffect, useRef, useState, type ChangeEvent, type JSX } from "react";

import {
  IndeterminateAuthenticationError,
  MFA_CODE_DIGITS,
  MFA_CODE_PATTERN,
  confirmEnrollment,
  openEnrollment,
  probeSession,
  submitLogin,
  submitSecondFactor,
  type EnrollmentOpened,
} from "../api/weavelit-authentication";

const LOGIN_HEADING = "Sign in";
const LOGIN_DESCRIPTION = "Sign in with your Weavelit account.";
const USERNAME_LABEL = "Username";
const PASSWORD_LABEL = "Password";
const LOGIN_ACTION_LABEL = "Sign in";

const SECOND_FACTOR_DESCRIPTION =
  "Enter the current code from the authenticator app registered to this account.";
const ENROLLMENT_DESCRIPTION =
  "This account must register an authenticator app before it can sign in. Add the setup key below to an authenticator app, then enter the code it shows.";

/**
 * The one warning the single disclosure carries.
 *
 * The Server returns the setup key and the setup link in exactly one response
 * and can never return them again, so the panel says so on the only page that
 * will ever hold them, before the operator can navigate away from it.
 */
const ENROLLMENT_DISCLOSURE_WARNING =
  "This setup key and setup link are shown once and cannot be shown again. Add them to an authenticator app before leaving this page.";

const CODE_LABEL = "Authentication code";
const CODE_ACTION_LABEL = "Verify code";
const ENROLLMENT_ACTION_LABEL = "Confirm authenticator app";
const SETUP_KEY_LABEL = "Setup key";
const SETUP_LINK_LABEL = "Setup link";

/**
 * The one message every failed sign-in presents.
 *
 * It names no cause, because the Server reports none and because presenting a
 * cause would reintroduce the account disclosure the Server's equal-work denial
 * exists to prevent.
 */
const LOGIN_FAILED_MESSAGE = "Sign-in failed.";

/**
 * The message a spent one-time value presents.
 *
 * A continuation is consumed by the first request that presents it, whether or
 * not that request was accepted, so a rejected code ends the attempt outright.
 * Saying so is what stops the panel from offering a retry the Server has
 * already made impossible.
 */
const ATTEMPT_ENDED_MESSAGE =
  "That code was not accepted. This sign-in attempt has ended, so sign in again to start a new one.";

const AUTHENTICATED_MESSAGE = "You are signed in.";

const INDETERMINATE_MESSAGE = "Your sign-in outcome is still being checked.";
const CHECKING_MESSAGE = "Checking whether your sign-in completed.";
const CHECK_AGAIN_LABEL = "Check again";

// Three probes over thirty seconds allow a delayed blocking commit to appear
// while keeping automatic reconciliation at six probes per minute or fewer.
const RECONCILIATION_RETRY_DELAYS_MS = [10_000, 20_000] as const;

const USERNAME_INPUT_ID = "weavelit-login-username";
const PASSWORD_INPUT_ID = "weavelit-login-password";
const CODE_INPUT_ID = "weavelit-login-code";
const SETUP_KEY_OUTPUT_ID = "weavelit-login-setup-key";
const SETUP_LINK_OUTPUT_ID = "weavelit-login-setup-link";

/**
 * Presentation state of the sign-in panel.
 *
 * `checking` and `absent` describe whether this deployment serves an
 * authentication surface at all. Neither is reachable from a credential
 * submission, so no rendered state can be read back as a reason for a denial.
 *
 * Every second-factor state is reached only from a continuation the Server
 * issued, so reaching one discloses nothing a denial does not: the states a
 * denied credential can reach remain exactly `failed` and `attempt-ended`.
 */
type LoginViewState =
  | "checking"
  | "absent"
  | "unauthenticated"
  | "submitting"
  | "failed"
  | "attempt-ended"
  | "indeterminate"
  | "second-factor"
  | "second-factor-submitting"
  | "enrollment"
  | "enrollment-submitting"
  | "authenticated";

/** Reports whether a state renders the username and password controls. */
function isCredentialState(state: LoginViewState): boolean {
  return (
    state === "unauthenticated" ||
    state === "submitting" ||
    state === "failed" ||
    state === "attempt-ended"
  );
}

/**
 * The minimal sign-in control of the Web UI.
 *
 * The username and password are held in component state alone and the password
 * is cleared as soon as the attempt it drove settles, whether that attempt
 * succeeded or failed. Neither is written to a URL, a cookie, or any browser
 * storage, and neither is rendered. The session and cross-site request forgery
 * values are held only in the Server's cookies; this component never reads the
 * session value and never renders either.
 *
 * The continuation, the enrollment value, the setup key, and the setup link are
 * held exactly the same way: in component state for the one attempt that needs
 * them, and dropped as soon as that attempt settles. Nothing that outlives the
 * enrollment retains them.
 */
export interface LoginPanelProps {
  readonly onAuthenticated?: () => void;
}

export function LoginPanel({ onAuthenticated }: LoginPanelProps = {}): JSX.Element | null {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [continuation, setContinuation] = useState("");
  const [provisioning, setProvisioning] = useState<EnrollmentOpened | null>(null);
  const [state, setState] = useState<LoginViewState>("checking");
  const [reconciling, setReconciling] = useState(false);
  const mounted = useRef(true);
  const reconciliationAttempt = useRef(0);
  const reconciliationTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      reconciliationAttempt.current += 1;
      if (reconciliationTimer.current !== null) {
        clearTimeout(reconciliationTimer.current);
        reconciliationTimer.current = null;
      }
    };
  }, []);

  // The Server's durable session store is the only authority on whether the
  // cookies this browser holds still authenticate, so the panel asks it rather
  // than remembering an earlier answer.
  useEffect(() => {
    void probeSession().then((probe) => {
      if (mounted.current) {
        setState(probe.kind === "authenticated" ? "authenticated" : probe.kind);
      }
    });
  }, []);

  useEffect(() => {
    if (state === "authenticated") {
      onAuthenticated?.();
    }
  }, [onAuthenticated, state]);

  const changeUsername = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setUsername(event.target.value);
  }, []);

  const changePassword = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setPassword(event.target.value);
  }, []);

  const changeCode = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setCode(event.target.value);
  }, []);

  /** Settles the attempt and drops every one-time value it was holding. */
  const endAttempt = useCallback((settled: LoginViewState) => {
    setPassword("");
    setCode("");
    setContinuation("");
    setProvisioning(null);
    setState(settled);
  }, []);

  const reconcileIndeterminateAuthentication = useCallback(() => {
    if (reconciling) {
      return;
    }
    if (reconciliationTimer.current !== null) {
      clearTimeout(reconciliationTimer.current);
      reconciliationTimer.current = null;
    }

    endAttempt("indeterminate");
    setReconciling(true);
    reconciliationAttempt.current += 1;
    const attempt = reconciliationAttempt.current;

    const probe = (retry: number): void => {
      if (!mounted.current || reconciliationAttempt.current !== attempt) {
        return;
      }
      void probeSession().then((result) => {
        if (!mounted.current || reconciliationAttempt.current !== attempt) {
          return;
        }
        if (result.kind === "authenticated") {
          reconciliationTimer.current = null;
          setReconciling(false);
          endAttempt("authenticated");
          return;
        }

        const delay = RECONCILIATION_RETRY_DELAYS_MS[retry];
        if (delay === undefined) {
          reconciliationTimer.current = null;
          setReconciling(false);
          setState("indeterminate");
          return;
        }
        reconciliationTimer.current = setTimeout(() => {
          reconciliationTimer.current = null;
          probe(retry + 1);
        }, delay);
      });
    };

    probe(0);
  }, [endAttempt, reconciling]);

  const submit = useCallback(() => {
    if (username === "" || password === "") {
      return;
    }
    setState("submitting");
    void submitLogin(username, password).then(
      (outcome) => {
        setPassword("");
        if (outcome.kind === "session") {
          endAttempt("authenticated");
          return;
        }
        if (outcome.kind === "mfa_required") {
          setContinuation(outcome.continuation);
          setState("second-factor");
          return;
        }
        // The enrollment is opened from this settled submission rather than
        // from a render effect, so the one response that discloses the setup
        // key is requested exactly once per attempt.
        void openEnrollment(outcome.continuation).then(
          (opened) => {
            setProvisioning(opened);
            setState("enrollment");
          },
          () => {
            endAttempt("attempt-ended");
          },
        );
      },
      (error: unknown) => {
        if (error instanceof IndeterminateAuthenticationError) {
          reconcileIndeterminateAuthentication();
          return;
        }
        // Every rejection presents identically, so the reason is discarded
        // here rather than inspected. There is nothing in it to render.
        endAttempt("failed");
      },
    );
  }, [username, password, endAttempt, reconcileIndeterminateAuthentication]);

  const submitCode = useCallback(() => {
    if (!MFA_CODE_PATTERN.test(code) || continuation === "") {
      return;
    }
    setState("second-factor-submitting");
    void submitSecondFactor(continuation, code).then(
      () => {
        endAttempt("authenticated");
      },
      (error: unknown) => {
        if (error instanceof IndeterminateAuthenticationError) {
          reconcileIndeterminateAuthentication();
          return;
        }
        endAttempt("attempt-ended");
      },
    );
  }, [code, continuation, endAttempt, reconcileIndeterminateAuthentication]);

  const submitEnrollmentCode = useCallback(() => {
    if (!MFA_CODE_PATTERN.test(code) || provisioning === null) {
      return;
    }
    setState("enrollment-submitting");
    void confirmEnrollment(provisioning.enrollment, code).then(
      () => {
        endAttempt("authenticated");
      },
      (error: unknown) => {
        if (error instanceof IndeterminateAuthenticationError) {
          reconcileIndeterminateAuthentication();
          return;
        }
        endAttempt("attempt-ended");
      },
    );
  }, [code, provisioning, endAttempt, reconcileIndeterminateAuthentication]);

  if (state === "checking" || state === "absent") {
    return null;
  }

  const submitting = state === "submitting";
  const codeSubmitting = state === "second-factor-submitting" || state === "enrollment-submitting";
  const codeIncomplete = !MFA_CODE_PATTERN.test(code);

  return (
    <section className="shell__login" data-authentication-state={state}>
      <h2 className="shell__login-title">{LOGIN_HEADING}</h2>
      {state === "authenticated" ? (
        <p className="shell__login-authenticated" role="status">
          {AUTHENTICATED_MESSAGE}
        </p>
      ) : null}
      {isCredentialState(state) ? (
        <>
          <p className="shell__login-description">{LOGIN_DESCRIPTION}</p>
          <label className="shell__login-label" htmlFor={USERNAME_INPUT_ID}>
            {USERNAME_LABEL}
          </label>
          <input
            id={USERNAME_INPUT_ID}
            className="shell__login-username"
            type="text"
            autoComplete="username"
            spellCheck={false}
            value={username}
            onChange={changeUsername}
            disabled={submitting}
          />
          <label className="shell__login-label" htmlFor={PASSWORD_INPUT_ID}>
            {PASSWORD_LABEL}
          </label>
          <input
            id={PASSWORD_INPUT_ID}
            className="shell__login-password"
            type="password"
            autoComplete="current-password"
            spellCheck={false}
            value={password}
            onChange={changePassword}
            disabled={submitting}
          />
          <button
            type="button"
            className="shell__login-action"
            onClick={submit}
            disabled={submitting || username === "" || password === ""}
          >
            {LOGIN_ACTION_LABEL}
          </button>
          {state === "failed" ? (
            <p className="shell__login-failure" role="alert">
              {LOGIN_FAILED_MESSAGE}
            </p>
          ) : null}
          {state === "attempt-ended" ? (
            <p className="shell__login-attempt-ended" role="alert">
              {ATTEMPT_ENDED_MESSAGE}
            </p>
          ) : null}
        </>
      ) : null}
      {state === "indeterminate" ? (
        <>
          <p className="shell__login-indeterminate" role="status">
            {reconciling ? CHECKING_MESSAGE : INDETERMINATE_MESSAGE}
          </p>
          <button
            type="button"
            className="shell__login-action"
            onClick={reconcileIndeterminateAuthentication}
            disabled={reconciling}
          >
            {CHECK_AGAIN_LABEL}
          </button>
        </>
      ) : null}
      {state === "second-factor" || state === "second-factor-submitting" ? (
        <>
          <p className="shell__login-description">{SECOND_FACTOR_DESCRIPTION}</p>
          <label className="shell__login-label" htmlFor={CODE_INPUT_ID}>
            {CODE_LABEL}
          </label>
          <input
            id={CODE_INPUT_ID}
            className="shell__login-code"
            type="text"
            inputMode="numeric"
            autoComplete="one-time-code"
            spellCheck={false}
            maxLength={MFA_CODE_DIGITS}
            value={code}
            onChange={changeCode}
            disabled={codeSubmitting}
          />
          <button
            type="button"
            className="shell__login-action"
            onClick={submitCode}
            disabled={codeSubmitting || codeIncomplete}
          >
            {CODE_ACTION_LABEL}
          </button>
        </>
      ) : null}
      {(state === "enrollment" || state === "enrollment-submitting") && provisioning !== null ? (
        <>
          <p className="shell__login-description">{ENROLLMENT_DESCRIPTION}</p>
          <p className="shell__login-disclosure" role="alert">
            {ENROLLMENT_DISCLOSURE_WARNING}
          </p>
          <label className="shell__login-label" htmlFor={SETUP_KEY_OUTPUT_ID}>
            {SETUP_KEY_LABEL}
          </label>
          {/* Read-only fields rather than links, so the disclosed values can be
              selected and copied without becoming a navigation target. */}
          <input
            id={SETUP_KEY_OUTPUT_ID}
            className="shell__login-setup-key"
            type="text"
            readOnly
            spellCheck={false}
            value={provisioning.secret}
          />
          <label className="shell__login-label" htmlFor={SETUP_LINK_OUTPUT_ID}>
            {SETUP_LINK_LABEL}
          </label>
          <input
            id={SETUP_LINK_OUTPUT_ID}
            className="shell__login-setup-link"
            type="text"
            readOnly
            spellCheck={false}
            value={provisioning.provisioningUri}
          />
          <label className="shell__login-label" htmlFor={CODE_INPUT_ID}>
            {CODE_LABEL}
          </label>
          <input
            id={CODE_INPUT_ID}
            className="shell__login-code"
            type="text"
            inputMode="numeric"
            autoComplete="one-time-code"
            spellCheck={false}
            maxLength={MFA_CODE_DIGITS}
            value={code}
            onChange={changeCode}
            disabled={codeSubmitting}
          />
          <button
            type="button"
            className="shell__login-action"
            onClick={submitEnrollmentCode}
            disabled={codeSubmitting || codeIncomplete}
          >
            {ENROLLMENT_ACTION_LABEL}
          </button>
        </>
      ) : null}
    </section>
  );
}

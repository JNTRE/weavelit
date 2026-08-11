import { useCallback, useEffect, useRef, useState, type ChangeEvent, type JSX } from "react";

import { probeSession, submitLogin } from "../api/weavelit-authentication";

const LOGIN_HEADING = "Sign in";
const LOGIN_DESCRIPTION = "Sign in with your Weavelit account.";
const USERNAME_LABEL = "Username";
const PASSWORD_LABEL = "Password";
const LOGIN_ACTION_LABEL = "Sign in";

/**
 * The one message every failed sign-in presents.
 *
 * It names no cause, because the Server reports none and because presenting a
 * cause would reintroduce the account disclosure the Server's equal-work denial
 * exists to prevent.
 */
const LOGIN_FAILED_MESSAGE = "Sign-in failed.";

const AUTHENTICATED_MESSAGE = "You are signed in.";

const USERNAME_INPUT_ID = "weavelit-login-username";
const PASSWORD_INPUT_ID = "weavelit-login-password";

/**
 * Presentation state of the sign-in panel.
 *
 * `checking` and `absent` describe whether this deployment serves an
 * authentication surface at all. Neither is reachable from a credential
 * submission, so no rendered state can be read back as a reason for a denial.
 */
type LoginViewState =
  "checking" | "absent" | "unauthenticated" | "submitting" | "failed" | "authenticated";

/**
 * The minimal sign-in control of the Web UI.
 *
 * The username and password are held in component state alone and the password
 * is cleared as soon as the attempt it drove settles, whether that attempt
 * succeeded or failed. Neither is written to a URL, a cookie, or any browser
 * storage, and neither is rendered. The session and cross-site request forgery
 * values are held only in the Server's cookies; this component never reads the
 * session value and never renders either.
 */
export function LoginPanel(): JSX.Element | null {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [state, setState] = useState<LoginViewState>("checking");
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
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

  const changeUsername = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setUsername(event.target.value);
  }, []);

  const changePassword = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setPassword(event.target.value);
  }, []);

  const submit = useCallback(() => {
    if (username === "" || password === "") {
      return;
    }
    setState("submitting");
    void submitLogin(username, password).then(
      () => {
        setPassword("");
        setState("authenticated");
      },
      () => {
        setPassword("");
        // Every rejection presents identically, so the reason is discarded
        // here rather than inspected. There is nothing in it to render.
        setState("failed");
      },
    );
  }, [username, password]);

  if (state === "checking" || state === "absent") {
    return null;
  }

  const submitting = state === "submitting";

  return (
    <section className="shell__login" data-authentication-state={state}>
      <h2 className="shell__login-title">{LOGIN_HEADING}</h2>
      {state === "authenticated" ? (
        <p className="shell__login-authenticated" role="status">
          {AUTHENTICATED_MESSAGE}
        </p>
      ) : (
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
        </>
      )}
    </section>
  );
}

import { useState, type ChangeEvent, type JSX } from "react";

import {
  IndeterminateAuthenticationError,
  probeSession,
  submitPasswordChange,
} from "../api/weavelit-authentication";

const PASSWORD_INPUT_ID = "weavelit-password-change-password";

export interface PasswordChangeFormProps {
  readonly onCompleted: () => void;
}

export function PasswordChangeForm({ onCompleted }: PasswordChangeFormProps): JSX.Element {
  const [password, setPassword] = useState("");
  const [state, setState] = useState<"ready" | "submitting" | "failed" | "indeterminate">("ready");
  const mutationLocked = state === "submitting" || state === "indeterminate";

  const changePassword = (event: ChangeEvent<HTMLInputElement>): void => {
    setPassword(event.target.value);
  };

  const submit = (): void => {
    if (password === "" || mutationLocked) {
      return;
    }
    setState("submitting");
    void submitPasswordChange(password).then(
      () => {
        setPassword("");
        onCompleted();
      },
      (error: unknown) => {
        setPassword("");
        setState(error instanceof IndeterminateAuthenticationError ? "indeterminate" : "failed");
      },
    );
  };

  const checkSession = (): void => {
    void probeSession().then((probe) => {
      if (probe.kind === "authenticated" && !probe.passwordChangeRequired) {
        onCompleted();
      } else {
        setState("indeterminate");
      }
    });
  };

  return (
    <section className="password-change" data-password-change-state={state}>
      <h2 className="password-change__title">Choose a new password</h2>
      <label className="password-change__label" htmlFor={PASSWORD_INPUT_ID}>
        New password
      </label>
      <input
        id={PASSWORD_INPUT_ID}
        className="password-change__input"
        type="password"
        autoComplete="new-password"
        spellCheck={false}
        value={password}
        onChange={changePassword}
        disabled={mutationLocked}
      />
      <button
        type="button"
        className="password-change__action"
        onClick={submit}
        disabled={password === "" || mutationLocked}
      >
        Save password
      </button>
      {state === "failed" ? <p role="alert">Password change was not accepted.</p> : null}
      {state === "indeterminate" ? (
        <div role="alert">
          <p>Password change outcome is still being checked.</p>
          <button type="button" className="password-change__action" onClick={checkSession}>
            Check session
          </button>
        </div>
      ) : null}
    </section>
  );
}

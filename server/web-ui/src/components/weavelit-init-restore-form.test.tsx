import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RestoreSubmissionForm } from "./weavelit-init-restore-form";

const RECOVERY_KEY = "AGE-SECRET-KEY-1EXAMPLEEXAMPLEEXAMPLEEXAMPLE";
const TICKET = "0123456789abcdefghijklmnopqrstuvwxyzABC-_";
const CORRELATION = "0123456789abcdef0123456789abcdef";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function ticketResponse(): Response {
  return jsonResponse({ result: { restore_ticket: TICKET }, correlation_id: CORRELATION }, 202);
}

function completionResponse(): Response {
  return jsonResponse({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }, 200);
}

function mockRoutedFetch(routes: {
  key: () => Promise<Response>;
  upload: () => Promise<Response>;
}) {
  return vi
    .spyOn(globalThis, "fetch")
    .mockImplementation((input: unknown) =>
      input === "/api/v1/restore/artifact" ? routes.upload() : routes.key(),
    );
}

function section(): HTMLElement {
  return document.querySelector<HTMLElement>("section.shell__restore")!;
}

function renderForm(onCompleted: () => void = () => {}): void {
  render(<RestoreSubmissionForm onCompleted={onCompleted} />);
}

function artifactInput(): HTMLInputElement {
  return screen.getByLabelText("Backup file");
}

function recoveryKeyInput(): HTMLInputElement {
  return screen.getByLabelText("Recovery key");
}

function actionButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Restore backup" });
}

function chooseArtifact(): File {
  const file = new File([new Uint8Array([0x57, 0x4c, 0x42, 0x4b])], "backup.wlitbackup");
  fireEvent.change(artifactInput(), { target: { files: [file] } });
  return file;
}

function enterRecoveryKey(value = RECOVERY_KEY): void {
  fireEvent.change(recoveryKeyInput(), { target: { value } });
}

describe("RestoreSubmissionForm", () => {
  it("presents a file input, a masked key input, and a submit control", () => {
    renderForm();

    expect(section().dataset.restoreState).toBe("idle");
    expect(artifactInput().type).toBe("file");
    expect(recoveryKeyInput().type).toBe("password");
    expect(recoveryKeyInput().autocomplete).toBe("off");
    expect(actionButton().disabled).toBe(true);
  });

  it("enables the submit control only once both inputs are supplied", () => {
    renderForm();

    chooseArtifact();
    expect(actionButton().disabled).toBe(true);

    enterRecoveryKey();
    expect(actionButton().disabled).toBe(false);
  });

  it("submits the recovery key and then the artifact, and reports completion", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });

    renderForm();
    const file = chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("completed");
    });
    expect(screen.getByRole("status").textContent).toBe(
      "The backup was restored and this deployment now runs in normal operation.",
    );
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls[1]![1]?.body).toBe(file);
    // The completed state withdraws the controls, so no second attempt exists.
    expect(screen.queryByRole("button", { name: "Restore backup" })).toBeNull();
  });

  it("reports completion to its caller exactly once when the Restore succeeds", async () => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(section().dataset.restoreState).toBe("completed");
  });

  it("reports no completion to its caller when the Restore fails", async () => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(jsonResponse({ error: "recovery_key_invalid" }, 400)),
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("failed");
    });
    expect(completed).not.toHaveBeenCalled();
  });

  it("holds the submitting state and submits once while requests are in flight", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => new Promise<Response>(() => {}),
      upload: () => Promise.resolve(completionResponse()),
    });

    renderForm();
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("submitting");
    });
    expect(actionButton().disabled).toBe(true);
    expect(artifactInput().disabled).toBe(true);
    expect(recoveryKeyInput().disabled).toBe(true);

    fireEvent.click(actionButton());
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("clears the recovery key after a successful attempt", async () => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });

    renderForm();
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("completed");
    });
    expect(document.body.innerHTML).not.toContain(RECOVERY_KEY);
    expect(document.body.innerHTML).not.toContain(TICKET);
  });

  it("clears the recovery key after a failed attempt", async () => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(jsonResponse({ error: "recovery_key_invalid" }, 400)),
    });

    renderForm();
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("failed");
    });
    expect(recoveryKeyInput().value).toBe("");
    expect(document.body.innerHTML).not.toContain(RECOVERY_KEY);
    expect(document.body.innerHTML).not.toContain(TICKET);
    // The control is offered again, but only after the key is entered anew.
    expect(actionButton().disabled).toBe(true);
  });

  it.each([
    [400, "recovery_key_invalid"],
    [400, "backup_invalid"],
    [400, "backup_incompatible"],
    [403, "restore_ticket_invalid"],
    [409, "restore_not_allowed"],
    [503, "service_unavailable"],
  ])("renders only the stable code of a %i rejection", async (status, code) => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () =>
        Promise.resolve(jsonResponse({ error: code, detail: "/var/lib/weavelit" }, status)),
    });

    renderForm();
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(code);
    expect(document.body.textContent).not.toContain("/var/lib/weavelit");
    expect(document.body.textContent).not.toContain(String(status));
  });

  it("renders the fixed code when a failure carries no stable code", async () => {
    mockRoutedFetch({
      key: () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443")),
      upload: () => Promise.resolve(completionResponse()),
    });

    renderForm();
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("restore_failed");
    expect(document.body.textContent).not.toContain("ECONNREFUSED");
  });

  it("writes nothing to browser storage", async () => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });

    renderForm();
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("completed");
    });
    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    expect(document.cookie).toBe("");
  });
});

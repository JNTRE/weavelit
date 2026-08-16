import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RestoreSubmissionForm } from "./weavelit-init-restore-form";

const RECOVERY_KEY = "AGE-SECRET-KEY-1EXAMPLEEXAMPLEEXAMPLEEXAMPLE";
const TICKET = "0123456789abcdefghijklmnopqrstuvwxyzABC-_";
const CORRELATION = "0123456789abcdef0123456789abcdef";
const RECONCILIATION_CAPABILITY = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi";

const RECONCILIATION_PATH = "/api/v1/lifecycle/reconciliation";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function ticketResponse(reconciliationCapability = RECONCILIATION_CAPABILITY): Response {
  return jsonResponse(
    {
      result: { restore_ticket: TICKET, reconciliation_capability: reconciliationCapability },
      correlation_id: CORRELATION,
    },
    202,
  );
}

function completionResponse(): Response {
  return jsonResponse({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }, 200);
}

/** The exact confirmation a matching submitted capability receives. */
function confirmedReconciliation(): Promise<Response> {
  return Promise.resolve(jsonResponse({ result: "reconciliation_confirmed" }, 200));
}

/** The fixed answer a non-matching submitted capability receives. */
function notFoundReconciliation(): Promise<Response> {
  return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
}

/**
 * Routes the two submission requests and the reconciliation request apart.
 *
 * A reconciliation a test did not stage is refused rather than answered, so a
 * test that never expected one cannot silently read one.
 */
function mockRoutedFetch(routes: {
  key: () => Promise<Response>;
  upload: () => Promise<Response>;
  reconciliation?: () => Promise<Response>;
}) {
  return vi.spyOn(globalThis, "fetch").mockImplementation((input: unknown) => {
    if (input === "/api/v1/restore/artifact") {
      return routes.upload();
    }
    if (input === RECONCILIATION_PATH) {
      return routes.reconciliation === undefined
        ? Promise.reject(new Error("unrouted reconciliation"))
        : routes.reconciliation();
    }
    return routes.key();
  });
}

/** Counts the calls a mocked fetch received for one exact request target. */
function callsTo(mock: { mock: { calls: unknown[][] } }, path: string): number {
  return mock.mock.calls.filter((call) => call[0] === path).length;
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

/** The recheck control an unsettled attempt offers, if one is presented. */
function recheckButton(): HTMLButtonElement | null {
  return screen.queryByRole("button", { name: "Check again" });
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
    expect(document.body.innerHTML).not.toContain(RECONCILIATION_CAPABILITY);
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
    expect(alert.dataset.restoreError).toBe("restore_failed");
    expect(alert.textContent).toBe("restore_failed");
    expect(document.body.textContent).not.toContain("ECONNREFUSED");
  });

  it("completes when reconciliation confirms a Restore whose transport failed", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.reject(new Error("ECONNRESET")),
      reconciliation: confirmedReconciliation,
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    // The accepted upload may have sealed this deployment before its answer was
    // lost, and only its matching submitted capability confirms that outcome.
    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(section().dataset.restoreState).toBe("completed");
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps an unsettled Restore indeterminate when reconciliation mismatches", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.reject(new Error("ECONNRESET")),
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    const alert = await screen.findByRole("alert");
    // A mismatch proves a different deployment, never that this submission
    // settled, so it is never presented as the determinate failure it is not.
    expect(alert.textContent).toContain("The Restore did not report an outcome");
    expect(section().dataset.restoreState).toBe("indeterminate");
    expect(completed).not.toHaveBeenCalled();
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(1);
    // The key stays with the attempt it drove, because that attempt has not
    // settled and this is the only copy of it.
    expect(recoveryKeyInput().value).toBe(RECOVERY_KEY);
    expect(artifactInput().disabled).toBe(true);
    expect(recoveryKeyInput().disabled).toBe(true);
    expect(actionButton().disabled).toBe(false);
    expect(recheckButton()).not.toBeNull();
  });

  it("completes an unsettled Restore only when a recheck confirms its capability", async () => {
    const reconciliation = vi
      .fn<() => Promise<Response>>()
      .mockImplementationOnce(notFoundReconciliation)
      .mockImplementationOnce(confirmedReconciliation);
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.reject(new Error("ECONNRESET")),
      reconciliation,
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await screen.findByRole("alert");
    fireEvent.click(recheckButton()!);

    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(section().dataset.restoreState).toBe("completed");
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(2);
    for (const call of fetchMock.mock.calls.filter((call) => call[0] === RECONCILIATION_PATH)) {
      expect(call[1]?.body).toBe(
        JSON.stringify({ reconciliation_capability: RECONCILIATION_CAPABILITY }),
      );
    }
    expect(document.body.innerHTML).not.toContain(RECOVERY_KEY);
  });

  it("uses a new capability only after a retry receives a new ticket", async () => {
    const replacementCapability = "Zyxwvutsrqponmlkjihgfedcba0123456789ABCDE-_";
    const fetchMock = mockRoutedFetch({
      key: vi
        .fn<() => Promise<Response>>()
        .mockImplementationOnce(() => Promise.resolve(ticketResponse(RECONCILIATION_CAPABILITY)))
        .mockImplementationOnce(() => Promise.resolve(ticketResponse(replacementCapability))),
      upload: () => Promise.reject(new Error("ECONNRESET")),
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await screen.findByRole("alert");
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(2);
    });
    const reconciliationCalls = fetchMock.mock.calls.filter(
      (call) => call[0] === RECONCILIATION_PATH,
    );
    expect(reconciliationCalls[0]?.[1]?.body).toBe(
      JSON.stringify({ reconciliation_capability: RECONCILIATION_CAPABILITY }),
    );
    expect(reconciliationCalls[1]?.[1]?.body).toBe(
      JSON.stringify({ reconciliation_capability: replacementCapability }),
    );
    expect(section().dataset.restoreState).toBe("indeterminate");
    expect(completed).not.toHaveBeenCalled();
  });

  it.each([
    [409, "restore_not_allowed"],
    [404, "not_found"],
  ])(
    "completes when a retry is answered %i by the Restore its unsettled attempt committed",
    async (status, code) => {
      const reconciliation = vi
        .fn<() => Promise<Response>>()
        .mockImplementationOnce(notFoundReconciliation)
        .mockImplementationOnce(confirmedReconciliation);
      const fetchMock = mockRoutedFetch({
        key: vi
          .fn<() => Promise<Response>>()
          .mockImplementationOnce(() => Promise.resolve(ticketResponse()))
          .mockImplementationOnce(() => Promise.resolve(jsonResponse({ error: code }, status))),
        upload: () => Promise.reject(new Error("ECONNRESET")),
        reconciliation,
      });
      const completed = vi.fn();

      renderForm(completed);
      chooseArtifact();
      enterRecoveryKey();
      fireEvent.click(actionButton());

      await screen.findByRole("alert");
      // The same key drives the retry, because the attempt it belongs to never
      // settled.
      fireEvent.click(actionButton());

      await waitFor(() => {
        expect(completed).toHaveBeenCalledTimes(1);
      });
      expect(section().dataset.restoreState).toBe("completed");
      expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(2);
      expect(document.body.innerHTML).not.toContain(RECOVERY_KEY);
    },
  );

  it("keeps a retry answered restore_not_allowed unsettled when nothing proves it", async () => {
    mockRoutedFetch({
      key: vi
        .fn<() => Promise<Response>>()
        .mockImplementationOnce(() => Promise.resolve(ticketResponse()))
        .mockImplementationOnce(() =>
          Promise.resolve(jsonResponse({ error: "restore_not_allowed" }, 409)),
        ),
      upload: () => Promise.reject(new Error("ECONNRESET")),
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await screen.findByRole("alert");
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(recheckButton()).not.toBeNull();
    });
    // The code is reconciled, never believed: with no evidence the attempt is
    // still unsettled and is neither completed nor failed.
    expect(section().dataset.restoreState).toBe("indeterminate");
    expect(completed).not.toHaveBeenCalled();
    expect(recoveryKeyInput().value).toBe(RECOVERY_KEY);
  });

  it("still fails determinately, and drops the key, after an unsettled attempt", async () => {
    const fetchMock = mockRoutedFetch({
      key: vi
        .fn<() => Promise<Response>>()
        .mockImplementationOnce(() => Promise.resolve(ticketResponse()))
        .mockImplementationOnce(() => Promise.resolve(ticketResponse())),
      upload: vi
        .fn<() => Promise<Response>>()
        .mockImplementationOnce(() => Promise.reject(new Error("ECONNRESET")))
        .mockImplementationOnce(() =>
          Promise.resolve(jsonResponse({ error: "recovery_key_invalid" }, 400)),
        ),
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    await screen.findByRole("alert");
    fireEvent.click(actionButton());

    await waitFor(() => {
      expect(section().dataset.restoreState).toBe("failed");
    });
    // A rejection the Restore route reported about this very attempt is
    // evidence, so it settles and the key it used is released.
    expect(completed).not.toHaveBeenCalled();
    expect(recoveryKeyInput().value).toBe("");
    expect(document.body.innerHTML).not.toContain(RECOVERY_KEY);
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(1);
  });

  it("presents a Server-reported internal failure without reconciling it", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(jsonResponse({ error: "restore_failed" }, 500)),
    });
    const completed = vi.fn();

    renderForm(completed);
    chooseArtifact();
    enterRecoveryKey();
    fireEvent.click(actionButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("restore_failed");
    // The Server reported this one itself, so it is a determinate answer even
    // though it carries the same code a lost answer presents as.
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(0);
    expect(completed).not.toHaveBeenCalled();
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

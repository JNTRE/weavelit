import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  applyTotpEnablement,
  changeLogConfiguration,
  listLogConfigurations,
  previewTotpEnablement,
  viewLogConfiguration,
  type LogConfiguration,
} from "../api/weavelit-administration-configuration";
import { probeSession } from "../api/weavelit-authentication";
import { ConfigurationWorkspace } from "./weavelit-configuration-workspace";

vi.mock("../api/weavelit-administration-configuration");
vi.mock("../api/weavelit-authentication");

const PREVIEW = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const configuration: LogConfiguration = {
  configurationName: "primary",
  module: "sqlite",
  enabled: true,
  settings: [],
  assignedLogTypes: ["system", "audit"],
};

beforeEach(() => {
  vi.mocked(listLogConfigurations).mockResolvedValue({
    items: [configuration],
    nextCursor: null,
  });
  vi.mocked(viewLogConfiguration).mockResolvedValue(configuration);
  vi.mocked(previewTotpEnablement).mockResolvedValue({
    currentEnabled: true,
    desiredEnabled: false,
    affectedUsers: 1,
    preview: PREVIEW,
  });
  vi.mocked(applyTotpEnablement).mockResolvedValue({
    currentEnabled: false,
    affectedUsers: 1,
  });
  vi.mocked(changeLogConfiguration).mockResolvedValue(configuration);
  vi.mocked(probeSession).mockResolvedValue({
    kind: "authenticated",
    passwordChangeRequired: false,
  });
  localStorage.clear();
  sessionStorage.clear();
});

describe("Configuration workspace", () => {
  it("keeps the preview unrendered, consumes it once, and reconciles self-disable", async () => {
    const onSessionEnded = vi.fn();
    vi.mocked(probeSession).mockResolvedValue({ kind: "unauthenticated" });
    render(<ConfigurationWorkspace onSessionEnded={onSessionEnded} />);

    await screen.findByText("primary");
    fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
    await screen.findByText(/This change affects 1 enrolled account/);

    expect(document.body.textContent).not.toContain(PREVIEW);
    expect(location.href).not.toContain(PREVIEW);
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "Apply disablement" }));
    await waitFor(() => {
      expect(applyTotpEnablement).toHaveBeenCalledWith(false, PREVIEW);
      expect(probeSession).toHaveBeenCalledTimes(1);
      expect(onSessionEnded).toHaveBeenCalledTimes(1);
    });
    expect(applyTotpEnablement).toHaveBeenCalledTimes(1);
    expect(previewTotpEnablement).toHaveBeenCalledTimes(1);
  });

  it("loads safe Log detail and submits one complete name-based change", async () => {
    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");

    fireEvent.click(screen.getByRole("button", { name: "View" }));
    await waitFor(() => {
      expect(viewLogConfiguration).toHaveBeenCalledWith("primary");
    });
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));

    await waitFor(() => {
      expect(changeLogConfiguration).toHaveBeenCalledWith({
        configurationName: "primary",
        enabled: true,
        settings: [],
        assignments: [
          { logType: "system", configurationName: "primary" },
          { logType: "audit", configurationName: "primary" },
        ],
      });
    });
    expect(changeLogConfiguration).toHaveBeenCalledTimes(1);
  });
});

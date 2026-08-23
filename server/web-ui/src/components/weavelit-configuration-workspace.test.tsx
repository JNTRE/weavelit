import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ConfigurationAdministrationAccessDeniedError,
  ConfigurationConflictError,
  ConfigurationIndeterminateError,
  ConfigurationSessionInvalidError,
  applyTotpEnablement,
  changeLogConfiguration,
  listLogConfigurations,
  previewTotpEnablement,
  viewLogConfiguration,
  type LogConfiguration,
  type LogConfigurationsPage,
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
    publicId: "QUFBQUFBQUFBQUFBQUFBQQ",
    passwordChangeRequired: false,
  });
  localStorage.clear();
  sessionStorage.clear();
});

describe("Configuration workspace", () => {
  it("keeps the preview unrendered, consumes it once, and reconciles self-disable", async () => {
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    sessionStorage.setItem("sanity-check", "safe-value");
    expect(setItemSpy).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(sessionStorage.length).toBe(1);
    sessionStorage.clear();
    setItemSpy.mockClear();

    const onAdministrationEnded = vi.fn();
    vi.mocked(probeSession).mockResolvedValue({ kind: "unauthenticated" });
    render(<ConfigurationWorkspace onAdministrationEnded={onAdministrationEnded} />);

    await screen.findByText("primary");
    fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
    await screen.findByText(/This change affects 1 enrolled account/);

    expect(document.body.textContent).not.toContain(PREVIEW);
    expect(location.href).not.toContain(PREVIEW);
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(setItemSpy).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Apply disablement" }));
    await waitFor(() => {
      expect(applyTotpEnablement).toHaveBeenCalledWith(false, PREVIEW);
      expect(probeSession).toHaveBeenCalledTimes(1);
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(applyTotpEnablement).toHaveBeenCalledTimes(1);
    expect(previewTotpEnablement).toHaveBeenCalledTimes(1);
    expect(setItemSpy).not.toHaveBeenCalled();
    setItemSpy.mockRestore();
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

  it("blocks Log submission while Refresh is pending", async () => {
    let resolveRefresh!: (page: LogConfigurationsPage) => void;
    const pendingRefresh = new Promise<LogConfigurationsPage>((resolve) => {
      resolveRefresh = resolve;
    });
    let reads = 0;
    vi.mocked(listLogConfigurations).mockImplementation(() => {
      reads += 1;
      return reads === 1
        ? Promise.resolve({ items: [configuration], nextCursor: null })
        : pendingRefresh;
    });

    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    const heading = await screen.findByRole("heading", { name: "primary" });
    const form = heading.closest("form");
    expect(form).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(listLogConfigurations).toHaveBeenCalledTimes(2);
    });
    for (const control of form!.querySelectorAll<
      HTMLInputElement | HTMLSelectElement | HTMLButtonElement
    >("input, select, button"))
      expect(control.disabled).toBe(true);

    fireEvent.submit(form!);
    expect(changeLogConfiguration).not.toHaveBeenCalled();

    await act(async () => {
      resolveRefresh({ items: [configuration], nextCursor: null });
      await pendingRefresh;
    });
  });

  it(
    "keeps the committed Log projection and blocks Load more during an active save",
    async () => {
      const stale = { ...configuration, configurationName: "stale" };
      vi.mocked(listLogConfigurations).mockImplementation((cursor) =>
        cursor === "next-cursor"
          ? Promise.resolve({ items: [stale], nextCursor: null })
          : Promise.resolve({ items: [configuration], nextCursor: "next-cursor" }),
      );
      const changed = { ...configuration, enabled: false };
      let resolveChange!: (value: LogConfiguration) => void;
      const pendingChange = new Promise<LogConfiguration>((resolve) => {
        resolveChange = resolve;
      });
      vi.mocked(changeLogConfiguration).mockReturnValue(pendingChange);

      render(<ConfigurationWorkspace />);
      await screen.findByText("primary");
      fireEvent.click(screen.getByRole("button", { name: "View" }));
      const heading = await screen.findByRole("heading", { name: "primary" });
      fireEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
      const form = heading.closest("form");
      const loadMore = screen.getByRole<HTMLButtonElement>("button", { name: "Load more" });
      expect(form).not.toBeNull();

      act(() => {
        form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
        loadMore.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
      });

      expect(changeLogConfiguration).toHaveBeenCalledTimes(1);
      expect(listLogConfigurations).toHaveBeenCalledTimes(1);
      expect(loadMore.disabled).toBe(true);

      await act(async () => {
        resolveChange(changed);
        await pendingChange;
      });

      expect(screen.getByText("Disabled")).toBeTruthy();
      expect(screen.queryByText("stale")).toBeNull();
      expect(listLogConfigurations).toHaveBeenCalledTimes(1);
    },
  );

  it("ignores a stale Load more page after Refresh replaces the collection", async () => {
    const refreshed = { ...configuration, configurationName: "refreshed" };
    const stale = { ...configuration, configurationName: "stale" };
    let resolveStalePage!: (page: LogConfigurationsPage) => void;
    const pendingStalePage = new Promise<LogConfigurationsPage>((resolve) => {
      resolveStalePage = resolve;
    });
    let firstPageReads = 0;
    vi.mocked(listLogConfigurations).mockImplementation((cursor) => {
      if (cursor === "stale-cursor") return pendingStalePage;
      if (cursor === "refreshed-cursor") return Promise.resolve({ items: [], nextCursor: null });
      firstPageReads += 1;
      return Promise.resolve(
        firstPageReads === 1
          ? { items: [configuration], nextCursor: "stale-cursor" }
          : { items: [refreshed], nextCursor: "refreshed-cursor" },
      );
    });
    vi.mocked(viewLogConfiguration).mockResolvedValue(refreshed);
    vi.mocked(changeLogConfiguration).mockResolvedValue(refreshed);

    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => {
      expect(listLogConfigurations).toHaveBeenCalledWith("stale-cursor");
    });

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await screen.findByText("refreshed");

    await act(async () => {
      resolveStalePage({ items: [stale], nextCursor: "wrong-cursor" });
      await pendingStalePage;
    });

    expect(screen.queryByText("primary")).toBeNull();
    expect(screen.queryByText("stale")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => {
      expect(listLogConfigurations).toHaveBeenCalledWith("refreshed-cursor");
    });

    fireEvent.click(screen.getByRole("button", { name: "View" }));
    await screen.findByRole("heading", { name: "refreshed" });
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    await waitFor(() => {
      expect(changeLogConfiguration).toHaveBeenCalledWith({
        configurationName: "refreshed",
        enabled: true,
        settings: [],
        assignments: [
          { logType: "system", configurationName: "refreshed" },
          { logType: "audit", configurationName: "refreshed" },
        ],
      });
    });
  });

  it.each([
    ["session invalid", () => new ConfigurationSessionInvalidError()],
    ["authorization loss", () => new ConfigurationAdministrationAccessDeniedError()],
  ])("ends administration once when a TOTP preview reports %s", async (_label, error) => {
    vi.mocked(previewTotpEnablement).mockRejectedValue(error());
    const onAdministrationEnded = vi.fn();
    render(<ConfigurationWorkspace onAdministrationEnded={onAdministrationEnded} />);
    await screen.findByText("primary");

    fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText(/outcome is unknown/)).toBeNull();
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Review disablement" }).disabled,
    ).toBe(false);

    const readsBeforeRefresh = vi.mocked(listLogConfigurations).mock.calls.length;
    vi.mocked(listLogConfigurations).mockRejectedValueOnce(error());
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(listLogConfigurations).toHaveBeenCalledTimes(readsBeforeRefresh + 1);
    });
    expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Log configurations are unavailable.")).toBeNull();
  });

  it.each([
    ["TOTP apply", () => new ConfigurationAdministrationAccessDeniedError()],
    ["Log change", () => new ConfigurationSessionInvalidError()],
  ])("ends administration when %s reports terminal access loss", async (action, error) => {
    const onAdministrationEnded = vi.fn();
    if (action === "TOTP apply") vi.mocked(applyTotpEnablement).mockRejectedValue(error());
    else vi.mocked(changeLogConfiguration).mockRejectedValue(error());

    render(<ConfigurationWorkspace onAdministrationEnded={onAdministrationEnded} />);
    await screen.findByText("primary");
    if (action === "TOTP apply") {
      fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
      await screen.findByText(/This change affects 1 enrolled account/);
      fireEvent.click(screen.getByRole("button", { name: "Apply disablement" }));
    } else {
      fireEvent.click(screen.getByRole("button", { name: "View" }));
      await screen.findByRole("heading", { name: "primary" });
      fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    }

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText(/outcome is unknown|was not changed/)).toBeNull();
  });

  it("keeps TOTP Review controls retryable when preview is unavailable", async () => {
    vi.mocked(previewTotpEnablement).mockRejectedValueOnce(new Error("preview failed"));
    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");

    const previewCallsBefore = vi.mocked(previewTotpEnablement).mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
    await screen.findByText("TOTP preview is unavailable. Try again.");

    expect(screen.queryByText(/outcome is unknown/)).toBeNull();
    const reviewEnablement = screen.getByRole<HTMLButtonElement>("button", {
      name: "Review enablement",
    });
    const reviewDisablement = screen.getByRole<HTMLButtonElement>("button", {
      name: "Review disablement",
    });
    expect(reviewEnablement.disabled).toBe(false);
    expect(reviewDisablement.disabled).toBe(false);

    fireEvent.click(reviewEnablement);
    await waitFor(() => {
      expect(previewTotpEnablement).toHaveBeenCalledTimes(previewCallsBefore + 2);
    });
    expect(previewTotpEnablement).toHaveBeenLastCalledWith(true);
  });

  it.each([
    ["stale topology", () => new ConfigurationConflictError()],
    ["indeterminate outcome", () => new ConfigurationIndeterminateError()],
  ])("locks the Log form after a %s until Configuration is refreshed", async (_label, error) => {
    vi.mocked(changeLogConfiguration).mockRejectedValue(error());
    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");

    fireEvent.click(screen.getByRole("button", { name: "View" }));
    await screen.findByRole("heading", { name: "primary" });
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    await screen.findByText(/Refresh before another change/);

    const form = screen.getByRole("heading", { name: "primary" }).closest("form");
    expect(form).not.toBeNull();
    for (const control of form!.querySelectorAll<
      HTMLInputElement | HTMLSelectElement | HTMLButtonElement
    >("input, select, button"))
      expect(control.disabled).toBe(true);

    const readsBeforeRefresh = vi.mocked(listLogConfigurations).mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(listLogConfigurations).toHaveBeenCalledTimes(readsBeforeRefresh + 1);
    });
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    await screen.findByRole("heading", { name: "primary" });
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Save configuration" }).disabled,
    ).toBe(false);
  });

  it("keeps the Log form locked when Refresh fails after a conflict", async () => {
    vi.mocked(changeLogConfiguration).mockRejectedValue(new ConfigurationConflictError());
    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");

    fireEvent.click(screen.getByRole("button", { name: "View" }));
    await screen.findByRole("heading", { name: "primary" });
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    await screen.findByText(/Log configuration changed/);

    vi.mocked(listLogConfigurations).mockRejectedValueOnce(new Error("refresh failed"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await screen.findByText("Log configurations are unavailable.");
    expect(screen.getByText(/Log configuration changed/)).toBeTruthy();
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Save configuration" }).disabled,
    ).toBe(true);
  });

  it.each(["apply", "session reconciliation"])(
    "locks TOTP Review controls after indeterminate %s until Configuration is refreshed",
    async (outcome) => {
      if (outcome === "apply")
        vi.mocked(applyTotpEnablement).mockRejectedValue(new ConfigurationIndeterminateError());
      else vi.mocked(probeSession).mockResolvedValue({ kind: "absent" });

      render(<ConfigurationWorkspace />);
      await screen.findByText("primary");
      fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
      await screen.findByText(/This change affects 1 enrolled account/);
      fireEvent.click(screen.getByRole("button", { name: "Apply disablement" }));
      await screen.findByText(/outcome is unknown/);

      const reviewEnablement = screen.getByRole<HTMLButtonElement>("button", {
        name: "Review enablement",
      });
      const reviewDisablement = screen.getByRole<HTMLButtonElement>("button", {
        name: "Review disablement",
      });
      expect(reviewEnablement.disabled).toBe(true);
      expect(reviewDisablement.disabled).toBe(true);

      const readsBeforeRefresh = vi.mocked(listLogConfigurations).mock.calls.length;
      fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
      await waitFor(() => {
        expect(listLogConfigurations).toHaveBeenCalledTimes(readsBeforeRefresh + 1);
      });
      expect(reviewEnablement.disabled).toBe(false);
      expect(reviewDisablement.disabled).toBe(false);
    },
  );

  it("keeps TOTP Review controls locked when Refresh fails after an indeterminate outcome", async () => {
    vi.mocked(applyTotpEnablement).mockRejectedValue(new ConfigurationIndeterminateError());
    render(<ConfigurationWorkspace />);
    await screen.findByText("primary");

    fireEvent.click(screen.getByRole("button", { name: "Review disablement" }));
    await screen.findByText(/This change affects 1 enrolled account/);
    fireEvent.click(screen.getByRole("button", { name: "Apply disablement" }));
    await screen.findByText(/outcome is unknown/);

    vi.mocked(listLogConfigurations).mockRejectedValueOnce(new Error("refresh failed"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await screen.findByText("Log configurations are unavailable.");
    expect(screen.getByText(/TOTP enablement outcome is unknown/)).toBeTruthy();
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Review enablement" }).disabled,
    ).toBe(true);
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Review disablement" }).disabled,
    ).toBe(true);
  });

  it("ignores a stale View response after a later configuration is selected", async () => {
    const secondary = { ...configuration, configurationName: "secondary" };
    let resolvePrimary!: (value: LogConfiguration) => void;
    let resolveSecondary!: (value: LogConfiguration) => void;
    vi.mocked(listLogConfigurations).mockResolvedValue({
      items: [configuration, secondary],
      nextCursor: null,
    });
    vi.mocked(viewLogConfiguration).mockImplementation(
      (configurationName) =>
        new Promise((resolve) => {
          if (configurationName === "primary") resolvePrimary = resolve;
          else resolveSecondary = resolve;
        }),
    );

    render(<ConfigurationWorkspace />);
    await screen.findByText("secondary");
    const viewButtons = screen.getAllByRole("button", { name: "View" });
    fireEvent.click(viewButtons[0]!);
    fireEvent.click(viewButtons[1]!);

    await act(async () => {
      resolveSecondary(secondary);
      await Promise.resolve();
    });
    await screen.findByRole("heading", { name: "secondary" });
    await act(async () => {
      resolvePrimary(configuration);
      await Promise.resolve();
    });
    expect(screen.getByRole("heading", { name: "secondary" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "primary" })).toBeNull();
  });

  it("returns stale authorization loss from View to the shell", async () => {
    const secondary = { ...configuration, configurationName: "secondary" };
    let rejectPrimary!: (reason?: unknown) => void;
    let resolveSecondary!: (value: LogConfiguration) => void;
    vi.mocked(listLogConfigurations).mockResolvedValue({
      items: [configuration, secondary],
      nextCursor: null,
    });
    vi.mocked(viewLogConfiguration).mockImplementation(
      (configurationName) =>
        new Promise((resolve, reject) => {
          if (configurationName === "primary") rejectPrimary = reject;
          else resolveSecondary = resolve;
        }),
    );
    const onAdministrationEnded = vi.fn();

    render(<ConfigurationWorkspace onAdministrationEnded={onAdministrationEnded} />);
    await screen.findByText("secondary");
    const viewButtons = screen.getAllByRole("button", { name: "View" });
    fireEvent.click(viewButtons[0]!);
    fireEvent.click(viewButtons[1]!);

    await act(async () => {
      rejectPrimary(new ConfigurationAdministrationAccessDeniedError());
      await Promise.resolve();
    });
    expect(onAdministrationEnded).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveSecondary(secondary);
      await Promise.resolve();
    });
  });

  it.each(["list", "view"])(
    "returns a session-invalid %s read to the shell instead of showing unavailable Configuration",
    async (read) => {
      if (read === "list")
        vi.mocked(listLogConfigurations).mockRejectedValue(new ConfigurationSessionInvalidError());
      else
        vi.mocked(viewLogConfiguration).mockRejectedValue(new ConfigurationSessionInvalidError());
      const onAdministrationEnded = vi.fn();

      render(<ConfigurationWorkspace onAdministrationEnded={onAdministrationEnded} />);
      if (read === "view") {
        await screen.findByText("primary");
        fireEvent.click(screen.getByRole("button", { name: "View" }));
      }

      await waitFor(() => {
        expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
      });
      expect(screen.queryByText("Log configurations are unavailable.")).toBeNull();
    },
  );
});

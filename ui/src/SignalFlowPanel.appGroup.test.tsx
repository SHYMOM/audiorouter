// @vitest-environment jsdom
import { createRef } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SignalFlowPanel } from "./SignalFlowPanel";
import { demoSession } from "./fixtures";
import type { ApplicationRow, UiBackend } from "./backend";

function makeBackend(): UiBackend {
  return {
    connected: true,
    mode: "memory",
    permissions: [],
    grantId: null,
  } as unknown as UiBackend;
}

function renderPicker(addApplicationGroupFromPicker: (apps: ApplicationRow[], paths: string[]) => string | null) {
  return render(
    <SignalFlowPanel
      flowCanvas={<div />}
      backend={makeBackend()}
      connectionSource=""
      setConnectionSource={vi.fn()}
      outputPorts={[]}
      encodePort={(nodeId, portName) => `${nodeId}:${portName}`}
      connectionDestination=""
      setConnectionDestination={vi.fn()}
      inputPorts={[]}
      addConnection={() => true}
      openConnectionDialog={vi.fn()}
      connectionDialogOpen={false}
      connectionDialog={createRef()}
      closeConnectionDialog={vi.fn()}
      connectionDialogSource={createRef()}
      pluginPickerOpen={false}
      pluginPickerDialog={createRef()}
      pluginInsertEdgeId={null}
      closePluginPicker={vi.fn()}
      draft={demoSession}
      selectedNode={undefined}
      setSelectedNodeId={vi.fn()}
      setSelectedNodeIds={vi.fn()}
      unloadPluginNode={vi.fn()}
      liveDiagnostics={null}
      addPluginToDraft={vi.fn()}
      applicationPickerOpen
      applications={[]}
      applicationPickerSelection=""
      applicationPickerDialog={createRef()}
      closeApplicationPicker={vi.fn()}
      refreshApplications={vi.fn()}
      applicationsError={null}
      setApplicationPickerSelection={vi.fn()}
      addApplicationCaptureFromPicker={vi.fn()}
      addApplicationCapturePathFromPicker={() => null}
      addApplicationGroupFromPicker={addApplicationGroupFromPicker}
    />,
  );
}

describe("SignalFlowPanel app group picker", () => {
  it("shows manually added paths in group mode when no running apps exist and surfaces group errors", () => {
    const addGroup = vi
      .fn<() => string | null>()
      .mockReturnValueOnce("Choose at least two different applications for a group.")
      .mockReturnValueOnce(null);

    renderPicker(addGroup);

    fireEvent.click(screen.getByRole("button", { name: "App group" }));

    const input = screen.getByLabelText(/Executable path/i);
    fireEvent.change(input, { target: { value: "C:\\Games\\game.exe" } });
    fireEvent.click(screen.getByRole("button", { name: /Add path to group/i }));

    fireEvent.change(input, { target: { value: "C:\\Apps\\discord.exe" } });
    fireEvent.click(screen.getByRole("button", { name: /Add path to group/i }));

    expect(screen.getByText("C:\\Games\\game.exe")).toBeTruthy();
    expect(screen.getByText("C:\\Apps\\discord.exe")).toBeTruthy();

    const createButton = screen.getByRole("button", { name: /Create app group/i });
    expect((createButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(createButton);
    expect(addGroup).toHaveBeenCalledWith([], ["C:\\Games\\game.exe", "C:\\Apps\\discord.exe"]);
    expect(screen.getByText("Choose at least two different applications for a group.")).toBeTruthy();

    fireEvent.click(createButton);
    expect(addGroup).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("Choose at least two different applications for a group.")).toBeNull();
  });
});

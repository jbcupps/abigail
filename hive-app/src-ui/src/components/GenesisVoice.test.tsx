import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import GenesisVoice from "./GenesisVoice";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../runtimeMode", () => ({ detectRuntimeMode: () => "native" }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

describe("Windows Genesis voice", () => {
  it("requires consent and routes recognized creation commands through review actions", async () => {
    vi.mocked(invoke).mockResolvedValue("Accept suggestions.");
    const onCommand = vi.fn();
    render(<GenesisVoice onTranscript={vi.fn()} onSubmit={vi.fn()} onCommand={onCommand} response="" disabled={false} />);
    expect(screen.getByRole("button", { name: "Speak an answer" })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("checkbox", { name: /I consent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Speak an answer" }));
    await waitFor(() => expect(onCommand).toHaveBeenCalledWith("accept suggestions"));
    expect(invoke).toHaveBeenCalledWith("transcribe_genesis_speech", { consent: true });
  });

  it("ignores a late transcript after consent is withdrawn", async () => {
    let finish: (text: string) => void = () => undefined;
    vi.mocked(invoke).mockImplementation((command) => command === "transcribe_genesis_speech" ? new Promise(resolve => { finish = resolve as (text: string) => void; }) : Promise.resolve(undefined));
    const onTranscript = vi.fn();
    render(<GenesisVoice onTranscript={onTranscript} onSubmit={vi.fn()} onCommand={vi.fn()} response="" disabled={false} />);
    fireEvent.click(screen.getByRole("checkbox", { name: /I consent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Speak an answer" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("transcribe_genesis_speech", { consent: true }));
    fireEvent.click(screen.getByRole("checkbox", { name: /I consent/ }));
    await act(async () => { finish("A private thought"); });
    expect(onTranscript).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("cancel_genesis_speech", undefined);
  });
});

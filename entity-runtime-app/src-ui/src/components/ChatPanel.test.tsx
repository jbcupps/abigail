import { act, fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatGatewayCallbacks, ChatGatewayStream } from "../chat/chatGateway";
import type { ConversationSnapshot } from "../lib/conversation";
import ChatPanel from "./ChatPanel";

const mocks = vi.hoisted(() => ({
  send: vi.fn(),
  cancel: vi.fn(),
  dispose: vi.fn(),
  loadConversation: vi.fn(),
}));

vi.mock("../lib/conversation", () => ({ loadConversation: mocks.loadConversation }));

vi.mock("../chat/EntityHttpChatGateway", () => ({
  EntityHttpChatGateway: class {
    send = mocks.send;
  },
}));

vi.mock("../presence/Presence", () => ({
  default: ({ state, reducedMotion }: { state: string; reducedMotion?: boolean }) =>
    <div data-testid="presence" data-state={state} data-paused={reducedMotion} />,
}));

const profile = {
  name: "Vesper",
  purpose: "A quiet place to explore ideas.",
  temperament: "curious" as const,
  boundaries: "Ask before taking action.",
  identity: "aether" as const,
};

let callbacks: ChatGatewayCallbacks;

async function renderReady(element: ReactElement) {
  const view = render(element);
  await act(async () => {});
  return view;
}

function sendMessage(text = "Hello") {
  fireEvent.change(screen.getByRole("textbox", { name: "YOUR MESSAGE" }), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

describe("embodied Entity conversation", () => {
  afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadConversation.mockReset().mockResolvedValue({
      entity_id: "entity-123", name: "Your Entity", session_id: "restored-session", messages: [],
    });
    mocks.cancel.mockResolvedValue(undefined);
    mocks.dispose.mockResolvedValue(undefined);
    mocks.send.mockImplementation(async (_request, nextCallbacks) => {
      callbacks = nextCallbacks;
      return { cancel: mocks.cancel, dispose: mocks.dispose };
    });
  });

  it("derives request and text activity from real gateway callbacks without claiming speech", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    expect(screen.getByText("Ready when you are")).toBeInTheDocument();
    fireEvent.focus(screen.getByRole("textbox"));
    expect(screen.getByText("Composing text · microphone off")).toBeInTheDocument();
    sendMessage();
    expect(screen.getByText("Awaiting response")).toBeInTheDocument();
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "thinking");
    await act(async () => callbacks.onToken?.("A real token"));
    expect(screen.getByText("Receiving text")).toBeInTheDocument();
    expect(screen.getByText("A real token")).toBeInTheDocument();
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "thinking");
    await act(async () => callbacks.onDone?.({ reply: "A finished reply" }));
    expect(screen.getByText("A finished reply")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
    expect(mocks.send).toHaveBeenCalledWith(
      expect.objectContaining({ message: "Hello", sessionId: expect.any(String) }),
      expect.any(Object),
    );
  });

  it("keeps restoration idle and blocks sending until the exact persisted session is available", async () => {
    let restore!: (snapshot: ConversationSnapshot) => void;
    mocks.loadConversation.mockReturnValueOnce(new Promise<ConversationSnapshot>(resolve => { restore = resolve; }));
    render(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    expect(screen.getByText("Restoring your conversation")).toBeInTheDocument();
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "idle");
    expect(screen.getByRole("textbox")).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Too early" } });
    fireEvent.submit(screen.getByRole("textbox").closest("form")!);
    expect(mocks.send).not.toHaveBeenCalled();

    await act(async () => restore({ entity_id: "entity-123", name: "Vesper", session_id: "persisted-session-42", messages: [
      { role: "assistant", content: "A reply from before reopening." },
    ] }));
    expect(screen.getByText("A reply from before reopening.")).toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeEnabled();
    sendMessage("Continue that conversation");
    expect(mocks.send).toHaveBeenCalledWith({ message: "Continue that conversation", sessionId: "persisted-session-42" }, expect.any(Object));
    expect(screen.getByText("Awaiting response")).toBeInTheDocument();
  });

  it("times out a stalled restore without accepting its late history, then retries safely", async () => {
    vi.useFakeTimers();
    let restore!: (snapshot: ConversationSnapshot) => void;
    mocks.loadConversation.mockReturnValueOnce(new Promise<ConversationSnapshot>(resolve => { restore = resolve; }));
    render(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
    expect(screen.getByRole("alert")).toHaveTextContent("Your conversation could not be restored");
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "error");
    expect(screen.getByRole("textbox")).toBeDisabled();
    expect(mocks.send).not.toHaveBeenCalled();

    await act(async () => restore({ entity_id: "old", session_id: "old-session", messages: [{ role: "assistant", content: "Stale history" }] }));
    expect(screen.queryByText("Stale history")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeDisabled();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Retry" })));
    expect(screen.getByRole("textbox")).toBeEnabled();
    expect(screen.getByText("Ready when you are")).toBeInTheDocument();
    sendMessage("Try the restored session");
    expect(mocks.send).toHaveBeenCalledWith({ message: "Try the restored session", sessionId: "restored-session" }, expect.any(Object));
  });

  it("rejects an old connection's history after replacement restores a different session", async () => {
    let restore!: (snapshot: ConversationSnapshot) => void;
    mocks.loadConversation.mockReturnValueOnce(new Promise<ConversationSnapshot>(resolve => { restore = resolve; }));
    const { rerender } = render(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    await act(async () => rerender(<ChatPanel baseUrl="http://127.0.0.1:43143" />));
    await act(async () => restore({ entity_id: "old", session_id: "old-session", messages: [{ role: "assistant", content: "Old connection history" }] }));
    expect(screen.queryByText("Old connection history")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeEnabled();
    sendMessage("For the new connection");
    expect(mocks.send).toHaveBeenCalledWith({ message: "For the new connection", sessionId: "restored-session" }, expect.any(Object));
  });

  it("stops immediately before a stream handle arrives and ignores late output", async () => {
    let resolveStream!: (stream: ChatGatewayStream) => void;
    mocks.send.mockImplementation((_request, nextCallbacks) => {
      callbacks = nextCallbacks;
      return new Promise<ChatGatewayStream>((resolve) => { resolveStream = resolve; });
    });
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    sendMessage();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByText("Reply paused here")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
    await act(async () => {
      callbacks.onToken?.("This must be ignored");
      resolveStream({ cancel: mocks.cancel, dispose: mocks.dispose });
    });
    expect(mocks.cancel).toHaveBeenCalledOnce();
    expect(screen.queryByText("This must be ignored")).not.toBeInTheDocument();
    expect(within(screen.getByRole("log")).queryByText("Vesper")).not.toBeInTheDocument();
  });

  it("keeps partial text when stopped and never overwrites it with a late completion", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    sendMessage();
    await act(async () => callbacks.onToken?.("Partial answer"));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await act(async () => callbacks.onDone?.({ reply: "Late final answer" }));
    expect(screen.getByText("Partial answer")).toBeInTheDocument();
    expect(screen.queryByText("Late final answer")).not.toBeInTheDocument();
    expect(mocks.cancel).toHaveBeenCalledOnce();
  });

  it("guards against simultaneous submit events before React updates", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Send once" } });
    const form = screen.getByRole("textbox").closest("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(mocks.send).toHaveBeenCalledOnce();
  });

  it("surfaces transport errors, removes empty responses, and permits retry", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    sendMessage();
    await act(async () => callbacks.onError?.({
      code: "transport", message: "The local Entity is unavailable.", interrupted: false,
    }));
    expect(screen.getByRole("alert")).toHaveTextContent("The local Entity is unavailable.");
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "error");
    expect(within(screen.getByRole("log")).queryByText("Vesper")).not.toBeInTheDocument();
    sendMessage("Try again");
    expect(mocks.send).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("disposes streams when a connection changes and isolates the new transcript", async () => {
    const { rerender } = await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    sendMessage("Old conversation");
    await act(async () => callbacks.onToken?.("Old reply"));
    const oldCallbacks = callbacks;
    rerender(<ChatPanel baseUrl="http://127.0.0.1:43143" profile={profile} />);
    expect(mocks.dispose).toHaveBeenCalledOnce();
    expect(screen.queryByText("Old conversation")).not.toBeInTheDocument();
    await act(async () => oldCallbacks.onDone?.({ reply: "Stale reply" }));
    expect(screen.queryByText("Stale reply")).not.toBeInTheDocument();
  });

  it("disposes a delayed stream if the panel unmounts before the handle arrives", async () => {
    let resolveStream!: (stream: ChatGatewayStream) => void;
    mocks.send.mockImplementation(() => new Promise<ChatGatewayStream>((resolve) => { resolveStream = resolve; }));
    const { unmount } = await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    unmount();
    await act(async () => resolveStream({ cancel: mocks.cancel, dispose: mocks.dispose }));
    expect(mocks.dispose).toHaveBeenCalledOnce();
  });

  it("keeps demo state controls explicit and never sends a preview draft", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} demo />);
    expect(screen.getByText(/Design preview · no model connected/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Speaking" }));
    expect(screen.getByText("Speaking · demonstration; no audio")).toBeInTheDocument();
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "speaking");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Private preview draft" } });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(mocks.send).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox")).toHaveValue("Private preview draft");
    expect(mocks.loadConversation).not.toHaveBeenCalled();
  });

  it("offers motion pause and a text fallback without removing status or the composer", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Pause motion" }));
    expect(screen.getByTestId("presence")).toHaveAttribute("data-paused", "true");
    fireEvent.click(screen.getByRole("checkbox", { name: "Text only" }));
    expect(screen.queryByTestId("presence")).not.toBeInTheDocument();
    expect(screen.getByText("Ready when you are")).toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it.each(["unmount", "reconnect"])("disposes a stopped turn's delayed handle after %s", async (transition) => {
    let resolveStream!: (stream: ChatGatewayStream) => void;
    mocks.send.mockImplementation(() => new Promise<ChatGatewayStream>((resolve) => { resolveStream = resolve; }));
    const { unmount, rerender } = await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));

    if (transition === "unmount") unmount();
    else rerender(<ChatPanel baseUrl="http://127.0.0.1:43143" />);
    await act(async () => resolveStream({ cancel: mocks.cancel, dispose: mocks.dispose }));

    expect(mocks.dispose).toHaveBeenCalledOnce();
    expect(mocks.cancel).toHaveBeenCalledOnce();
  });

  it("does not carry an unsent draft into a replacement connection", async () => {
    const { rerender } = await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Draft for the first Entity" } });

    rerender(<ChatPanel baseUrl="http://127.0.0.1:43143" />);

    expect(screen.getByRole("textbox")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(mocks.send).not.toHaveBeenCalled();
  });

  it("keeps a new turn active when a stopped turn emits late terminal callbacks", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage("First turn");
    const firstCallbacks = callbacks;
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    sendMessage("Second turn");
    const secondCallbacks = callbacks;

    await act(async () => {
      firstCallbacks.onToken?.("Stale text");
      firstCallbacks.onDone?.({ reply: "Stale completion" });
      firstCallbacks.onError?.({ code: "transport", message: "Stale failure", interrupted: false });
    });

    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    expect(screen.getByText("Awaiting response")).toBeInTheDocument();
    expect(screen.queryByText(/Stale/)).not.toBeInTheDocument();
    await act(async () => secondCallbacks.onDone?.({ reply: "Current reply" }));
    expect(screen.getByText("Current reply")).toBeInTheDocument();
  });

  it("does not report receiving text for an empty token, and explains an empty completion", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    sendMessage();
    await act(async () => callbacks.onToken?.(""));
    expect(screen.getByText("Awaiting response")).toBeInTheDocument();

    await act(async () => callbacks.onDone?.({ reply: "" }));

    expect(screen.getByRole("alert")).toHaveTextContent("The Entity finished without a text response. Check the saved conversation before sending again.");
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "error");
    expect(within(screen.getByRole("log")).queryByText("Vesper")).not.toBeInTheDocument();
    sendMessage("Try again");
    expect(screen.getByText("Awaiting response")).toBeInTheDocument();
  });

  it.each(["", " \n"])("uses streamed text for a blank completion %j and restores text-focus state", async (reply) => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    act(() => screen.getByRole("textbox").focus());
    sendMessage();
    await act(async () => {
      callbacks.onToken?.("Streamed reply");
      callbacks.onDone?.({ reply });
    });

    expect(screen.getByText("Streamed reply")).toBeInTheDocument();
    expect(screen.getByText(/Composing text/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    act(() => screen.getByRole("textbox").blur());
    expect(screen.getByText("Ready when you are")).toBeInTheDocument();
  });

  it("preserves partial text on failure and ignores a later success callback", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    await act(async () => {
      callbacks.onToken?.("Partial result");
      callbacks.onError?.({ code: "transport", message: "Connection lost during the reply.", interrupted: false });
      callbacks.onDone?.({ reply: "Late success" });
    });

    expect(screen.getByText("Partial result")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Connection lost during the reply.");
    expect(screen.getByTestId("presence")).toHaveAttribute("data-state", "error");
    expect(screen.queryByText("Late success")).not.toBeInTheDocument();
  });

  it.each([
    ["gentle", "Find one small next step", "Help me make a manageable plan."],
    ["direct", "Turn a goal into a plan", "Help me turn a goal into three practical next steps."],
    ["curious", "Try a tiny experiment", "Help me design a small experiment"],
  ] as const)("offers an optional %s first task even after a persisted greeting, without sending or overwriting a draft", async (temperament, title, draft) => {
    mocks.loadConversation.mockResolvedValue({ entity_id: "entity-123", session_id: "restored-session", messages: [
      { role: "assistant", content: "A saved welcome from your Entity." },
    ] });
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={{ ...profile, temperament }} />);
    fireEvent.click(screen.getByRole("button", { name: title }));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toContain(draft);
    expect(screen.getByRole("textbox")).toHaveFocus();
    expect(mocks.send).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: title })).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "My own mission" } });
    fireEvent.click(screen.getByRole("button", { name: title }));
    expect(screen.getByRole("textbox")).toHaveValue("My own mission");
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(mocks.send).toHaveBeenCalledWith({ message: "My own mission", sessionId: "restored-session" }, expect.any(Object));
    expect(screen.queryByRole("heading", { name: "Where shall we begin?" })).not.toBeInTheDocument();
  });

  it("shows only saved user context on return and lets the user continue without inserting or sending text", async () => {
    mocks.loadConversation.mockResolvedValue({ entity_id: "entity-123", session_id: "restored-session", messages: [
      { role: "user", content: "Explore a\nmoon garden" },
      { role: "assistant", content: "Invented fixture reply." },
    ] });
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    const context = screen.getByRole("complementary", { name: "Saved conversation context" });
    expect(context).toHaveTextContent("Last time, you asked: “Explore a moon garden”");
    expect(screen.queryByRole("heading", { name: "Where shall we begin?" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Keep my draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Pick up from here" }));
    expect(screen.getByRole("textbox")).toHaveFocus();
    expect(screen.getByRole("textbox")).toHaveValue("Keep my draft");
    expect(mocks.send).not.toHaveBeenCalled();
    sendMessage("A new unsaved question");
    expect(screen.queryByRole("complementary", { name: "Saved conversation context" })).not.toBeInTheDocument();
  });

  it("explains a slow first response, clears the wait on actual text, and resets the timer for the next turn", async () => {
    vi.useFakeTimers();
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    expect(screen.getByText("Opening a reply").closest('[role="status"]')).toHaveAttribute("aria-atomic", "true");
    await act(async () => { await vi.advanceTimersByTimeAsync(12_000); });
    expect(screen.getByText("Still waiting for a reply")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    await act(async () => callbacks.onToken?.("A first word"));
    expect(screen.getByText("Reply arriving")).toBeInTheDocument();
    expect(screen.queryByText("Still waiting for a reply")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.queryByText("Reply arriving")).not.toBeInTheDocument();
    sendMessage("Next request");
    expect(screen.getByText("Opening a reply")).toBeInTheDocument();
    await act(async () => callbacks.onDone?.({ reply: "Done" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(12_000); });
    expect(screen.queryByText("Still waiting for a reply")).not.toBeInTheDocument();
  });

  it("checks saved history after Stop without replaying, preserves the draft, and blocks a concurrent send", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" profile={profile} />);
    sendMessage("Original request");
    const oldCallbacks = callbacks;
    await act(async () => callbacks.onToken?.("Visible partial"));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByText(/Stop was requested; remote work may still finish/)).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "My next draft" } });
    let resolve!: (snapshot: ConversationSnapshot) => void;
    mocks.loadConversation.mockReturnValueOnce(new Promise<ConversationSnapshot>(r => { resolve = r; }));
    fireEvent.click(screen.getByRole("button", { name: "Check saved conversation" }));
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.getByRole("textbox")).toBeEnabled();
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(screen.getByText("Visible partial")).toBeInTheDocument();
    await act(async () => resolve({ entity_id: "entity-123", session_id: "restored-session", messages: [
      { role: "user", content: "Original request" }, { role: "assistant", content: "Saved final reply" },
    ] }));
    await act(async () => oldCallbacks.onDone?.({ reply: "Late callback" }));
    expect(screen.getByText("Saved final reply")).toBeInTheDocument();
    expect(screen.queryByText("Late callback")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toHaveValue("My next draft");
    expect(mocks.send).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Check saved conversation" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("never turns the Stop click into submitting a draft already in the composer", async () => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage("Request in progress");
    await act(async () => callbacks.onToken?.("Partial reply"));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Do not send this draft" } });
    const stopButton = screen.getByRole("button", { name: "Stop" });
    fireEvent.click(stopButton);
    expect(screen.getByRole("textbox")).toHaveValue("Do not send this draft");
    expect(screen.getByRole("textbox")).toHaveFocus();
    expect(mocks.send).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Send" })).not.toBe(stopButton);
    expect(screen.getByText(/Stop was requested; remote work may still finish/)).toBeInTheDocument();
  });

  it.each(["failure", "timeout"])("keeps partial text and a draft after a saved-history %s, without accepting late history", async (failure) => {
    vi.useFakeTimers();
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    await act(async () => {
      callbacks.onToken?.("Partial text to keep");
      callbacks.onError?.({ code: "transport", message: "Offline", interrupted: false });
    });
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Draft to keep" } });
    let resolve!: (snapshot: ConversationSnapshot) => void;
    if (failure === "failure") mocks.loadConversation.mockRejectedValueOnce(new Error("offline"));
    else mocks.loadConversation.mockReturnValueOnce(new Promise<ConversationSnapshot>(r => { resolve = r; }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Check saved conversation" })));
    if (failure === "timeout") {
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
      await act(async () => resolve({ entity_id: "entity-123", session_id: "restored-session", messages: [{ role: "assistant", content: "Too late" }] }));
    }
    expect(screen.getByRole("alert")).toHaveTextContent("Nothing was resent");
    expect(screen.getByText("Partial text to keep")).toBeInTheDocument();
    expect(screen.queryByText("Too late")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toHaveValue("Draft to keep");
    expect(screen.getByRole("button", { name: "Check saved conversation" })).toBeEnabled();
    expect(mocks.send).toHaveBeenCalledOnce();
  });

  it.each([
    { entity_id: "wrong-entity", session_id: "restored-session" },
    { entity_id: "entity-123", session_id: "wrong-session" },
  ])("fails closed when rechecking returns another identity or session: %j", async (identity) => {
    await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    mocks.loadConversation.mockResolvedValueOnce({ ...identity, messages: [{ role: "assistant", content: "Must not expose" }] });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Check saved conversation" })));
    expect(screen.getByRole("alert")).toHaveTextContent("changed identity or session");
    expect(screen.queryByText("Must not expose")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeDisabled();
    expect(mocks.send).toHaveBeenCalledOnce();
  });

  it("ignores a saved-history check after changing connections", async () => {
    const view = await renderReady(<ChatPanel baseUrl="http://127.0.0.1:43142" />);
    sendMessage();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    let resolve!: (snapshot: ConversationSnapshot) => void;
    mocks.loadConversation.mockReturnValueOnce(new Promise<ConversationSnapshot>(r => { resolve = r; }));
    fireEvent.click(screen.getByRole("button", { name: "Check saved conversation" }));
    view.rerender(<ChatPanel baseUrl="http://127.0.0.1:43143" />);
    await act(async () => resolve({ entity_id: "entity-123", session_id: "restored-session", messages: [{ role: "assistant", content: "Old connection history" }] }));
    expect(screen.queryByText("Old connection history")).not.toBeInTheDocument();
    expect(screen.queryByText(/Nothing was resent/)).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeEnabled();
    expect(mocks.send).toHaveBeenCalledOnce();
  });
});

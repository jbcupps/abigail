import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatPanel from "./ChatPanel";

const json = (data: unknown) => new Response(JSON.stringify({ ok: true, data }), { headers: { "Content-Type": "application/json" } });
const sse = (body: string) => new Response(body, { headers: { "Content-Type": "text/event-stream" } });
const base = "http://127.0.0.1:43142";

afterEach(() => vi.unstubAllGlobals());

describe("Durable Entity conversations", () => {
  it("restores named history, streams a reply, and resumes the same conversation after reopening on a new port", async () => {
    const history = [{ role: "user", content: "Remember our appointment" }, { role: "assistant", content: "Friday at 3 PM." }];
    let calls = 0;
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: history });
      expect(url).toContain("/v1/chat/stream");
      const request = JSON.parse(init?.body as string);
      expect(request).toMatchObject({ message: "What time?", session_id: "entity-ada" });
      calls += 1;
      history.push({ role: "user", content: request.message }, { role: "assistant", content: "Three in the afternoon." });
      return sse('event: token\ndata: Three in the afternoon.\n\nevent: done\ndata: {"reply":"Three in the afternoon."}\n\n');
    }));
    const first = render(<ChatPanel baseUrl={base} />);
    expect(await screen.findByText("Friday at 3 PM.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ada" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), { target: { value: "What time?" } });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message" }), { key: "Enter" });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message" }), { key: "Enter" });
    expect(await screen.findByText("Three in the afternoon.")).toBeInTheDocument();
    expect(calls).toBe(1);
    first.unmount();
    render(<ChatPanel baseUrl="http://127.0.0.1:50001" />);
    expect(await screen.findByText("Three in the afternoon.")).toBeInTheDocument();
    expect(screen.getByText("Remember our appointment")).toBeInTheDocument();
  });

  it("blocks new messages when history fails, and restores them on Retry", async () => {
    let failHistory = true;
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (failHistory) throw new Error("offline");
      return json({ session_id: null, messages: [] });
    }));
    render(<ChatPanel baseUrl={base} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be restored");
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    failHistory = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Hi, I'm Ada. What can I help you with today?")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();
  });

  it("shows a stream failure without silently replaying the message or executing tools again", async () => {
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      requests.push(url);
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: null, messages: [] });
      expect(JSON.parse(init?.body as string).session_id).toBe("entity-ada");
      return sse("event: token\ndata: Partial reply\n\n");
    }));
    render(<ChatPanel baseUrl={base} />);
    await screen.findByText("Hi, I'm Ada. What can I help you with today?");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), { target: { value: "Please help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText(/The reply could not be completed/)).toBeInTheDocument();
    expect(requests.filter((url) => url.endsWith("/v1/chat/stream"))).toHaveLength(1);
    expect(requests.some((url) => url.endsWith("/v1/chat"))).toBe(false);
    expect(screen.getByText("Partial reply")).toBeInTheDocument();
  });

  it("cancels the active request when its window closes", async () => {
    const cancelled = vi.fn();
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: [] });
      if (url.endsWith("/cancel")) {
        cancelled(JSON.parse(init?.body as string));
        return json({ cancelled: true });
      }
      return new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
      });
    }));
    const view = render(<ChatPanel baseUrl={base} />);
    await screen.findByText("Hi, I'm Ada. What can I help you with today?");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), { target: { value: "Please help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("button", { name: "Stop" });
    await act(async () => view.unmount());
    await waitFor(() => expect(cancelled).toHaveBeenCalledWith({ session_id: "entity-ada" }));
  });
});

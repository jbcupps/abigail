import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatPanel from "./ChatPanel";
import { defaultProfile } from "../presence/profile";

const json = (data: unknown) => new Response(JSON.stringify({ ok: true, data }), { headers: { "Content-Type": "application/json" } });
const sse = (body: string) => new Response(body, { headers: { "Content-Type": "text/event-stream" } });
const base = "http://127.0.0.1:43142";

afterEach(() => vi.unstubAllGlobals());

describe("Durable Entity conversations", () => {
  it("reconciles a result saved after Stop with GETs only, retaining the next draft", async () => {
    let saved = false;
    const requests: Array<{ url: string; method: string }> = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      requests.push({ url, method: init?.method || "GET" });
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: saved ? [
        { role: "user", content: "Original request" }, { role: "assistant", content: "Result saved by the fixture" },
      ] : [] });
      if (url.endsWith("/cancel")) {
        saved = true;
        return json({ cancelled: true });
      }
      expect(url).toContain("/v1/chat/stream");
      return new Response(new ReadableStream({ start(controller) {
        controller.enqueue(new TextEncoder().encode("event: token\ndata: A partial reply\n\n"));
        init?.signal?.addEventListener("abort", () => controller.close(), { once: true });
      } }), { headers: { "Content-Type": "text/event-stream" } });
    }));
    render(<ChatPanel baseUrl={base} expectedEntityId="ada" />);
    await screen.findByRole("heading", { name: "Where shall we begin?" });
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Original request" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("A partial reply");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "A draft to keep" } });
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByText("A partial reply")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Check saved conversation" }));
    await screen.findByText("Result saved by the fixture");
    expect(screen.getByRole("textbox")).toHaveValue("A draft to keep");
    expect(requests.filter(request => request.url.endsWith("/v1/chat/stream"))).toHaveLength(1);
    expect(requests.filter(request => request.method === "POST")).toEqual([
      { url: `${base}/v1/chat/stream`, method: "POST" }, { url: `${base}/v1/chat/cancel`, method: "POST" },
    ]);
    expect(requests.filter(request => request.url.includes("/history"))).toHaveLength(2);
  });

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
    const first = render(<ChatPanel baseUrl={base} expectedEntityId="ada" />);
    expect(await screen.findByText("Friday at 3 PM.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ada" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "YOUR MESSAGE" }), { target: { value: "What time?" } });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "YOUR MESSAGE" }), { key: "Enter" });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "YOUR MESSAGE" }), { key: "Enter" });
    expect(await screen.findByText("Three in the afternoon.")).toBeInTheDocument();
    expect(calls).toBe(1);
    first.unmount();
    render(<ChatPanel baseUrl="http://127.0.0.1:50001" expectedEntityId="ada" />);
    expect(await screen.findByText("Three in the afternoon.")).toBeInTheDocument();
    expect(screen.getByText("Remember our appointment")).toBeInTheDocument();
  });

  it("never exposes another Entity's history or sends when a selected runtime URL answers with a different ID", async () => {
    let entityId = "other";
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      requests.push(url);
      if (url.includes("/v1/status")) return json({ entity_id: entityId, name: entityId === "ada" ? "Ada" : "Other Entity" });
      if (url.includes("/history")) return json({
        session_id: `persisted-${entityId}`,
        messages: [{ role: "assistant", content: entityId === "ada" ? "Ada's fixture history" : "Other Entity's fixture history" }],
      });
      throw new Error("No message should be sent");
    }));
    render(<ChatPanel baseUrl={base} expectedEntityId="ada" profile={defaultProfile("Ada")} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(/different Entity/i);
    expect(screen.queryByText("Other Entity's fixture history")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    fireEvent.submit(screen.getByRole("textbox").closest("form")!);
    expect(requests.some(url => url.includes("/chat/stream"))).toBe(false);

    // Explicit retry may restore the selected identity after the endpoint is corrected.
    entityId = "ada";
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Ada's fixture history")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeEnabled();
    expect(screen.queryByText("Other Entity's fixture history")).not.toBeInTheDocument();
  });

  it("clears the old transcript and draft when a replacement port does not belong to the selected Entity", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      const original = url.startsWith(base);
      if (url.includes("/v1/status")) return json({ entity_id: original ? "ada" : "other", name: original ? "Ada" : "Other" });
      return json({ session_id: original ? "ada-session" : "other-session", messages: [
        { role: "assistant", content: original ? "Ada fixture conversation" : "Other fixture conversation" },
      ] });
    }));
    const view = render(<ChatPanel baseUrl={base} expectedEntityId="ada" />);
    await screen.findByText("Ada fixture conversation");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Unsent fixture draft" } });
    view.rerender(<ChatPanel baseUrl="http://127.0.0.1:50001" expectedEntityId="ada" />);
    expect(await screen.findByRole("alert")).toHaveTextContent(/different Entity/i);
    expect(screen.queryByText("Ada fixture conversation")).not.toBeInTheDocument();
    expect(screen.queryByText("Other fixture conversation")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toHaveValue("");
    expect(screen.getByRole("textbox")).toBeDisabled();
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
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeDisabled();
    failHistory = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Hi, I'm Ada. What can I help you with today?")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeEnabled();
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
    fireEvent.change(screen.getByRole("textbox", { name: "YOUR MESSAGE" }), { target: { value: "Please help" } });
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
    fireEvent.change(screen.getByRole("textbox", { name: "YOUR MESSAGE" }), { target: { value: "Please help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("button", { name: "Stop" });
    await act(async () => view.unmount());
    await waitFor(() => expect(cancelled).toHaveBeenCalledWith({ session_id: "entity-ada" }));
  });
});

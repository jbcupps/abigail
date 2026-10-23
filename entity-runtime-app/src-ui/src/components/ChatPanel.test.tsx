import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatPanel from "./ChatPanel";

const json = (data: unknown) => new Response(JSON.stringify({ ok: true, data }), { headers: { "Content-Type": "application/json" } });
const sse = (body: string) => new Response(body, { headers: { "Content-Type": "text/event-stream" } });
const base = "http://127.0.0.1:43142";
const presence = {
  name: "Ada",
  purpose: "Bring calm clarity",
  temperament: "playful",
  boundaries: "Ask before acting",
  identity: "aurora",
  crest: "constellation",
  motion: "lively",
};

afterEach(() => vi.unstubAllGlobals());

describe("Durable Entity conversations", () => {
  it("renders the canonical Presence beside the Entity name without sending an Entity ID", async () => {
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      requests.push(url);
      if (url.endsWith("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) {
        return json({
          session_id: "entity-ada",
          messages: [{ role: "assistant", content: "Welcome home." }],
        });
      }
      if (url.endsWith("/v1/presence")) return json(presence);
      throw new Error(`Unexpected request: ${url}`);
    }));

    render(<ChatPanel baseUrl={base} />);

    expect(await screen.findByText("Welcome home.")).toBeInTheDocument();
    const crest = await screen.findByRole("img", { name: "Ada's Presence crest" });
    expect(crest).toHaveAttribute("data-palette", "aurora");
    expect(crest).toHaveAttribute("data-crest", "constellation");
    expect(screen.getByText("Playful · Aurora · Constellation · Lively")).toBeInTheDocument();
    expect(requests).toContain(`${base}/v1/presence`);
    expect(requests.some((url) => url.includes("/v1/entities/"))).toBe(false);
  });

  it("does not fetch Presence when the header is hidden", async () => {
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      requests.push(url);
      if (url.endsWith("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) {
        return json({ session_id: "entity-ada", messages: [] });
      }
      throw new Error(`Unexpected request: ${url}`);
    }));

    render(<ChatPanel baseUrl={base} showHeader={false} />);
    expect(await screen.findByText("Hi, I'm Ada. What can I help you with today?")).toBeInTheDocument();
    await waitFor(() => expect(requests.some((url) => url.endsWith("/v1/presence"))).toBe(false));
  });

  it.each(["missing", "unavailable"] as const)(
    "restores conversation when Presence is %s",
    async (presenceState) => {
      const requests: string[] = [];
      vi.stubGlobal("fetch", vi.fn(async (url: string) => {
        requests.push(url);
        if (url.endsWith("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
        if (url.includes("/history")) {
          return json({
            session_id: "entity-ada",
            messages: [{ role: "user", content: "Keep this conversation." }],
          });
        }
        if (url.endsWith("/v1/presence")) {
          if (presenceState === "missing") return json(null);
          throw new Error("Hive is restarting");
        }
        throw new Error(`Unexpected request: ${url}`);
      }));

      render(<ChatPanel baseUrl={base} />);

      expect(await screen.findByText("Keep this conversation.")).toBeInTheDocument();
      expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();
      await waitFor(() => expect(requests).toContain(`${base}/v1/presence`));
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.queryByRole("img", { name: /Presence crest/ })).not.toBeInTheDocument();
    },
  );

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

  it("shows fixed verify-before-repeating copy for a post-tool safety block", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: [] });
      return sse(
        `event: error\ndata: ${JSON.stringify({
          code: "action_completed_verify_before_repeating",
          message: "secret-leak: sensitive backend finding",
          interrupted: false,
        })}\n\n`,
      );
    }));
    render(<ChatPanel baseUrl={base} />);
    await screen.findByText("Hi, I'm Ada. What can I help you with today?");

    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Use a tool" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(
      await screen.findByText("An action completed. Verify the result before repeating it."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/secret-leak/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/try again/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/you can continue/i)).not.toBeInTheDocument();
  });

  it("keeps a successful reply and adds the fixed completion-risk notice", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: [] });
      return sse(
        `event: done\ndata: ${JSON.stringify({
          reply: "The calendar action completed.",
          completion_risk: "action_completed_verify_before_repeating",
        })}\n\n`,
      );
    }));
    render(<ChatPanel baseUrl={base} />);
    await screen.findByText("Hi, I'm Ada. What can I help you with today?");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Use a tool" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(await screen.findByText("The calendar action completed.")).toBeInTheDocument();
    expect(
      screen.getByText("An action completed. Verify the result before repeating it."),
    ).toBeInTheDocument();
  });

  it("warns that an action may have completed when cancellation outcome is unknown", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: [] });
      if (url.endsWith("/cancel")) return json({ cancelled: true });
      return new Promise<Response>((resolve, reject) => {
        const body = new ReadableStream<Uint8Array>({ start() {} });
        resolve(new Response(body, { headers: { "Content-Type": "text/event-stream" } }));
        init?.signal?.addEventListener("abort", () => {
          reject(new DOMException("Aborted", "AbortError"));
        });
      });
    }));
    render(<ChatPanel baseUrl={base} />);
    await screen.findByText("Hi, I'm Ada. What can I help you with today?");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Use a tool" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));

    expect(
      await screen.findByText(
        "An action may have completed. Verify the result before repeating it.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/try again/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/you can continue/i)).not.toBeInTheDocument();
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

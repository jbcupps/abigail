import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatPanel from "./ChatPanel";

const json = (data: unknown) =>
  new Response(JSON.stringify({ ok: true, data }), {
    headers: { "Content-Type": "application/json" },
  });
const sse = (body: string) =>
  new Response(body, { headers: { "Content-Type": "text/event-stream" } });
const base = "http://127.0.0.1:43142";

afterEach(() => vi.unstubAllGlobals());

describe("Hive Entity chat completion-risk notices", () => {
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
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      if (url.includes("/v1/status")) return json({ entity_id: "ada", name: "Ada" });
      if (url.includes("/history")) return json({ session_id: "entity-ada", messages: [] });
      if (url.endsWith("/cancel")) return json({ cancelled: true });
      return new Response(new ReadableStream<Uint8Array>({ start() {} }), {
        headers: { "Content-Type": "text/event-stream" },
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
});

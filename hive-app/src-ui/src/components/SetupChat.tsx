import { useEffect, useRef, useState } from "react";
import { SetupButton } from "./SetupControls";
import {
  getSetupStatus,
  retrySetup,
  sendSetupMessage,
  setupHistory,
  useLocalModel,
  type SetupMessage,
  type SetupStatus,
} from "../lib/daemonClient";

export default function SetupChat({
  onConnect,
  setup,
  onStatus,
}: {
  onConnect: () => void;
  setup: SetupStatus | null;
  onStatus: (status: SetupStatus) => void;
}) {
  const [messages, setMessages] = useState<SetupMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  useEffect(() => {
    void setupHistory()
      .then(({ messages }) => setMessages(messages))
      .catch((e: Error) => setError(e.message));
  }, []);
  useEffect(() => {
    scroll.current?.scrollTo?.({ top: scroll.current.scrollHeight });
  }, [messages, busy]);
  const send = async () => {
    if (busy || !input.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const response = await sendSetupMessage(input);
      setMessages(response.messages);
      setInput("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      void getSetupStatus()
        .then(onStatus)
        .catch(() => undefined);
    } finally {
      setBusy(false);
    }
  };
  const retryLocal = async () => {
    setBusy(true);
    setError(null);
    try {
      let status = await retrySetup();
      const deadline = Date.now() + 240_000;
      while (
        ["pending", "loading"].includes(status.phase) &&
        Date.now() < deadline
      ) {
        await new Promise((resolve) => setTimeout(resolve, 750));
        status = await getSetupStatus();
      }
      onStatus(status);
      if (status.phase !== "ready") setError(status.message);
    } catch {
      setError(
        "Abigail could not restart. Close and reopen the app to try again.",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex h-full flex-col bg-theme-bg-elevated">
      <div className="flex items-center justify-between border-b border-theme-border px-5 py-3 text-sm">
        <span>
          {setup?.active_provider === "local"
            ? "Abigail · On this computer"
            : `Abigail · ${setup?.active_provider} · ${setup?.active_model}`}
        </span>
        {setup?.active_provider !== "local" && (
          <SetupButton
            type="button"
            disabled={busy}
            onClick={() => {
              void useLocalModel()
                .then(onStatus)
                .catch((e: Error) => setError(e.message));
            }}
            className="text-theme-primary"
          >
            Continue locally
          </SetupButton>
        )}
      </div>
      <div
        ref={scroll}
        className="flex-1 overflow-y-auto p-6 space-y-4"
        aria-live="polite"
      >
        {messages.length === 0 && (
          <div className="mx-auto max-w-xl pt-8">
            <p className="text-xl text-theme-text-bright">
              Hi, I’m Abigail. Let’s get you settled in.
            </p>
            <p className="mt-3 text-theme-text-dim">
              We can talk on this computer, even offline. I can help you connect
              a more capable API model whenever you are ready.
            </p>
            <SetupButton
              type="button"
              onClick={onConnect}
              className="mt-4 text-theme-primary hover:underline"
            >
              Connect a model
            </SetupButton>
          </div>
        )}
        {messages.map((message, index) => (
          <div
            key={index}
            className={
              message.role === "user"
                ? "flex justify-end"
                : "flex justify-start"
            }
          >
            <p
              className={`max-w-[85%] whitespace-pre-wrap rounded-theme-lg px-4 py-3 ${message.role === "user" ? "bg-theme-bubble-user" : "bg-theme-bubble-assistant"}`}
            >
              {message.content}
            </p>
          </div>
        ))}
        {busy && (
          <p className="text-sm text-theme-text-dim">Abigail is thinking…</p>
        )}
        {error && (
          <p role="alert" className="text-sm text-theme-danger">
            {error}
          </p>
        )}
        {setup?.phase === "error" && (
          <SetupButton disabled={busy} onClick={() => void retryLocal()}>
            Retry local model
          </SetupButton>
        )}
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
        className="flex flex-wrap gap-3 border-t border-theme-border p-4"
      >
        <input
          aria-label="Message Abigail"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          maxLength={4000}
          placeholder="Ask me about getting started…"
          className="min-w-0 flex-1 rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2"
        />
        <SetupButton
          type="submit"
          disabled={busy || !input.trim()}
          variant="primary"
        >
          Send
        </SetupButton>
      </form>
    </div>
  );
}

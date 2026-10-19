import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Presence, { type PresenceMedia } from "./Presence";

describe("presence media and motion", () => {
  let preferenceListeners: Set<() => void>;
  let mediaQuery: {
    matches: boolean;
    addEventListener: ReturnType<typeof vi.fn>;
    removeEventListener: ReturnType<typeof vi.fn>;
  };

  beforeEach(() => {
    preferenceListeners = new Set();
    mediaQuery = {
      matches: false,
      addEventListener: vi.fn((_event: string, listener: () => void) => preferenceListeners.add(listener)),
      removeEventListener: vi.fn((_event: string, listener: () => void) => preferenceListeners.delete(listener)),
    };
    vi.stubGlobal("matchMedia", vi.fn(() => mediaQuery));
    vi.stubGlobal("fetch", vi.fn());
    vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("labels its procedural portrait and does not invent a runtime state", () => {
    const { container, rerender } = render(<Presence />);
    expect(screen.getByRole("img", { name: "Procedural portrait" }).tagName.toLowerCase()).toBe("svg");
    expect(container.firstElementChild).toHaveClass("state-idle");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    rerender(<Presence state="interrupted" />);
    expect(container.firstElementChild).toHaveClass("state-interrupted");
    expect(container.firstElementChild).not.toHaveClass("state-speaking");
  });

  it.each<{ title: string; media: PresenceMedia }>([
    { title: "remote portrait", media: { kind: "portrait", src: "https://media.example.test/portrait.png" } },
    { title: "remote video", media: { kind: "video", src: "https://media.example.test/presence.mp4" } },
    { title: "protocol-relative portrait", media: { kind: "portrait", src: "//media.example.test/portrait.png" } },
    { title: "remote state loop", media: { kind: "video", src: "/presence.mp4", loops: { speaking: "https://media.example.test/speaking.mp4" } } },
    { title: "embedded data", media: { kind: "portrait", src: "data:image/svg+xml,<svg></svg>" } },
  ])("falls back without exposing a network media element for $title", ({ media }) => {
    const { container } = render(<Presence media={media} state="speaking" />);

    expect(screen.getByRole("status")).toHaveTextContent(/Media unavailable.*procedural fallback/);
    expect(screen.getByRole("img", { name: "Procedural portrait" }).tagName.toLowerCase()).toBe("svg");
    expect(container.querySelector("video, img, [src], [poster]")).toBeNull();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("allows a same-origin clip while dropping a remote poster", () => {
    const { container } = render(<Presence media={{ kind: "video", src: "/presence.mp4", poster: "https://media.example.test/poster.png" }} />);
    const video = container.querySelector("video");

    expect(video).toHaveAttribute("src", new URL("/presence.mp4", window.location.href).href);
    expect(video).not.toHaveAttribute("poster");
    expect(video?.muted).toBe(true);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("replaces a failed portrait with the procedural fallback and can recover with a new source", () => {
    const { container, rerender } = render(<Presence media={{ kind: "portrait", src: "/portrait.png" }} label="Sol portrait" />);
    fireEvent.error(screen.getByRole("img", { name: "Sol portrait" }));

    expect(screen.getByRole("status")).toHaveTextContent(/procedural fallback/);
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByRole("img", { name: "Sol portrait" }).tagName.toLowerCase()).toBe("svg");

    rerender(<Presence media={{ kind: "portrait", src: "/replacement.png" }} label="Sol portrait" />);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(container.querySelector("img")).toHaveAttribute("src", new URL("/replacement.png", window.location.href).href);
  });

  it("falls back when video playback is rejected", async () => {
    vi.mocked(HTMLMediaElement.prototype.play).mockRejectedValueOnce(new Error("Playback unavailable"));
    const { container } = render(<Presence media={{ kind: "video", src: "/presence.mp4" }} />);

    expect(await screen.findByRole("status")).toHaveTextContent(/procedural fallback/);
    expect(container.querySelector("video")).toBeNull();
    expect(screen.getByRole("img", { name: "Procedural portrait" })).toBeInTheDocument();
  });

  it("pauses a clip and procedural motion when reduced motion is explicitly selected", () => {
    const { container, rerender } = render(<Presence media={{ kind: "video", src: "/presence.mp4" }} reducedMotion />);
    expect(container.firstElementChild).toHaveClass("motion-paused");
    expect(container.querySelector("video")).not.toHaveAttribute("autoplay");
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled();
    expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled();

    rerender(<Presence media={{ kind: "video", src: "/presence.mp4" }} reducedMotion={false} />);
    expect(container.firstElementChild).not.toHaveClass("motion-paused");
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledOnce();
  });

  it("responds to operating-system motion changes and removes its subscription on unmount", () => {
    const { container, unmount } = render(<Presence media={{ kind: "video", src: "/presence.mp4" }} />);
    expect(window.matchMedia).toHaveBeenCalledWith("(prefers-reduced-motion: reduce)");
    expect(preferenceListeners.size).toBe(1);

    act(() => {
      mediaQuery.matches = true;
      preferenceListeners.forEach(listener => listener());
    });
    expect(container.firstElementChild).toHaveClass("motion-paused");
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalledOnce();
    expect(container.querySelector("video")).not.toHaveAttribute("autoplay");

    unmount();
    expect(preferenceListeners.size).toBe(0);
  });

  it("does not start video playback when the operating system already requests reduced motion", () => {
    mediaQuery.matches = true;
    const { container } = render(<Presence media={{ kind: "video", src: "/presence.mp4" }} />);

    expect(container.firstElementChild).toHaveClass("motion-paused");
    expect(container.querySelector("video")).not.toHaveAttribute("autoplay");
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled();
    expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
  });
});

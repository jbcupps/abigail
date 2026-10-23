import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import PresenceCrest from "./PresenceCrest";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

function installMotionPreference(initial: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const query = {
    matches: initial,
    media: REDUCED_MOTION_QUERY,
    onchange: null,
    addEventListener: (_type: string, listener: EventListenerOrEventListenerObject) => {
      listeners.add(listener as (event: MediaQueryListEvent) => void);
    },
    removeEventListener: (_type: string, listener: EventListenerOrEventListenerObject) => {
      listeners.delete(listener as (event: MediaQueryListEvent) => void);
    },
    addListener: (listener: (event: MediaQueryListEvent) => void) => listeners.add(listener),
    removeListener: (listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener),
    dispatchEvent: () => true,
  } as MediaQueryList;
  vi.stubGlobal("matchMedia", vi.fn((media: string) => {
    expect(media).toBe(REDUCED_MOTION_QUERY);
    return query;
  }));
  return {
    set(matches: boolean) {
      Object.defineProperty(query, "matches", { value: matches, configurable: true });
      const event = { matches, media: REDUCED_MOTION_QUERY } as MediaQueryListEvent;
      listeners.forEach((listener) => listener(event));
    },
  };
}

afterEach(() => vi.unstubAllGlobals());

describe("PresenceCrest", () => {
  it("renders the canonical palette and crest glyph with an accessible label", () => {
    installMotionPreference(false);
    render(
      <PresenceCrest
        palette="ember"
        crest="lantern"
        motion="calm"
        size={40}
        label="Aster's Presence crest"
      />,
    );

    const crest = screen.getByRole("img", { name: "Aster's Presence crest" });
    expect(crest).toHaveAttribute("data-palette", "ember");
    expect(crest).toHaveAttribute("data-crest", "lantern");
    expect(crest).toHaveAttribute("data-motion", "calm");
    expect(crest).toHaveClass("is-calm");
    expect(crest.querySelector('path[d="M26 18h12M24 22h16l-2 22H26z"]')).not.toBeNull();
  });

  it("downgrades animation to still and reacts to live reduced-motion changes", () => {
    const preference = installMotionPreference(false);
    render(
      <PresenceCrest
        palette="aurora"
        crest="constellation"
        motion="lively"
        label="Nova's Presence crest"
      />,
    );
    const crest = screen.getByRole("img", { name: "Nova's Presence crest" });
    expect(crest).toHaveAttribute("data-motion", "lively");
    expect(crest).toHaveClass("is-lively");

    act(() => preference.set(true));
    expect(crest).toHaveAttribute("data-motion", "still");
    expect(crest).toHaveClass("is-still");
    expect(crest).not.toHaveClass("is-lively");

    act(() => preference.set(false));
    expect(crest).toHaveAttribute("data-motion", "lively");
    expect(crest).toHaveClass("is-lively");
  });
});

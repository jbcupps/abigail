import "@testing-library/jest-dom/vitest";

if (!globalThis.crypto) {
  Object.defineProperty(globalThis, "crypto", {
    value: {},
    configurable: true,
  });
}

if (!globalThis.crypto.randomUUID) {
  Object.defineProperty(globalThis.crypto, "randomUUID", {
    value: () => "00000000-0000-4000-8000-000000000000",
    configurable: true,
  });
}

if (!HTMLElement.prototype.scrollTo) {
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    value: () => undefined,
    configurable: true,
  });
}

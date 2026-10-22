import { describe, expect, it } from "vitest";
// Vitest runs in Node; the production browser bundle does not import this module.
// @ts-expect-error The Hive UI intentionally does not ship Node type declarations.
import { readFileSync } from "node:fs";

const legacyAppSource = readFileSync(
  new URL("../../dist/app.js", import.meta.url),
  "utf8",
);

describe("tracked legacy Hive reveal", () => {
  it("never renders hidden ethics weights or axis names", () => {
    expect(legacyAppSource).not.toMatch(
      /certificate\.weights|\bdeontology\b|\bteleology\b|\bareteology\b|\bwelfare\b/i,
    );
  });
});

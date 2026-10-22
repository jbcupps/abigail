import { describe, expect, it } from "vitest";
import legacyAppSource from "../../dist/app.js?raw";

describe("tracked legacy Hive reveal", () => {
  it("never renders hidden ethics weights or axis names", () => {
    expect(legacyAppSource).not.toMatch(
      /certificate\.weights|\bdeontology\b|\bteleology\b|\bareteology\b|\bwelfare\b/i,
    );
  });
});

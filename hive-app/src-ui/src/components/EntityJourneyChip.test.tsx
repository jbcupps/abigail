import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import EntityJourneyChip from "./EntityJourneyChip";
import { getEntityJourney } from "../lib/questClient";
import { entityJourneyFixture } from "../test/questFixture";

vi.mock("../lib/questClient", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/questClient")>(),
  getEntityJourney: vi.fn(),
}));

describe("Entity Journey chip", () => {
  beforeEach(() => {
    vi.mocked(getEntityJourney).mockResolvedValue(entityJourneyFixture());
  });
  afterEach(() => {
    cleanup();
    vi.resetAllMocks();
  });

  it("opens the Journey panel from a born Entity card summary", async () => {
    const onOpen = vi.fn();
    render(<EntityJourneyChip entityId="e1" onOpen={onOpen} />);
    const button = await screen.findByRole("button", { name: "Open Nova's Journey" });
    expect(button).toHaveTextContent("View journey");
    expect(screen.getByText("Level 2 · Kindling")).toBeInTheDocument();
    fireEvent.click(button);
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("stays silent when journey lookup fails", async () => {
    vi.mocked(getEntityJourney).mockRejectedValue(new Error("offline"));
    const { container } = render(<EntityJourneyChip entityId="e1" onOpen={vi.fn()} />);
    await Promise.resolve();
    expect(container).toBeEmptyDOMElement();
  });
});

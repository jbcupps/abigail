import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import CreateEntityCard from "./CreateEntityCard";

describe("Entity creation entry", () => {
  it("opens the resumable Genesis flow without creating an incomplete Entity", () => {
    const onOpen = vi.fn();
    render(<CreateEntityCard onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: "Create or resume an Entity" }));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
});

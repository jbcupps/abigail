import "../../test/tauri-mock";
import { render } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import SplashScreen from "../SplashScreen";

describe("SplashScreen", () => {
  it("renders without crashing", () => {
    const onComplete = vi.fn();
    const { container } = render(<SplashScreen onComplete={onComplete} />);
    expect(container).toBeTruthy();
  });

  it("calls onComplete on click", async () => {
    const onComplete = vi.fn();
    const { container } = render(<SplashScreen onComplete={onComplete} />);
    // SplashScreen has an onClick handler on its container
    container.firstElementChild?.dispatchEvent(
      new MouseEvent("click", { bubbles: true })
    );
    expect(onComplete).toHaveBeenCalled();
  });
});

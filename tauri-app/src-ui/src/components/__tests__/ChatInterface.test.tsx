import "../../test/tauri-mock";
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ThemeProvider } from "../../contexts/ThemeContext";
import ChatInterface from "../ChatInterface";

function renderWithTheme(ui: React.ReactElement) {
  return render(<ThemeProvider>{ui}</ThemeProvider>);
}

describe("ChatInterface", () => {
  it("renders without crashing", () => {
    renderWithTheme(<ChatInterface />);
  });

  it("shows the message input placeholder", () => {
    renderWithTheme(<ChatInterface />);
    const input = screen.getByPlaceholderText(/type|message|ask/i);
    expect(input).toBeInTheDocument();
  });
});

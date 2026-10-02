import { parse, type Rule } from "postcss";
import { describe, expect, it } from "vitest";
import appCss from "./index.css?inline";

// Exercise the same Vite/PostCSS output embedded by main.tsx. Checking only
// source palette variables misses a broken utility/config import entirely.
const stylesheet = parse(appCss);
const defaults = new Map<string, string>();
stylesheet.walkRules((rule) => {
  if (rule.selectors.some((selector) => selector.trim() === ":root")) {
    rule.walkDecls((declaration) => { defaults.set(declaration.prop, declaration.value); });
  }
});
function declaration(selector: string, property: string): string {
  let value = "";
  stylesheet.walkRules((rule: Rule) => {
    if (rule.selectors.includes(selector)) {
      rule.walkDecls(property, (item) => { value = item.value; });
    }
  });
  expect(value, `Generated ${selector} must set ${property}`).not.toBe("");
  return value;
}
function resolved(value: string): string {
  return value.replace(/var\((--[\w-]+)\)/g, (_match, name: string) => {
    const defined = defaults.get(name);
    expect(defined, `Default theme must define ${name}`).toBeTruthy();
    return resolved(defined!);
  });
}

describe("Embedded application stylesheet", () => {
  it("supplies actual default colors, layout spacing, type scale and control shape", () => {
    const background = resolved(declaration(".bg-theme-bg", "background-color"));
    const foreground = resolved(declaration(".text-theme-text", "color"));
    expect(background).not.toBe(foreground);
    expect(resolved(declaration(".bg-theme-primary", "background-color"))).not.toBe(background);
    expect(resolved(declaration(".p-8", "padding"))).toMatch(/(?:px|rem)/);
    expect(resolved(declaration(".text-2xl", "font-size"))).toMatch(/(?:px|rem)/);
    expect(resolved(declaration(".rounded-theme-md", "border-radius"))).toMatch(/(?:px|rem)/);
  });

  it("resets browser heading sizes so the generated type scale can apply", () => {
    expect(declaration("h1", "font-size")).toBe("inherit");
    expect(declaration("h1", "font-weight")).toBe("inherit");
  });
});

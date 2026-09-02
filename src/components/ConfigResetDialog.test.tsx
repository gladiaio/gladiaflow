import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ConfigResetDialog } from "./ConfigResetDialog";

describe("ConfigResetDialog", () => {
  it("clearly describes the destructive action and retained backup", () => {
    const html = renderToStaticMarkup(
      <ConfigResetDialog
        open
        isResetting={false}
        error={null}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );

    expect(html).toContain("Reset all settings?");
    expect(html).toContain("API key");
    expect(html).toContain("shortcut");
    expect(html).toContain("timestamped backup");
    expect(html).toContain("Reset all settings");
    expect(html).toContain("Cancel");
  });
});

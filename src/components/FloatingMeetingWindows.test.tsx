import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { FloatingMeetingController } from "./FloatingMeetingWindows";

describe("FloatingMeetingController", () => {
  it("renders persistent meeting controls without the main workspace", () => {
    const html = renderToStaticMarkup(<FloatingMeetingController />);

    expect(html).toContain("Open meeting");
    expect(html).toContain('aria-label="Pause meeting"');
    expect(html).toContain('aria-label="Stop meeting"');
  });
});

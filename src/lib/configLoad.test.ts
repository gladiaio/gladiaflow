import { describe, expect, it, vi } from "vitest";
import { loadSavedApiKey } from "./configLoad";

describe("loadSavedApiKey", () => {
  it("distinguishes a missing key from a load failure", async () => {
    await expect(loadSavedApiKey(async () => null)).resolves.toEqual({
      ok: true,
      apiKey: null,
    });

    const failure = await loadSavedApiKey(async () => {
      throw new Error("malformed config");
    });
    expect(failure).toEqual({
      ok: false,
      message:
        "GladiaFlow couldn't read your saved settings. Your API key has not been changed.",
    });
  });

  it("returns the saved key without logging its value", async () => {
    const invokeApiKey = vi.fn(async () => "secret-key");

    await expect(loadSavedApiKey(invokeApiKey)).resolves.toEqual({
      ok: true,
      apiKey: "secret-key",
    });
    expect(invokeApiKey).toHaveBeenCalledOnce();
  });
});

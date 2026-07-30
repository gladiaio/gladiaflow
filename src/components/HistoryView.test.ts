import { describe, expect, it } from "vitest";
import { getHistoryPaginationSummary } from "./HistoryView";

describe("getHistoryPaginationSummary", () => {
  it("describes an empty result", () => {
    expect(
      getHistoryPaginationSummary({
        isLoading: false,
        total: 0,
        currentPage: 1,
        pageSize: 4,
      }),
    ).toBe("Page 1 of 1 · 0 items");
  });

  it("describes a full middle page", () => {
    expect(
      getHistoryPaginationSummary({
        isLoading: false,
        total: 84,
        currentPage: 2,
        pageSize: 4,
      }),
    ).toBe("Page 2 of 21 · Items 5–8 of 84");
  });

  it("describes a partial final page", () => {
    expect(
      getHistoryPaginationSummary({
        isLoading: false,
        total: 10,
        currentPage: 3,
        pageSize: 4,
      }),
    ).toBe("Page 3 of 3 · Items 9–10 of 10");
  });

  it("uses a stable loading label", () => {
    expect(
      getHistoryPaginationSummary({
        isLoading: true,
        total: 84,
        currentPage: 2,
        pageSize: 4,
      }),
    ).toBe("Loading…");
  });
});

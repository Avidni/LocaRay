import type { TunnelSnapshot } from "./contracts/generated/TunnelSnapshot";
import { describe, expect, it } from "vitest";

describe("generated tunnel contract", () => {
  it("represents the backend idle state", () => {
    const snapshot: TunnelSnapshot = {
      sessionId: null,
      phase: "idle",
      port: null,
      localUrl: null,
      publicUrl: null,
      startedAt: null,
      stopAt: null,
      originReachable: null,
      tunnelConnected: false,
      error: null,
    };

    expect(snapshot.phase).toBe("idle");
    expect(snapshot.publicUrl).toBeNull();
  });
});

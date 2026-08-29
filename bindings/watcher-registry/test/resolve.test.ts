import { describe, it, expect } from "vitest";
import { createRequire } from "node:module";

// Smoke test for the package "exports" map. Runs against the compiled dist/,
// so it must execute after `npm run build`. Verifies that both a CommonJS
// require() and an ESM import() of the package entry point resolve and expose
// the generated Client plus the network address map.
const require = createRequire(import.meta.url);

describe("WatcherRegistry package resolution (post-build)", () => {
  it("resolves via require()", () => {
    const cjs = require("../dist/index.js");
    expect(typeof cjs.Client).toBe("function");
    expect(cjs.networks).toBeDefined();
  });

  it("resolves via dynamic import()", async () => {
    const esm = await import("../dist/index.js");
    expect(typeof esm.Client).toBe("function");
    expect(esm.networks).toBeDefined();
  });
});

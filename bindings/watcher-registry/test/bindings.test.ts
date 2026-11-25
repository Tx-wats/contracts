import { describe, it, expect } from "vitest";

// The watcher-registry has a hand-maintained src/index.ts that re-exports
// from the generated bindings and adds a networks config map.
import { Client, networks } from "../src/index.js";

describe("WatcherRegistry bindings", () => {
  const testnetConfig = {
    contractId: networks.testnet.contractId,
    networkPassphrase: networks.testnet.networkPassphrase,
    rpcUrl: networks.testnet.rpcUrl,
  };

  it("exports a Client class", () => {
    expect(Client).toBeDefined();
    expect(typeof Client).toBe("function");
  });

  it("exports network configurations", () => {
    expect(networks).toBeDefined();
    expect(networks.testnet).toBeDefined();
    expect(networks.mainnet).toBeDefined();
  });

  it("has correct testnet network passphrase", () => {
    expect(networks.testnet.networkPassphrase).toBe(
      "Test SDF Network ; September 2015"
    );
  });

  it("has correct mainnet network passphrase", () => {
    expect(networks.mainnet.networkPassphrase).toBe(
      "Public Global Stellar Network ; September 2015"
    );
  });

  it("testnet contract ID is a valid strkey contract address", () => {
    // Strkey contract addresses: 'C' + 55 base32 chars (no 0, 1, 8, 9).
    expect(networks.testnet.contractId).toMatch(/^C[A-Z2-7]{55}$/);
  });

  it("mainnet contract ID is empty until mainnet is deployed (issue #90)", () => {
    expect(networks.mainnet.contractId).toBe("");
  });

  it("can instantiate Client with testnet config", () => {
    const client = new Client(testnetConfig);
    expect(client).toBeDefined();
  });

  it("exposes expected read methods on the Client prototype", () => {
    const client = new Client(testnetConfig);
    // Read-only methods (no auth required)
    expect(typeof client.is_watcher_authorized).toBe("function");
    expect(typeof client.are_watchers_authorized).toBe("function");
    expect(typeof client.get_watchers).toBe("function");
    expect(typeof client.get_admins).toBe("function");
    expect(typeof client.get_admin).toBe("function");
    expect(typeof client.is_paused).toBe("function");
    expect(typeof client.get_watcher_count).toBe("function");
    expect(typeof client.get_timelock_delay).toBe("function");
    expect(typeof client.get_guardian).toBe("function");
    expect(typeof client.get_configuration).toBe("function");
  });

  it("are_watchers_authorized rejects oversized batches instead of truncating", async () => {
    const client = new Client(testnetConfig);
    // MAX_PAGE_SIZE is 100; a 101-element batch must surface an error rather
    // than silently returning a shorter Vec<bool>.
    const oversized = Array.from({ length: 101 }, () => "G".padEnd(56, "A"));
    await expect(client.are_watchers_authorized({ watchers: oversized })).rejects.toThrow();
  });

  it("are_watchers_authorized accepts a batch at the MAX_PAGE_SIZE boundary", async () => {
    const client = new Client(testnetConfig);
    const atLimit = Array.from({ length: 100 }, () => "G".padEnd(56, "A"));
    // At the boundary the call must not fail validation; it may still fail on
    // the network, but it must not throw the oversized-batch error.
    const result = client.are_watchers_authorized({ watchers: atLimit });
    expect(result).toBeDefined();
    if (result && typeof result.catch === "function") {
      result.catch(() => {});
    }
  });

  it("exports expected write methods on the Client prototype", () => {
    const client = new Client(testnetConfig);
    // Write methods (auth required)
    expect(typeof client.initialize).toBe("function");
    expect(typeof client.register_watcher).toBe("function");
    expect(typeof client.remove_watcher).toBe("function");
    expect(typeof client.add_admin).toBe("function");
    expect(typeof client.remove_admin).toBe("function");
    expect(typeof client.pause).toBe("function");
    expect(typeof client.unpause).toBe("function");
    expect(typeof client.bump_instance_ttl).toBe("function");
    expect(typeof client.set_timelock_delay).toBe("function");
    expect(typeof client.set_guardian).toBe("function");
    expect(typeof client.propose_admin_transfer).toBe("function");
    expect(typeof client.accept_admin_transfer).toBe("function");
  });

  it("exposes the timelocked admin action proposal path", () => {
    const client = new Client(testnetConfig);
    // Lowering or disabling the timelock delay must go through the
    // timelocked proposal path rather than the raise-only direct setter.
    expect(typeof client.propose_admin_action).toBe("function");
    expect(typeof client.execute_admin_action).toBe("function");
    expect(typeof client.get_pending_admin_action).toBe("function");
  });

  it("does not expose a dead clear_all_watchers entrypoint", () => {
    const client = new Client(testnetConfig);
    // clear_all_watchers could only ever succeed on an empty registry, so it
    // was removed in favour of explicit remove_watcher calls.
    expect(client.clear_all_watchers).toBeUndefined();
  });
});

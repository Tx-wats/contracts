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
    expect(typeof client.is_authorized).toBe("function");
    expect(typeof client.get_watchers).toBe("function");
    expect(typeof client.get_admins).toBe("function");
    expect(typeof client.get_admin).toBe("function");
  });

  it("exposes expected write methods on the Client prototype", () => {
    const client = new Client(testnetConfig);
    // Write methods (auth required)
    expect(typeof client.initialize).toBe("function");
    expect(typeof client.register_watcher).toBe("function");
    expect(typeof client.remove_watcher).toBe("function");
    expect(typeof client.add_admin).toBe("function");
    expect(typeof client.remove_admin).toBe("function");
    expect(typeof client.transfer_admin).toBe("function");
  });
});

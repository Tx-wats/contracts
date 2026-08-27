# Deployments

This file tracks deployed contract addresses for each network.  
After running `scripts/deploy.sh`, replace the placeholder values with the printed addresses and commit the update.

> **Address format:** Stellar contract addresses are 56-character strings beginning with `C`  
> (e.g. `CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4`).  
> Testnet addresses are only valid until the next testnet reset.

---

## Stellar Testnet

| Contract | Address | WASM Hash |
|---|---|---|
| Alert Registry | `CDSO4GGZH7KBUQYKOIQDCMCFSRYEPOVDUX7Z4IB5TWNTLT2GDRKDQOYR` | `TODO` |
| Watcher Registry | `CCSHRYACRNVSLC5NP3V2DL6LGID57TQT2TJXVUVXBBZX6SED6N3F7X6J` | `TODO` |

Deployed and initialized on 2026-08-17. Verified end to end on-chain:
`register_alert` → `get_alert`, then a full `propose_webhook` → `confirm_webhook`
rotation emitting `alert.wh_prop` and `alert.wh_conf`.

- [Alert Registry on Stellar Expert](https://stellar.expert/explorer/testnet/contract/CDSO4GGZH7KBUQYKOIQDCMCFSRYEPOVDUX7Z4IB5TWNTLT2GDRKDQOYR)
- [Watcher Registry on Stellar Expert](https://stellar.expert/explorer/testnet/contract/CCSHRYACRNVSLC5NP3V2DL6LGID57TQT2TJXVUVXBBZX6SED6N3F7X6J)

### Testnet Network Details

| Parameter | Value |
|---|---|
| Network | Testnet |
| RPC URL | `https://soroban-testnet.stellar.org` |
| Network Passphrase | `Test SDF Network ; September 2015` |
| Horizon URL | `https://horizon-testnet.stellar.org` |

> Testnet resets periodically. Re-deploy with `bash scripts/deploy.sh` and update the addresses above after each reset.

---

## Stellar Mainnet

| Contract | Address | WASM Hash |
|---|---|---|
| Alert Registry | `CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX` | `TODO` |
| Watcher Registry | `CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX` | `TODO` |

### Mainnet Network Details

| Parameter | Value |
|---|---|
| Network | Mainnet (Public) |
| RPC URL | `https://mainnet.stellar.validationcloud.io/v1/<API_KEY>` |
| Network Passphrase | `Public Global Stellar Network ; September 2015` |
| Horizon URL | `https://horizon.stellar.org` |

> Mainnet has not been deployed yet. Replace placeholders and remove this note once a production deployment is made.

---

## How to Update This File

1. Run the deploy script:
   ```bash
   bash scripts/deploy.sh
   ```
2. Copy the contract addresses and WASM hashes printed at the end of the script output.
3. Replace the corresponding `CXXX...` placeholders and `TODO` hashes in the table above.
4. Commit the update:
   ```bash
   git add DEPLOYMENTS.md
   git commit -m "deploy: update contract addresses for <network> (<date>)"
   ```

---

## Deployment History

| Date | Network | Contract | Address | WASM Hash | Notes |
|---|---|---|---|---|---|
| — | — | — | — | — | Initial placeholder |

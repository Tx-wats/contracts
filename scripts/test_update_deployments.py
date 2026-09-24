#!/usr/bin/env python3
"""
Unit tests for update_deployments.py.
"""

import unittest
from update_deployments import update_deployments_content

SAMPLE_DEPLOYMENTS = """# Deployments

## Stellar Testnet

| Contract | Address | WASM Hash |
|---|---|---|
| Alert Registry | `CDSO4GGZH7KBUQYKOIQDCMCFSRYEPOVDUX7Z4IB5TWNTLT2GDRKDQOYR` | `TODO` |
| Watcher Registry | `CCSHRYACRNVSLC5NP3V2DL6LGID57TQT2TJXVUVXBBZX6SED6N3F7X6J` | `TODO` |

## Stellar Mainnet

| Contract | Address | WASM Hash |
|---|---|---|
| Alert Registry | `CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX` | `TODO` |
| Watcher Registry | `CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX` | `TODO` |

## Deployment History

| Date | Network | Contract | Address | WASM Hash | Notes |
|---|---|---|---|---|---|
| — | — | — | — | — | Initial placeholder |
"""

class TestUpdateDeployments(unittest.TestCase):
    def test_update_testnet(self):
        updated = update_deployments_content(
            SAMPLE_DEPLOYMENTS,
            network="testnet",
            alert_id="CALERTTESTNET123",
            watcher_id="CWATCHERTESTNET123",
            alert_hash="hash_alert_1",
            watcher_hash="hash_watcher_1",
            tag="v0.2.0",
            date_str="2026-09-24"
        )
        self.assertIn("| Alert Registry | `CALERTTESTNET123` | `hash_alert_1` |", updated)
        self.assertIn("| Watcher Registry | `CWATCHERTESTNET123` | `hash_watcher_1` |", updated)
        # Check history row includes Network column
        self.assertIn("| 2026-09-24 | Testnet | Alert Registry | `CALERTTESTNET123` | `hash_alert_1` | v0.2.0 |", updated)
        # Check placeholder is not damaged
        self.assertIn("| — | — | — | — | — | Initial placeholder |", updated)

    def test_update_mainnet(self):
        updated = update_deployments_content(
            SAMPLE_DEPLOYMENTS,
            network="mainnet",
            alert_id="CALERTMAINNET123",
            watcher_id="CWATCHERMAINNET123",
            alert_hash="mainnet_hash_1",
            watcher_hash="mainnet_hash_2",
            notes="Production release",
            date_str="2026-09-24"
        )
        self.assertIn("| Alert Registry | `CALERTMAINNET123` | `mainnet_hash_1` |", updated)
        self.assertIn("| Watcher Registry | `CWATCHERMAINNET123` | `mainnet_hash_2` |", updated)
        self.assertIn("| 2026-09-24 | Mainnet | Alert Registry | `CALERTMAINNET123` | `mainnet_hash_1` | Production release |", updated)

if __name__ == "__main__":
    unittest.main()

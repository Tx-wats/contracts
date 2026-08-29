# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Please report security issues by emailing:

**emmanuelanalaba@gmail.com**

Include as much detail as possible:
- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- Affected contract(s): `alert-registry`, `watcher-registry`, or both
- Any suggested mitigations

### What to expect

| Timeline | Action |
|----------|--------|
| Within 48 hours | Acknowledgement of your report |
| Within 7 days | Initial assessment and severity classification |
| Within 30 days | Patch or mitigation plan communicated to reporter |
| After fix is deployed | Public disclosure coordinated with reporter |

We follow responsible disclosure: we ask that you give us reasonable time to address the issue before any public disclosure.

## Scope

The following are in scope:

- Logic errors in `AlertRegistry` or `WatcherRegistry` contract functions
- Authorization bypass (e.g., circumventing `require_auth()`)
- Storage manipulation or data corruption vectors
- Denial-of-service via resource exhaustion on-chain

The following are **out of scope**:

- Issues in third-party dependencies (report those upstream)
- Stellar protocol-level vulnerabilities (report to the [Stellar Development Foundation](https://stellar.org/bug-bounty))
- Issues in off-chain infrastructure not part of this repository

## Incident Response & Operator Runbook

In the event of a suspected or confirmed compromise of an administrative private key, operators should immediately follow the step-by-step procedures outlined in the [Incident Response Runbook](docs/incident-response.md):

* **Immediate containment actions** (revoking admin privileges via `remove_admin` or `transfer_admin`)
* **State audit & remediation** (purging unauthorized watchers, restoring watcher gating, validating per-owner limits)
* **Stakeholder notification procedures** for downstream watcher nodes and alert owners
* **Post-incident review and key hardening**

See [`docs/incident-response.md`](docs/incident-response.md) for full operational guidance.

## Contact

Maintainer: Emmanuel Chukwunyere — emmanuelanalaba@gmail.com  
Organization: [Tx-wat](https://github.com/Tx-wat)


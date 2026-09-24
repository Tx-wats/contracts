# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

### Primary channel: GitHub Private Vulnerability Reporting

Report through GitHub's private vulnerability reporting:

**<https://github.com/Tx-wats/contracts/security/advisories/new>**

(or **Security → Advisories → Report a vulnerability** on the repository page).

This is the preferred channel because:

- the report is visible to **every** repository maintainer, not to a single
  person, so it is not lost if one maintainer is unavailable;
- it stays private and is never sent over plain email;
- the fix, the CVE request and the eventual public advisory are coordinated in
  one place, together with you.

### Backup contacts

Use these only if you cannot use GitHub's private reporting, or if you have had
no acknowledgement within the timeline below:

| Role | Contact |
|------|---------|
| Primary maintainer | Emmanuel Chukwunyere — emmanuelanalaba@gmail.com |
| Secondary maintainer | [@Valreb001](https://github.com/Valreb001) (repository co-owner, see [`CODEOWNERS`](.github/CODEOWNERS)) |

When using a backup contact, send only a short note asking for a private
channel. Do not include exploit details in plain email or in a public GitHub
mention.

### What to include

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

### For maintainers

Private vulnerability reporting is a repository setting
(**Settings → Code security → Private vulnerability reporting → Enable**). A
repository admin can also enable it, and check that it is on, with:

```bash
gh api -X PUT repos/Tx-wats/contracts/private-vulnerability-reporting
gh api repos/Tx-wats/contracts/private-vulnerability-reporting   # {"enabled": true}
```

Keep at least two maintainers subscribed to security advisories so a report
never depends on one person being available.

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

Security reports: [GitHub private vulnerability reporting](https://github.com/Tx-wats/contracts/security/advisories/new) (preferred)  
Maintainers: Emmanuel Chukwunyere (emmanuelanalaba@gmail.com), [@Valreb001](https://github.com/Valreb001)  
Organization: [Tx-wats](https://github.com/Tx-wats)

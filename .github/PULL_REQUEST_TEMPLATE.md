<!-- Please provide a short description of the changes in this PR. -->

## Description
- What changed and why.

## Related Issue
- Link any related issue(s) prefixed with `#`.
- If this PR **closes** an issue, write `Closes #N` on its own line so GitHub
  auto-closes it on merge.

## Type of change
- Bugfix
- New feature
- Docs update
- Chore

## Regression tests

> **Required for every bugfix.**
> If this PR closes an issue, it must include at least one test that would
> have caught the original bug — a test that fails on `main` before the fix
> and passes after. Add it to `src/regression_tests.rs` (or the relevant
> `#[cfg(test)]` module) with a doc comment that names the issue:
>
> ```rust
> /// Regression test for #N — <one-line description of the original bug>.
> #[test]
> fn test_regression_issue_N_<short_name>() { … }
> ```
>
> If no regression test is needed (e.g. pure docs or chore), explain why
> in the "Testing notes" section below.

- [ ] I have added a `regression_tests.rs` entry (or explained why none is needed)

## Checklist
- [ ] My code follows the repository style
- [ ] I have added relevant tests (unit / integration)
- [ ] I have updated or added documentation if necessary
- [ ] All CI checks pass (`ci`, `msrv`, `Check WASM binary sizes`)

## Testing notes
- How to run tests or manual verification steps.
- If no regression test was added, explain why here.

## Release notes / changelog
- Short note for changelog if this should be documented for release.

If you'd like help polishing this PR or splitting into smaller changes, ask maintainers.

#![no_main]

//! Stateful fuzz harness for `WatcherRegistry`.
//!
//! Applies a fuzzer-controlled sequence of admin/watcher/timelock operations and
//! asserts the following invariants after every step:
//!
//! 1. `W_CNT == len(get_watchers())` — the stored counter stays in sync with the list.
//! 2. `len(get_admins()) >= 1` — there is always at least one admin.
//! 3. No duplicate addresses in the watcher set.
//! 4. No duplicate addresses in the admin set.
//! 5. While a timelock delay is set and no action is queued, lowering the delay
//!    via `propose_admin_action` is accepted.
//! 6. Addresses that are not registered watchers are never authorized, so any
//!    scope or info left behind by a removed watcher has no effect.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use watcher_registry::{AdminAction, WatcherInfo, WatcherRegistry, WatcherRegistryClient};

// ── Operation vocabulary ──────────────────────────────────────────────────────

/// Each variant encodes indices into the shared address pool so the fuzzer
/// only needs to emit small integers, not full 32-byte addresses.
#[derive(Arbitrary, Debug)]
enum Op {
    RegisterWatcher  { admin_idx: u8, watcher_idx: u8 },
    RemoveWatcher    { admin_idx: u8, watcher_idx: u8 },
    ReplaceWatcher   { admin_idx: u8, old_idx: u8, new_idx: u8 },
    AddAdmin         { caller_idx: u8, new_admin_idx: u8 },
    RemoveAdmin      { caller_idx: u8, target_idx: u8 },
    ClearAllWatchers { admin_idx: u8 },
    RegisterWatchers { admin_idx: u8, watcher_mask: u16 },
    SetWatcherScope  { admin_idx: u8, watcher_idx: u8, target_mask: u16 },
    SetWatcherInfo   { admin_idx: u8, watcher_idx: u8, label: Option<u8> },
    SetTimelockDelay { caller_idx: u8, delay: u16 },
    ProposeAction    { caller_idx: u8, action: ActionKind, arg_idx: u8, delay: u16 },
    ExecuteAction    { caller_idx: u8 },
    CancelAction     { caller_idx: u8 },
    AdvanceLedger    { ledgers: u8 },
}

/// Admin actions the fuzzer may queue. `Upgrade` is excluded because executing
/// it needs a WASM hash installed on the host.
#[derive(Arbitrary, Debug)]
enum ActionKind {
    AddAdmin,
    TransferAdmin,
    ClearAllWatchers,
    SetTimelockDelay,
    /// Clear all watchers except a kept set of up to two pooled addresses.
    ClearAllWatchers { admin_idx: u8, keep_a_idx: u8, keep_b_idx: u8 },
    SetGuardian      { admin_idx: u8, guardian_idx: u8 },
    Pause            { caller_idx: u8 },
    Unpause          { caller_idx: u8 },
}

// ── Structured fuzz input ─────────────────────────────────────────────────────

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    /// Pool size is kept between 2 and 16 via modular reduction in the harness.
    pool_size: u8,
    /// The sequence of operations to apply.
    ops: Vec<Op>,
}

// ── Harness ───────────────────────────────────────────────────────────────────

fuzz_target!(|input: FuzzInput| {
    // Pool of 2-16 addresses; keeps indices meaningful without blowing the budget.
    let pool_size = (input.pool_size as usize % 15) + 2;

    let env = Env::default();
    env.mock_all_auths();

    // Build a stable address pool for this run.
    let pool: Vec<Address> = (0..pool_size)
        .map(|_| Address::generate(&env))
        .collect();

    // Resolve a fuzz-controlled index to a pooled address.
    let addr = |idx: u8| -> Address { pool[idx as usize % pool_size].clone() };

    // Build a contract `Vec` from the pool entries selected by the bits of `mask`.
    let select = |mask: u16| -> soroban_sdk::Vec<Address> {
        let mut out = soroban_sdk::Vec::new(&env);
        for (i, a) in pool.iter().enumerate() {
            if (mask >> i) & 1 == 1 {
                out.push_back(a.clone());
            }
        }
        out
    };

    // Deploy contract with pool[0] as the bootstrap admin via __constructor.
    let contract_id = env.register(WatcherRegistry, (pool[0].clone(),));
    let client = WatcherRegistryClient::new(&env, &contract_id);

    for op in &input.ops {
        // All documented error variants are acceptable outcomes; only panics
        // (host-level InvokeError) would indicate a contract bug.
        let _ = match op {
            Op::RegisterWatcher { admin_idx, watcher_idx } =>
                client.try_register_watcher(&addr(*admin_idx), &addr(*watcher_idx)),

            Op::RemoveWatcher { admin_idx, watcher_idx } =>
                client.try_remove_watcher(&addr(*admin_idx), &addr(*watcher_idx)),

            Op::ReplaceWatcher { admin_idx, old_idx, new_idx } =>
                client.try_replace_watcher(
                    &addr(*admin_idx),
                    &addr(*old_idx),
                    &addr(*new_idx),
                ),

            Op::AddAdmin { caller_idx, new_admin_idx } =>
                client.try_add_admin(&addr(*caller_idx), &addr(*new_admin_idx)),

            Op::RemoveAdmin { caller_idx, target_idx } =>
                client.try_remove_admin(&addr(*caller_idx), &addr(*target_idx)),

            Op::ClearAllWatchers { admin_idx } =>
                client.try_clear_all_watchers(&addr(*admin_idx)),

            Op::RegisterWatchers { admin_idx, watcher_mask } =>
                client.try_register_watchers(&addr(*admin_idx), &select(*watcher_mask)),

            Op::SetWatcherScope { admin_idx, watcher_idx, target_mask } =>
                client.try_set_watcher_scope(
                    &addr(*admin_idx),
                    &addr(*watcher_idx),
                    &select(*target_mask),
                ),

            Op::SetWatcherInfo { admin_idx, watcher_idx, label } =>
                client.try_set_watcher_info(
                    &addr(*admin_idx),
                    &addr(*watcher_idx),
                    &(*label).map(|n| WatcherInfo {
                        label: soroban_sdk::String::from_str(&env, &format!("label-{n}")),
                        operator: soroban_sdk::String::from_str(&env, "operator"),
                    }),
                ),

            Op::SetTimelockDelay { caller_idx, delay } =>
                client.try_set_timelock_delay(&addr(*caller_idx), &(*delay as u32)),

            Op::ProposeAction { caller_idx, action, arg_idx, delay } => {
                let action = match action {
                    ActionKind::AddAdmin => AdminAction::AddAdmin(addr(*arg_idx)),
                    ActionKind::TransferAdmin => AdminAction::TransferAdmin(addr(*arg_idx)),
                    ActionKind::ClearAllWatchers => AdminAction::ClearAllWatchers,
                    ActionKind::SetTimelockDelay => AdminAction::SetTimelockDelay(*delay as u32),
                };
                client
                    .try_propose_admin_action(&addr(*caller_idx), &action)
                    .map(|r| r.map(|_| ()))
            }

            Op::ExecuteAction { caller_idx } =>
                client.try_execute_admin_action(&addr(*caller_idx)),

            Op::CancelAction { caller_idx } =>
                client.try_cancel_admin_action(&addr(*caller_idx)),

            Op::AdvanceLedger { ledgers } => {
                // Keep instance storage (admins, watchers, timelock state) alive
                // across the jump. Advances are at most 255 ledgers, far below
                // the bump threshold, so the entry cannot expire mid-advance.
                client.bump_instance_ttl();
                env.ledger().with_mut(|li| li.sequence_number += *ledgers as u32);
                Ok(())
            }
            Op::ClearAllWatchers { admin_idx, keep_a_idx, keep_b_idx } => {
                // Build the kept set from the pool; duplicates collapse naturally
                // because the contract treats the kept set as a set.
                let mut keep: Vec<Address> = Vec::new();
                keep.push(addr(*keep_a_idx));
                keep.push(addr(*keep_b_idx));
                client.try_clear_all_watchers(&addr(*admin_idx), &keep)
            }

            Op::SetGuardian { admin_idx, guardian_idx } =>
                client.try_set_guardian(&addr(*admin_idx), &addr(*guardian_idx)),

            Op::Pause { caller_idx } =>
                client.try_pause(&addr(*caller_idx)),

            Op::Unpause { caller_idx } =>
                client.try_unpause(&addr(*caller_idx)),
        };

        // ── Invariant checks ─────────────────────────────────────────────────

        let watchers = client.get_watchers();
        let w_cnt    = client.get_watcher_count();

        // 1. W_CNT counter must equal the actual watcher list length.
        assert_eq!(
            w_cnt,
            watchers.len(),
            "W_CNT ({w_cnt}) != len(watchers) ({}): counter is out of sync",
            watchers.len(),
        );

        // get_admins returns Result; the contract is always initialized here
        // so Err(NotInitialized) is impossible — unwrap is safe.
        let admins = client.get_admins().expect("get_admins must not fail on initialized contract");

        // 2. Admin set must never be empty (LastAdmin guard).
        assert!(
            admins.len() >= 1,
            "admin set is empty — contract would be permanently locked"
        );

        // 3. No duplicate watchers.
        let watcher_vec: Vec<Address> = (0..watchers.len())
            .map(|i| watchers.get(i).unwrap())
            .collect();
        for i in 0..watcher_vec.len() {
            for j in (i + 1)..watcher_vec.len() {
                assert_ne!(
                    watcher_vec[i], watcher_vec[j],
                    "duplicate watcher found at indices {i} and {j}"
                );
            }
        }

        // 4. No duplicate admins.
        let admin_vec: Vec<Address> = (0..admins.len())
            .map(|i| admins.get(i).unwrap())
            .collect();
        for i in 0..admin_vec.len() {
            for j in (i + 1)..admin_vec.len() {
                assert_ne!(
                    admin_vec[i], admin_vec[j],
                    "duplicate admin found at indices {i} and {j}"
                );
            }
        }

        // 5. The timelock delay can always be lowered via proposal when nothing
        //    is queued. Probe with a delay of 0, then cancel the probe.
        if client.get_timelock_delay() > 0 && client.get_pending_action().is_none() {
            let admin = admins.get(0).unwrap();
            assert!(
                matches!(
                    client.try_propose_admin_action(&admin, &AdminAction::SetTimelockDelay(0)),
                    Ok(Ok(_))
                ),
                "lowering the timelock delay via proposal was rejected"
            );
            client.cancel_admin_action(&admin);
        }

        // 6. A pool address that is not a registered watcher is never authorized,
        //    so scope/info left behind after removal has no effect.
        for w in pool.iter().filter(|w| !watcher_vec.contains(*w)) {
            assert!(
                !client.is_watcher_authorized_for(w, &pool[0]),
                "unregistered address is still authorized"
        // 5. The guardian, if set, must be a member of the admin set.
        if let Some(guardian) = client.get_guardian() {
            assert!(
                admin_vec.contains(&guardian),
                "guardian is not a member of the admin set"
            );
        }
    }
});

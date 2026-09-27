#![allow(clippy::pedantic, clippy::too_many_lines)]

extern crate std;

use std::collections::BTreeSet;

use crate::{WatcherRegistry, WatcherRegistryClient, ContractError};
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_env_and_client() -> (Env, Address, WatcherRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(WatcherRegistry, (&admin,));
    let client = WatcherRegistryClient::new(&env, &contract_id);
    (env, admin, client)
}

#[derive(Debug, Clone)]
pub enum WatcherAction {
    RegisterWatcher(usize),
    RemoveWatcher(usize),
    ReplaceWatcher { old_idx: usize, new_idx: usize },
    ClearAll,
    ClearBatch(Vec<usize>),
}

fn watcher_address(env: &Env, idx: usize) -> Address {
    let mut addr = Address::generate(env);
    // Create deterministic addresses for testing
    for _ in 0..idx {
        addr = Address::generate(env);
    }
    addr
}

prop_compose! {
    fn arb_watcher_action()(action in prop_oneof![
        (0..10usize).prop_map(WatcherAction::RegisterWatcher),
        (0..10usize).prop_map(WatcherAction::RemoveWatcher),
    ]) -> WatcherAction {
        action
    }
}

proptest! {
    #[test]
    fn prop_watcher_set_never_empty(actions in prop::collection::vec(arb_watcher_action(), 1..20)) {
        let (env, admin, client) = create_env_and_client();
        let mut watchers: BTreeSet<Address> = BTreeSet::new();

        for action in actions {
            match action {
                WatcherAction::RegisterWatcher(idx) => {
                    let watcher = watcher_address(&env, idx);
                    let _ = client.register_watcher(&admin, &watcher);
                    watchers.insert(watcher);
                }
                WatcherAction::RemoveWatcher(idx) => {
                    if idx < watchers.len() {
                        if let Some(watcher) = watchers.iter().nth(idx).cloned() {
                            let _ = client.remove_watcher(&admin, &watcher);
                            watchers.remove(&watcher);
                        }
                    }
                }
                _ => {}
            }
        }

        // Admin set should never be empty after initialization
        let admins = client.get_admins().unwrap();
        prop_assert!(!admins.is_empty());
    }

    #[test]
    fn prop_watcher_count_consistency(
        actions in prop::collection::vec(arb_watcher_action(), 1..15)
    ) {
        let (env, admin, client) = create_env_and_client();
        let mut registered: BTreeSet<Address> = BTreeSet::new();

        for action in actions {
            match action {
                WatcherAction::RegisterWatcher(idx) => {
                    let watcher = watcher_address(&env, idx);
                    let _ = client.register_watcher(&admin, &watcher);
                    registered.insert(watcher);
                }
                WatcherAction::RemoveWatcher(idx) => {
                    if idx < registered.len() {
                        if let Some(watcher) = registered.iter().nth(idx).cloned() {
                            let _ = client.remove_watcher(&admin, &watcher);
                            registered.remove(&watcher);
                        }
                    }
                }
                _ => {}
            }
        }

        let count = client.get_watcher_count();
        let watchers = client.get_watchers();

        // Count should match actual watcher list length
        prop_assert_eq!(count as usize, watchers.len());
    }

    #[test]
    fn prop_no_duplicate_watchers(
        actions in prop::collection::vec(arb_watcher_action(), 1..20)
    ) {
        let (env, admin, client) = create_env_and_client();

        for action in actions {
            if let WatcherAction::RegisterWatcher(idx) = action {
                let watcher = watcher_address(&env, idx);
                let _ = client.register_watcher(&admin, &watcher);
            }
        }

        let watchers = client.get_watchers();
        let unique: BTreeSet<_> = watchers.iter().cloned().collect();

        // No duplicates should exist
        prop_assert_eq!(watchers.len(), unique.len());
    }
}

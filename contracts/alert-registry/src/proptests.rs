#![allow(
    clippy::pedantic,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::redundant_closure_for_method_calls,
    clippy::vec_init_then_push
)]

extern crate std;

use std::collections::BTreeMap;
use std::format;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use crate::{AlertRegistry, AlertRegistryClient, ContractError};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String as SorobanString, Vec as SorobanVec,
};

fn create_env_and_client() -> (Env, Address, AlertRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AlertRegistry, ());
    let client = AlertRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, admin, client)
}

fn make_hash64(env: &Env, ch: char) -> SorobanString {
    let buf = [ch as u8; 64];
    SorobanString::from_str(env, core::str::from_utf8(&buf).unwrap())
}

fn make_str(env: &Env, s: &str) -> SorobanString {
    SorobanString::from_str(env, s)
}

fn valid_rules(env: &Env, count: usize) -> SorobanVec<SorobanString> {
    let count = count.min(10);
    let mut v = SorobanVec::new(env);
    for i in 0..count {
        let rule_str = if i % 2 == 0 {
            "rule:transfer"
        } else {
            "rule:mint"
        };
        v.push_back(make_str(env, rule_str));
    }
    v
}

#[derive(Debug, Clone)]
pub enum AlertAction {
    Register {
        owner_idx: usize,
        target_idx: usize,
        label: String,
        hash_char: char,
        rules_count: usize,
    },
    UpdateAlert {
        alert_selector: usize,
        caller_idx: usize,
        rules_count: usize,
        active: bool,
    },
    UpdateWebhook {
        alert_selector: usize,
        caller_idx: usize,
        hash_char: char,
    },
    ProposeWebhook {
        alert_selector: usize,
        caller_idx: usize,
        hash_char: char,
    },
    ConfirmWebhook {
        alert_selector: usize,
        caller_idx: usize,
    },
    RenewTtl {
        alert_selector: usize,
        caller_idx: usize,
    },
    UpdateLabel {
        alert_selector: usize,
        caller_idx: usize,
        label: String,
    },
    UpdateTargetContract {
        alert_selector: usize,
        caller_idx: usize,
        target_idx: usize,
    },
    DeactivateAll {
        caller_idx: usize,
    },
    RemoveAlert {
        alert_selector: usize,
        caller_idx: usize,
    },
    RemoveAlertByAdmin {
        alert_selector: usize,
    },
    BumpAlert {
        alert_selector: usize,
        ttl: u32,
    },
}

fn action_strategy() -> impl Strategy<Value = AlertAction> {
    prop_oneof![
        30 => (
            0..3usize,
            0..3usize,
            "[a-zA-Z0-9 _-]{1,30}",
            any::<char>().prop_filter("alphanumeric char", char::is_ascii_alphanumeric),
            0..5usize
        ).prop_map(|(owner_idx, target_idx, label, hash_char, rules_count)| {
            AlertAction::Register {
                owner_idx,
                target_idx,
                label,
                hash_char,
                rules_count,
            }
        }),
        15 => (0..20usize, 0..4usize, 0..5usize, any::<bool>()).prop_map(
            |(alert_selector, caller_idx, rules_count, active)| {
                AlertAction::UpdateAlert {
                    alert_selector,
                    caller_idx,
                    rules_count,
                    active,
                }
            }
        ),
        10 => (
            0..20usize,
            0..4usize,
            any::<char>().prop_filter("alphanumeric char", char::is_ascii_alphanumeric)
        ).prop_map(|(alert_selector, caller_idx, hash_char)| {
            AlertAction::UpdateWebhook {
                alert_selector,
                caller_idx,
                hash_char,
            }
        }),
        15 => (
            0..20usize,
            0..4usize,
            any::<char>().prop_filter("alphanumeric char", char::is_ascii_alphanumeric)
        ).prop_map(|(alert_selector, caller_idx, hash_char)| {
            AlertAction::ProposeWebhook {
                alert_selector,
                caller_idx,
                hash_char,
            }
        }),
        15 => (0..20usize, 0..4usize).prop_map(|(alert_selector, caller_idx)| {
            AlertAction::ConfirmWebhook {
                alert_selector,
                caller_idx,
            }
        }),
        5 => (0..20usize, 0..4usize).prop_map(|(alert_selector, caller_idx)| {
            AlertAction::RenewTtl {
                alert_selector,
                caller_idx,
            }
        }),
        10 => (0..20usize, 0..4usize, "[a-zA-Z0-9 _-]{1,30}").prop_map(
            |(alert_selector, caller_idx, label)| {
                AlertAction::UpdateLabel {
                    alert_selector,
                    caller_idx,
                    label,
                }
            }
        ),
        10 => (0..20usize, 0..4usize, 0..3usize).prop_map(
            |(alert_selector, caller_idx, target_idx)| {
                AlertAction::UpdateTargetContract {
                    alert_selector,
                    caller_idx,
                    target_idx,
                }
            }
        ),
        8 => (0..4usize).prop_map(|caller_idx| AlertAction::DeactivateAll { caller_idx }),
        10 => (0..20usize, 0..4usize).prop_map(|(alert_selector, caller_idx)| {
            AlertAction::RemoveAlert {
                alert_selector,
                caller_idx,
            }
        }),
        5 => (0..20usize).prop_map(|alert_selector| AlertAction::RemoveAlertByAdmin { alert_selector }),
        10 => (0..20usize, 0..1_000_000u32).prop_map(|(alert_selector, ttl)| {
            AlertAction::BumpAlert {
                alert_selector,
                ttl,
            }
        }),
    ]
}

#[derive(Debug, Clone)]
struct ModelAlert {
    id: u64,
    owner_idx: usize,
    target_idx: usize,
    label: String,
    webhook_hash_char: char,
    pending_webhook_hash_char: Option<char>,
    rules_count: usize,
    active: bool,
    removed: bool,
    created_at: u64,
    updated_at: u64,
}

#[test]
fn test_proptest_state_machine_manual_smoke() {
    let initial_actions = vec![
        AlertAction::Register {
            owner_idx: 0,
            target_idx: 0,
            label: "Test".to_string(),
            hash_char: 'a',
            rules_count: 1,
        },
        AlertAction::ProposeWebhook {
            alert_selector: 0,
            caller_idx: 0,
            hash_char: 'b',
        },
        AlertAction::ConfirmWebhook {
            alert_selector: 0,
            caller_idx: 0,
        },
        AlertAction::RemoveAlert {
            alert_selector: 0,
            caller_idx: 0,
        },
    ];
    run_state_machine(initial_actions);
}

fn run_state_machine(actions: Vec<AlertAction>) {
    let (env, admin, client) = create_env_and_client();

    let owners = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env), // Extra address used for unauthorized caller testing
    ];

    let targets = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];

    let mut model_alerts: BTreeMap<u64, ModelAlert> = BTreeMap::new();
    let mut next_id: u64 = 0;
    let mut current_time: u64 = 1_000;
    env.ledger().set_timestamp(current_time);

    let select_id = |selector: usize, model: &BTreeMap<u64, ModelAlert>| -> Option<u64> {
        if model.is_empty() {
            None
        } else {
            let ids: Vec<u64> = model.keys().copied().collect();
            Some(ids[selector % ids.len()])
        }
    };

    for action in actions {
        current_time += 5;
        env.ledger().set_timestamp(current_time);

        match action {
            AlertAction::Register {
                owner_idx,
                target_idx,
                label,
                hash_char,
                rules_count,
            } => {
                let owner = &owners[owner_idx % 3];
                let target = &targets[target_idx % 3];
                let rules = valid_rules(&env, rules_count);
                let wh = make_hash64(&env, hash_char);
                let lbl = make_str(&env, &label);

                let id = client.register_alert(owner, target, &lbl, &wh, &rules);
                assert_eq!(
                    id, next_id,
                    "Assigned ID must equal next_id monotonic counter"
                );

                model_alerts.insert(
                    id,
                    ModelAlert {
                        id,
                        owner_idx: owner_idx % 3,
                        target_idx: target_idx % 3,
                        label,
                        webhook_hash_char: hash_char,
                        pending_webhook_hash_char: None,
                        rules_count: rules_count.min(10),
                        active: true,
                        removed: false,
                        created_at: current_time,
                        updated_at: current_time,
                    },
                );
                next_id += 1;
            }

            AlertAction::UpdateAlert {
                alert_selector,
                caller_idx,
                rules_count,
                active,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();
                    let rules = valid_rules(&env, rules_count);

                    let res = client.try_update_alert(caller, &id, &rules, &active);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.rules_count = rules_count.min(10);
                        alert.active = active;
                        alert.updated_at = current_time;
                    }
                }
            }

            AlertAction::UpdateWebhook {
                alert_selector,
                caller_idx,
                hash_char,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();
                    let wh = make_hash64(&env, hash_char);

                    let res = client.try_update_webhook(caller, &id, &wh);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.webhook_hash_char = hash_char;
                        alert.updated_at = current_time;
                    }
                }
            }

            AlertAction::ProposeWebhook {
                alert_selector,
                caller_idx,
                hash_char,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();
                    let wh = make_hash64(&env, hash_char);

                    let res = client.try_propose_webhook(caller, &id, &wh);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.pending_webhook_hash_char = Some(hash_char);
                        // Live hash and updated_at remain untouched upon propose
                    }
                }
            }

            AlertAction::ConfirmWebhook {
                alert_selector,
                caller_idx,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();

                    let res = client.try_confirm_webhook(caller, &id);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else if alert.pending_webhook_hash_char.is_none() {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::NoPendingWebhook);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.webhook_hash_char = alert.pending_webhook_hash_char.take().unwrap();
                        alert.updated_at = current_time;
                    }
                }
            }

            AlertAction::RenewTtl {
                alert_selector,
                caller_idx,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get(&id).unwrap();

                    let res = client.try_renew_alert_ttl(caller, &id);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        // renewed TTL leaves all data and updated_at unchanged
                    }
                }
            }

            AlertAction::UpdateLabel {
                alert_selector,
                caller_idx,
                label,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();
                    let lbl = make_str(&env, &label);

                    let res = client.try_update_label(caller, &id, &lbl);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.label = label;
                        alert.updated_at = current_time;
                    }
                }
            }

            AlertAction::UpdateTargetContract {
                alert_selector,
                caller_idx,
                target_idx,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();
                    let new_target = &targets[target_idx % 3];

                    let res = client.try_update_target_contract(caller, &id, new_target);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.target_idx = target_idx % 3;
                        alert.updated_at = current_time;
                    }
                }
            }

            AlertAction::DeactivateAll { caller_idx } => {
                let caller = &owners[caller_idx % owners.len()];
                let deactivated_count = client.deactivate_all_alerts(caller);

                let expected_deactivated = model_alerts
                    .values_mut()
                    .filter(|a| {
                        !a.removed && a.owner_idx == (caller_idx % owners.len()) && a.active
                    })
                    .map(|a| {
                        a.active = false;
                        a.updated_at = current_time;
                    })
                    .count() as u32;

                assert_eq!(deactivated_count, expected_deactivated);
            }

            AlertAction::RemoveAlert {
                alert_selector,
                caller_idx,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let caller = &owners[caller_idx % owners.len()];
                    let alert = model_alerts.get_mut(&id).unwrap();

                    let res = client.try_remove_alert(caller, &id);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else if caller_idx % owners.len() != alert.owner_idx {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::Unauthorized);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.removed = true;
                    }
                }
            }

            AlertAction::RemoveAlertByAdmin { alert_selector } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let alert = model_alerts.get_mut(&id).unwrap();

                    let res = client.try_remove_alert_by_admin(&admin, &id);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                        alert.removed = true;
                    }
                }
            }

            AlertAction::BumpAlert {
                alert_selector,
                ttl,
            } => {
                if let Some(id) = select_id(alert_selector, &model_alerts) {
                    let alert = model_alerts.get(&id).unwrap();
                    let res = client.try_bump_alert(&id, &ttl);
                    if alert.removed {
                        assert_eq!(res.unwrap_err().unwrap(), ContractError::AlertNotFound);
                    } else {
                        assert_eq!(res.unwrap(), Ok(()));
                    }
                }
            }
        }

        // ── Check Invariants After Every Action ───────────────────────────────

        // Invariant: Total registered alerts counter is monotonic and matches next_id
        assert_eq!(client.get_alert_count(), next_id);

        for (&id, alert) in &model_alerts {
            if alert.removed {
                // Invariant: A removed alert can never be read or retrieved
                assert!(
                    client.get_alert(&id).is_none(),
                    "Removed alert must not exist in get_alert"
                );
                assert!(
                    client.get_alert_active(&id).is_none(),
                    "Removed alert must not have active flag"
                );

                // Invariant: A removed alert can never be reactivated, updated, or manipulated
                let owner = &owners[alert.owner_idx];
                let dummy_rules = valid_rules(&env, 1);
                assert_eq!(
                    client
                        .try_update_alert(owner, &id, &dummy_rules, &true)
                        .unwrap_err()
                        .unwrap(),
                    ContractError::AlertNotFound,
                    "Removed alert cannot be reactivated"
                );
                assert_eq!(
                    client
                        .try_update_webhook(owner, &id, &make_hash64(&env, 'z'))
                        .unwrap_err()
                        .unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client
                        .try_propose_webhook(owner, &id, &make_hash64(&env, 'z'))
                        .unwrap_err()
                        .unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client.try_confirm_webhook(owner, &id).unwrap_err().unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client.try_renew_alert_ttl(owner, &id).unwrap_err().unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client
                        .try_update_label(owner, &id, &make_str(&env, "New"))
                        .unwrap_err()
                        .unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client
                        .try_update_target_contract(owner, &id, &targets[0])
                        .unwrap_err()
                        .unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client.try_remove_alert(owner, &id).unwrap_err().unwrap(),
                    ContractError::AlertNotFound
                );
                assert_eq!(
                    client.try_bump_alert(&id, &1000).unwrap_err().unwrap(),
                    ContractError::AlertNotFound
                );
            } else {
                // Invariant: Live alert matches expected state machine model
                let on_chain = client.get_alert(&id).expect("Live alert must exist");
                assert_eq!(on_chain.owner, owners[alert.owner_idx]);
                assert_eq!(on_chain.target_contract, targets[alert.target_idx]);
                assert_eq!(on_chain.label, make_str(&env, &alert.label));
                assert_eq!(
                    on_chain.webhook_hash,
                    make_hash64(&env, alert.webhook_hash_char)
                );
                assert_eq!(
                    on_chain.pending_webhook_hash,
                    alert
                        .pending_webhook_hash_char
                        .map(|c| make_hash64(&env, c))
                );
                assert_eq!(on_chain.active, alert.active);
                assert_eq!(client.get_alert_active(&id), Some(alert.active));
                assert_eq!(on_chain.created_at, alert.created_at);
                assert_eq!(on_chain.updated_at, alert.updated_at);
                assert!(on_chain.updated_at >= on_chain.created_at);
            }
        }

        // Invariant: Owner active count and index consistency
        for (i, owner) in owners.iter().take(3).enumerate() {
            let expected_active_count = model_alerts
                .values()
                .filter(|a| !a.removed && a.owner_idx == i)
                .count() as u32;
            assert_eq!(client.get_active_alert_count(owner), expected_active_count);

            let querier = Address::generate(&env);
            let on_chain_owner_alerts = client.get_alerts_by_owner(&querier, owner);
            let live_owner_alert_ids: Vec<u64> = model_alerts
                .values()
                .filter(|a| !a.removed && a.owner_idx == i)
                .map(|a| a.id)
                .collect();
            assert_eq!(
                on_chain_owner_alerts.len() as usize,
                live_owner_alert_ids.len()
            );
        }

        // Invariant: Contract index consistency
        for (i, target) in targets.iter().enumerate() {
            let querier = Address::generate(&env);
            let on_chain_contract_alerts = client.get_alerts_for_contract(&querier, target);
            let live_contract_alert_ids: Vec<u64> = model_alerts
                .values()
                .filter(|a| !a.removed && a.target_idx == i)
                .map(|a| a.id)
                .collect();
            assert_eq!(
                on_chain_contract_alerts.len() as usize,
                live_contract_alert_ids.len()
            );

            let on_chain_active_alerts = client.get_active_alerts_for_contract(target);
            let live_active_contract_alert_ids: Vec<u64> = model_alerts
                .values()
                .filter(|a| !a.removed && a.target_idx == i && a.active)
                .map(|a| a.id)
                .collect();
            assert_eq!(
                on_chain_active_alerts.len() as usize,
                live_active_contract_alert_ids.len()
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(25))]

    #[test]
    fn proptest_alert_state_machine_sequences(actions in prop::collection::vec(action_strategy(), 1..30)) {
        run_state_machine(actions);
    }

    #[test]
    fn proptest_webhook_rotation_lifecycle(
        initial_char in any::<char>().prop_filter("alphanumeric", char::is_ascii_alphanumeric),
        propose1_char in any::<char>().prop_filter("alphanumeric", char::is_ascii_alphanumeric),
        propose2_char in any::<char>().prop_filter("alphanumeric", char::is_ascii_alphanumeric),
    ) {
        let (env, _admin, client) = create_env_and_client();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &make_str(&env, "Alert"),
            &make_hash64(&env, initial_char),
            &valid_rules(&env, 1),
        );

        // Before propose: pending is None, confirm returns NoPendingWebhook
        let cfg = client.get_alert(&id).unwrap();
        assert_eq!(cfg.webhook_hash, make_hash64(&env, initial_char));
        assert!(cfg.pending_webhook_hash.is_none());
        assert_eq!(
            client.try_confirm_webhook(&owner, &id).unwrap_err().unwrap(),
            ContractError::NoPendingWebhook
        );

        // Propose 1: pending is Some(propose1), live hash is still initial
        client.propose_webhook(&owner, &id, &make_hash64(&env, propose1_char));
        let cfg1 = client.get_alert(&id).unwrap();
        assert_eq!(cfg1.webhook_hash, make_hash64(&env, initial_char));
        assert_eq!(cfg1.pending_webhook_hash, Some(make_hash64(&env, propose1_char)));

        // Propose 2: overwrites pending with propose2 without changing live hash
        client.propose_webhook(&owner, &id, &make_hash64(&env, propose2_char));
        let cfg2 = client.get_alert(&id).unwrap();
        assert_eq!(cfg2.webhook_hash, make_hash64(&env, initial_char));
        assert_eq!(cfg2.pending_webhook_hash, Some(make_hash64(&env, propose2_char)));

        // Confirm: promotes propose2 to live, clears pending
        client.confirm_webhook(&owner, &id);
        let cfg3 = client.get_alert(&id).unwrap();
        assert_eq!(cfg3.webhook_hash, make_hash64(&env, propose2_char));
        assert!(cfg3.pending_webhook_hash.is_none());

        // Subsequent confirm immediately fails with NoPendingWebhook
        assert_eq!(
            client.try_confirm_webhook(&owner, &id).unwrap_err().unwrap(),
            ContractError::NoPendingWebhook
        );
    }

    #[test]
    fn proptest_bump_alert_ttl_clamping(requested_ttl in any::<u32>()) {
        let (env, _admin, client) = create_env_and_client();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let id = client.register_alert(
            &owner,
            &target,
            &make_str(&env, "Alert"),
            &make_hash64(&env, '0'),
            &valid_rules(&env, 1),
        );

        // bump_alert is permissionless and succeeds for any requested TTL
        let res = client.try_bump_alert(&id, &requested_ttl);
        assert_eq!(res.unwrap(), Ok(()));

        // Alert remains intact
        let cfg = client.get_alert(&id).unwrap();
        assert_eq!(cfg.owner, owner);
    }

    #[test]
    fn proptest_pagination_bounds_and_ordering(
        alert_count in 1..25usize,
        offset in 0..30u32,
        limit in 1..15u32,
    ) {
        let (env, _admin, client) = create_env_and_client();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);
        let querier = Address::generate(&env);

        let mut ids = Vec::new();
        for i in 0..alert_count {
            let label = format!("Alert {i}");
            let id = client.register_alert(
                &owner,
                &target,
                &make_str(&env, &label),
                &make_hash64(&env, 'a'),
                &valid_rules(&env, 1),
            );
            ids.push(id);
        }

        let paginated = client.get_alerts_by_owner_paginated(&querier, &owner, &offset, &limit);
        let u_offset = offset as usize;
        let u_limit = limit as usize;

        if u_offset >= ids.len() {
            assert_eq!(paginated.len(), 0);
        } else {
            let expected_len = std::cmp::min(u_limit, ids.len() - u_offset);
            assert_eq!(paginated.len() as usize, expected_len);
            for (idx, alert) in paginated.iter().enumerate() {
                assert_eq!(alert.label, make_str(&env, &format!("Alert {}", u_offset + idx)));
            }
        }
    }

    #[test]
    fn proptest_modified_since_filtering(
        num_alerts in 2..15usize,
        since_offset in 0..50u64,
    ) {
        let (env, _admin, client) = create_env_and_client();
        let owner = Address::generate(&env);
        let target = Address::generate(&env);

        let mut timestamps = Vec::new();
        let base_time: u64 = 10_000;

        for i in 0..num_alerts {
            let t = base_time + (i as u64) * 10;
            env.ledger().set_timestamp(t);
            client.register_alert(
                &owner,
                &target,
                &make_str(&env, &format!("A{i}")),
                &make_hash64(&env, 'x'),
                &valid_rules(&env, 1),
            );
            timestamps.push(t);
        }

        let since = base_time + since_offset;
        let modified = client.get_alerts_modified_since(&since);

        let expected_count = timestamps.iter().filter(|&&t| t >= since).count();
        assert_eq!(modified.len() as usize, expected_count);

        for alert in modified.iter() {
            assert!(alert.updated_at >= since);
        }
    }
}

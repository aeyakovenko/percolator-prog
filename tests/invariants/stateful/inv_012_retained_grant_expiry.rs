//! INV-012: grant admission uses delivery-time Clock, not signing-time validity.
//! Retained expired renewals and distinct-transport retries must leave the prior
//! grant usable; an expiry-only repair must still admit real CPI exposure.

use super::*;

const LP: usize = 1;
const EXPIRY: u64 = 4;
const REPAIRED_EXPIRY: u64 = 7;

fn retained_grant(env: &V16Svm, grant: GrantOracle, expiry: u64, transport: u64) -> Transaction {
    let owner = &env.actors[LP].signer;
    let payer = &env.actors[4].signer;
    let mut accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new_readonly(env.market, false),
        AccountMeta::new(grant.scope[2], false),
    ];
    accounts.extend(
        grant.scope[4..]
            .iter()
            .map(|key| AccountMeta::new_readonly(*key, false)),
    );
    Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_price(transport),
            Instruction {
                program_id: env.program_id,
                accounts,
                data: ProgInstruction::SetMatcherConfig {
                    portfolio_id: grant.portfolio_id,
                    expected_sequence: grant.sequence,
                    enabled: 1,
                    trade_fee_cap_bps: 1,
                    expiry_slot: expiry,
                }
                .encode(),
            },
        ],
        Some(&payer.pubkey()),
        &[payer, owner],
        env.svm.latest_blockhash(),
    )
}

fn deliver_grant(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    tx: Transaction,
    expiry: u64,
    succeeds: bool,
) {
    let matcher_scope = history.replay()[LP].scope[4..].try_into().unwrap();
    let error = capability_step(
        env,
        history,
        succeeds,
        Some(AuthorizationEvent::Grant {
            actor: LP,
            cap: Some(1),
            expiry,
            matcher_scope,
        }),
        |env| env.land_retained(tx),
    );
    if let Some(error) = error {
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::InvalidInstruction as u32
            )),
            "expired grant must reject at application admission: {error}"
        );
    }
}

#[test]
fn v16_program_retained_grant_admission_binds_delivery_time_expiry() {
    let mut histories = 0;
    let mut expired_deliveries = 0;
    let mut preserved_fills = 0;
    for route in [CpiRoute::Single, CpiRoute::Batch] {
        for landing_slot in [EXPIRY - 1, EXPIRY, EXPIRY + 1] {
            for sign in [-1, 1] {
                let mut env = V16Svm::new(
                    [0xe1; 32],
                    MarketConfig {
                        initial_price: 100,
                        actor_deposits: [1_000_000; 5],
                        actor_token_balances: [2_000_000; 5],
                        ..MarketConfig::default()
                    },
                );
                let mut history = AuthorizationHistory::new(&env);
                history.assert_prefix(&env);
                assert!(env.current_slot() < EXPIRY);
                let original = history.replay();
                let size = sign * POS_SCALE as i128;
                let old_consumer = retained_capability_trade(&mut env, route, size);
                let retained = retained_grant(&env, original[LP], EXPIRY, 1);
                let retry = retained_grant(&env, original[LP], EXPIRY, 2);
                let repaired = retained_grant(&env, original[LP], REPAIRED_EXPIRY, 3);
                assert_eq!(
                    retained.message.instructions[1], retry.message.instructions[1],
                    "identical wrapper consent on independent unsent transports"
                );
                assert_ne!(retained.signatures, retry.signatures);
                let before = capability_frame(&env, &history);
                let simulation = env
                    .svm
                    .simulate_transaction(retained.clone().into())
                    .expect("the exact signed grant is executable before its expiry");
                assert!(simulation.compute_units_consumed <= TX_CU_LIMIT);
                assert!(capability_frame(&env, &history) == before);

                env.warp_to_slot(landing_slot);
                assert_eq!(env.current_slot(), landing_slot);
                let admitted = landing_slot < EXPIRY;
                deliver_grant(&mut env, &mut history, retained, EXPIRY, admitted);
                if !admitted {
                    deliver_grant(&mut env, &mut history, retry, EXPIRY, false);
                    expired_deliveries += 2;
                    assert_eq!(history.replay(), original, "no consumed owner sequence");
                    // Failed renewals cannot poison the independently retained live grant.
                    land_capability_trade(&mut env, &mut history, old_consumer, &original, size);
                    preserved_fills += 1;
                    deliver_grant(&mut env, &mut history, repaired, REPAIRED_EXPIRY, true);
                }
                let current = history.replay();
                assert_eq!(current[LP].sequence, original[LP].sequence + 1);
                assert_eq!(current[LP].cap, 1);
                assert_eq!(
                    current[LP].expiry,
                    if admitted { EXPIRY } else { REPAIRED_EXPIRY }
                );
                let fresh = retained_capability_trade(&mut env, route, size);
                land_capability_trade(&mut env, &mut history, fresh, &current, size);
                assert_eq!(
                    history.replay()[LP].positions[0],
                    -size * if admitted { 1 } else { 2 },
                    "accepted grant must support a real additional fill"
                );
                histories += 1;
            }
        }
    }
    assert_eq!(
        (histories, expired_deliveries, preserved_fills),
        (12, 16, 8)
    );
    println!("INV-012 retained grant expiry: 12 histories, 12 live simulations, 48 public transactions, 16 expired grant deliveries, 8 preserved-grant fills, 12 renewed-grant fills");
}

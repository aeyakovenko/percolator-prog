//! INV-067/068/082: paid receipt history survives replacement of its SPL destination.
//! Keeper-only repair, expiry and catch-up preserve owner identity and exact rent.

use super::*;
use crate::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;

const CLAIMANTS: [usize; 2] = [0, 4];
const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const RESIDUAL: u128 = 851;
const SUPPLY: u128 = 3_852;

fn keeper_step(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rent: u64,
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    super::keeper_step_with_limit(env, ixs, tracked, allowed, rent, rejection, 600_000)
}

struct ReceiptOracle {
    owners: [Pubkey; 5],
    portfolios: [Pubkey; 5],
    destinations: [Pubkey; 5],
    saved: [Pubkey; 2],
    receipts: [ResolvedPayoutReceiptV16; 2],
    identities: [(u64, u64, (percolator::ProvenanceHeaderV16, [u8; 32])); 2],
    provider: Pubkey,
}

impl ReceiptOracle {
    #[track_caller]
    fn check(&self, env: &V16CuEnv, paid: [u128; 5], normalized: bool) {
        let group = env.market_state().1;
        let residual = if normalized { RESIDUAL } else { 501 };
        let ledger = group.resolved_payout_ledger;
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, residual);
        assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
        assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
        assert!(!ledger.payout_halted);
        assert_eq!(group.materialized_portfolio_count, 5);
        assert_eq!(
            group.source_backing_buckets[3].status,
            if normalized {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(
            group.source_credit[3].fresh_reserved_backing_num,
            if normalized { 0 } else { 350 * BOUND_SCALE }
        );
        assert_eq!(env.token_amount(self.provider), 1);
        let mut capital = 0;
        for actor in 0..5 {
            let account = env.portfolio_state(self.portfolios[actor]);
            assert_eq!(account.owner, self.owners[actor].to_bytes());
            capital += account.capital.get();
            let receipt = resolved_receipt(&account);
            let saved = CLAIMANTS.iter().position(|index| *index == actor);
            let bank = saved.map_or(0, |index| env.token_amount(self.saved[index]) as u128);
            let tokens = env
                .svm
                .get_account(&self.destinations[actor])
                .map_or(0, |a| {
                    if a.owner == spl_token::ID {
                        let token = TokenAccount::unpack(&a.data).unwrap();
                        assert_eq!(token.owner, self.owners[actor]);
                        assert_eq!(token.mint, env.mint);
                        assert_eq!(token.delegate, COption::None);
                        assert_eq!(token.close_authority, COption::None);
                        u128::from(token.amount)
                    } else {
                        assert!(a.data.is_empty());
                        0
                    }
                });
            assert_eq!(
                tokens + bank,
                paid[actor],
                "owner {actor} cumulative payout"
            );
            assert!(paid[actor] <= CAPITAL[actor] + FACES[actor] * residual / 3_000);
            if let Some(index) = saved {
                let old = self.receipts[index];
                assert_eq!(bank, CAPITAL[actor] + FACES[actor] * 501 / 3_000);
                if receipt.present {
                    assert_eq!(receipt.paid_effective, paid[actor] - CAPITAL[actor]);
                    let mut identity = receipt;
                    identity.paid_effective = old.paid_effective;
                    identity.finalized = old.finalized;
                    assert_eq!(
                        identity, old,
                        "receipt identity survives destination recreation"
                    );
                } else {
                    // Terminal cleanup may consume the receipt only after its whole entitlement.
                    assert!(resolved_portfolio_is_terminal(env, self.portfolios[actor]));
                    assert_eq!(
                        paid[actor],
                        CAPITAL[actor] + FACES[actor] * RESIDUAL / 3_000
                    );
                }
                assert_eq!(
                    (
                        env.portfolio_id(self.portfolios[actor]),
                        env.portfolio_position_epoch(self.portfolios[actor]),
                        state::read_portfolio_owner_preflight(
                            &env.svm.get_account(&self.portfolios[actor]).unwrap().data
                        )
                        .unwrap(),
                    ),
                    self.identities[index]
                );
            }
        }
        assert_eq!(group.c_tot, capital);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault, SUPPLY - 1 - paid.iter().sum::<u128>());
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply as u128, SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
        assert!(self.owners.iter().all(|key| account_is_closed(env, *key)));
    }
}

#[test]
fn v16_program_recreated_destinations_preserve_paid_receipts_across_expiry_without_owners() {
    let mut peak = 0;
    for slot in [13, 17] {
        for reverse in [false, true] {
            for split in [false, true] {
                for prefund in [false, true] {
                    let mut world = World::before_receipts();
                    for actor in CLAIMANTS {
                        for _ in 0..8 {
                            if world.receipt(actor).present {
                                break;
                            }
                            world.land(&[world.payout(actor, false)], false).unwrap();
                        }
                        assert_eq!(
                            world.receipt(actor).paid_effective,
                            FACES[actor] * 501 / 3_000
                        );
                    }
                    let payouts: [Instruction; 5] = std::array::from_fn(|i| world.payout(i, false));
                    let claims = CLAIMANTS.map(|i| world.payout(i, true));
                    let identities = CLAIMANTS.map(|i| {
                        let key = world.actors[i].portfolio;
                        (
                            world.env.portfolio_id(key),
                            world.env.portfolio_position_epoch(key),
                            state::read_portfolio_owner_preflight(
                                &world.env.svm.get_account(&key).unwrap().data,
                            )
                            .unwrap(),
                        )
                    });
                    let receipts = CLAIMANTS.map(|i| world.receipt(i));
                    let mut saved = [Pubkey::default(); 2];
                    for (index, actor) in CLAIMANTS.into_iter().enumerate() {
                        let a = &world.actors[actor];
                        let bank = Keypair::new();
                        system_create_account_for_test(
                            &mut world.env.svm,
                            &world.env.payer,
                            &bank,
                            TokenAccount::LEN,
                            spl_token::ID,
                        );
                        send_raw_tx(
                            &mut world.env.svm,
                            &world.env.payer,
                            spl_token::instruction::initialize_account3(
                                &spl_token::ID,
                                &bank.pubkey(),
                                &world.env.mint,
                                &a.owner.pubkey(),
                            )
                            .unwrap(),
                            &[],
                        )
                        .unwrap();
                        let amount = world.env.token_amount(a.token);
                        send_raw_tx(
                            &mut world.env.svm,
                            &world.env.payer,
                            spl_token::instruction::transfer(
                                &spl_token::ID,
                                &a.token,
                                &bank.pubkey(),
                                &a.owner.pubkey(),
                                &[],
                                amount,
                            )
                            .unwrap(),
                            &[&a.owner],
                        )
                        .unwrap();
                        let rent = world.env.svm.get_account(&a.token).unwrap().lamports;
                        let mut owner = world.env.svm.get_account(&a.owner.pubkey()).unwrap();
                        send_raw_tx(
                            &mut world.env.svm,
                            &world.env.payer,
                            spl_token::instruction::close_account(
                                &spl_token::ID,
                                &a.token,
                                &a.owner.pubkey(),
                                &a.owner.pubkey(),
                                &[],
                            )
                            .unwrap(),
                            &[&a.owner],
                        )
                        .unwrap();
                        owner.lamports += rent;
                        assert_eq!(world.env.svm.get_account(&a.owner.pubkey()), Some(owner));
                        assert!(account_is_closed(&world.env, a.token));
                        saved[index] = bank.pubkey();
                    }
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &world.env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &world.env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&world.env.admin],
                    )
                    .unwrap();
                    for actor in &world.actors {
                        let lamports = world
                            .env
                            .svm
                            .get_account(&actor.owner.pubkey())
                            .unwrap()
                            .lamports;
                        send_raw_tx(
                            &mut world.env.svm,
                            &world.env.payer,
                            system_instruction::transfer(
                                &actor.owner.pubkey(),
                                &world.env.payer.pubkey(),
                                lamports,
                            ),
                            &[&actor.owner],
                        )
                        .unwrap();
                    }
                    let oracle = ReceiptOracle {
                        owners: std::array::from_fn(|i| world.actors[i].owner.pubkey()),
                        portfolios: std::array::from_fn(|i| world.actors[i].portfolio),
                        destinations: std::array::from_fn(|i| world.actors[i].token),
                        saved,
                        receipts,
                        identities,
                        provider: world.provider_token,
                    };
                    let mut env = world.env;
                    drop(world.actors);
                    let market = env.market;
                    let vault = env.vault;
                    let rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                    let seed = if prefund { rent + 1 } else { 0 };
                    for actor in CLAIMANTS {
                        if seed != 0 {
                            send_raw_tx(
                                &mut env.svm,
                                &env.payer,
                                system_instruction::transfer(
                                    &env.admin.pubkey(),
                                    &oracle.destinations[actor],
                                    seed,
                                ),
                                &[&env.admin],
                            )
                            .unwrap();
                        }
                    }
                    let mut tracked = vec![
                        market,
                        vault,
                        env.mint,
                        env.vault_authority,
                        env.admin.pubkey(),
                        oracle.provider,
                    ];
                    tracked.extend(oracle.owners);
                    tracked.extend(oracle.portfolios);
                    tracked.extend(oracle.destinations);
                    tracked.extend(saved);
                    let stable_rent: Vec<_> = tracked
                        .iter()
                        .filter(|key| !oracle.destinations.contains(key))
                        .map(|key| (*key, env.svm.get_account(key).map(|a| a.lamports)))
                        .collect();
                    let creates = CLAIMANTS.map(|actor| Instruction {
                        program_id: associated_token_program_id(),
                        accounts: vec![
                            AccountMeta::new(env.payer.pubkey(), true),
                            AccountMeta::new(oracle.destinations[actor], false),
                            AccountMeta::new_readonly(oracle.owners[actor], false),
                            AccountMeta::new_readonly(env.mint, false),
                            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: vec![1],
                    });
                    let mut paid = [
                        1_000 + 700 * 501 / 3_000,
                        0,
                        0,
                        0,
                        1_000 + 1_300 * 501 / 3_000,
                    ];
                    oracle.check(&env, paid, false);
                    env.svm.warp_to_slot(slot);
                    let order = if reverse { [1, 0] } else { [0, 1] };
                    let first = order[0];
                    let actor = CLAIMANTS[first];
                    let unsigned_close = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(oracle.owners[actor], false),
                            AccountMeta::new(market, false),
                            AccountMeta::new(oracle.portfolios[actor], false),
                        ],
                        data: env.close_portfolio_ix(oracle.portfolios[actor]).encode(),
                    };
                    peak = peak.max(keeper_step(
                        &mut env,
                        &[
                            payouts[2].clone(),
                            creates[first].clone(),
                            claims[first].clone(),
                            unsigned_close,
                        ],
                        &tracked,
                        &[],
                        0,
                        Some((5, PercolatorError::ExpectedSigner)),
                    ));
                    oracle.check(&env, paid, false);

                    peak = peak.max(keeper_step(
                        &mut env,
                        &payouts[2..3],
                        &tracked,
                        &[market, oracle.portfolios[2]],
                        0,
                        None,
                    ));
                    oracle.check(&env, paid, true);
                    for indices in order.chunks(if split { 1 } else { 2 }) {
                        let mut instructions = Vec::new();
                        let mut allowed = vec![market, vault];
                        for &i in indices {
                            instructions.extend([creates[i].clone(), claims[i].clone()]);
                            allowed.extend([
                                oracle.portfolios[CLAIMANTS[i]],
                                oracle.destinations[CLAIMANTS[i]],
                            ]);
                            paid[CLAIMANTS[i]] =
                                CAPITAL[CLAIMANTS[i]] + FACES[CLAIMANTS[i]] * RESIDUAL / 3_000;
                        }
                        peak = peak.max(keeper_step(
                            &mut env,
                            &instructions,
                            &tracked,
                            &allowed,
                            rent.saturating_sub(seed) * indices.len() as u64,
                            None,
                        ));
                        oracle.check(&env, paid, true);
                    }
                    for index in order {
                        peak = peak.max(keeper_step(
                            &mut env,
                            &[creates[index].clone(), claims[index].clone()],
                            &tracked,
                            &[],
                            0,
                            None,
                        ));
                        assert_eq!(
                            env.svm
                                .get_account(&oracle.destinations[CLAIMANTS[index]])
                                .unwrap()
                                .lamports,
                            rent.max(seed)
                        );
                    }
                    for _ in 0..16 {
                        for actor in [2, CLAIMANTS[order[0]], CLAIMANTS[order[1]], 1, 3] {
                            if resolved_portfolio_is_terminal(&env, oracle.portfolios[actor]) {
                                continue;
                            }
                            let mut crank = payouts[actor].clone();
                            crank.data = ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: vec![],
                            }
                            .encode();
                            peak = peak.max(keeper_step(
                                &mut env,
                                &[crank],
                                &tracked,
                                &[
                                    market,
                                    vault,
                                    oracle.portfolios[actor],
                                    oracle.destinations[actor],
                                ],
                                0,
                                None,
                            ));
                            if actor == 2
                                && (resolved_receipt(
                                    &env.portfolio_state(oracle.portfolios[actor]),
                                )
                                .present
                                    || resolved_portfolio_is_terminal(
                                        &env,
                                        oracle.portfolios[actor],
                                    ))
                            {
                                paid[actor] = CAPITAL[actor] + FACES[actor] * RESIDUAL / 3_000;
                            }
                            oracle.check(&env, paid, true);
                        }
                        if oracle
                            .portfolios
                            .iter()
                            .all(|key| resolved_portfolio_is_terminal(&env, *key))
                        {
                            break;
                        }
                    }
                    assert!(oracle
                        .portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&env, *key)));
                    assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368]);
                    assert_eq!(env.market_state().1.vault, 2);
                    assert_eq!(env.market_state().1.source_claim_bound_total_num, 0);
                    for (key, lamports) in stable_rent {
                        assert_eq!(
                            env.svm.get_account(&key).map(|a| a.lamports),
                            lamports,
                            "retained rent {key}"
                        );
                    }
                }
            }
        }
    }
    eprintln!("receipt destination recovery: 16 worlds, 32 repaired ATAs, 16 expiry/repair/payout rollbacks; peak CU={peak}");
}

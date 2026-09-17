//! INV-067: a Recovery forfeit drops only the unrefreshed gain, then the older
//! booked claim survives a partial receipt and backing expiry on the other asset.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const DEPOSITS: [u128; 4] = [1_000, 500, 1_000, 100];
const FACES: [u128; 4] = [14 * (150 - 100), 0, 26 * (150 - 100), 0];
const FORFEITED: u128 = 10 * (105 - 100);
const DENOMINATOR: u128 = FACES[0] + FACES[2];
const BACKING: u128 = 350;
// The Recovery debtor's loss replenishes the opposite source reserve. Its
// forfeited counterclaim cannot consume that stock before the late expiry.
const INITIAL_RESIDUAL: u128 = DEPOSITS[1] + 1;
const LATE_RELEASE: u128 = BACKING + FORFEITED;
const FINAL_RESIDUAL: u128 = INITIAL_RESIDUAL + LATE_RELEASE;
const SUPPLY: u128 = 2_600 + 1 + BACKING;
const CU_LIMIT: u64 = 600_000;

fn entitlement(actor: usize, residual: u128) -> u128 {
    FACES[actor] * residual / DENOMINATOR
}

struct World {
    env: V16CuEnv,
    actors: [late_expiry::Actor; 4],
    provider: Pubkey,
    peak: u64,
}

impl World {
    fn new() -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                initial_price: 100,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
        );
        let actors = std::array::from_fn(|actor| {
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[&owner],
            )
            .unwrap();
            env.portfolios.push(portfolio.pubkey());
            let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            Self::mint(&mut env, token, DEPOSITS[actor]);
            env.send(
                env.deposit_ix(portfolio.pubkey(), DEPOSITS[actor]),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                    AccountMeta::new(token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .unwrap();
            late_expiry::Actor {
                owner,
                portfolio: portfolio.pubkey(),
                token,
            }
        });
        let provider = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        Self::mint(&mut env, provider, 1 + BACKING);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        env.svm.warp_to_slot(1);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        }
        env.configure_permissionless_resolve_with_cu(100, 1);
        // One atom keeps the original PnL source-backed until the snapshot.
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 1, 1, 15);
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 3, BACKING, 40);
        Self {
            env,
            actors,
            provider,
            peak: 0,
        }
    }

    fn mint(env: &mut V16CuEnv, token: Pubkey, amount: u128) {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &env.admin.pubkey(),
                &[],
                amount as u64,
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
    }

    fn receipt(&self, actor: usize) -> ResolvedPayoutReceiptV16 {
        resolved_receipt(&self.env.portfolio_state(self.actors[actor].portfolio))
    }

    fn payout(&self, actor: usize, claim: bool) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(self.actors[actor].owner.pubkey(), false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.actors[actor].portfolio, false),
                AccountMeta::new(self.actors[actor].token, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: if claim {
                ProgInstruction::ClaimResolvedPayoutTopup
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            }
            .encode(),
        }
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        [
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.provider,
        ]
        .into_iter()
        .chain(
            self.actors
                .iter()
                .flat_map(|a| [a.owner.pubkey(), a.portfolio, a.token]),
        )
        .map(|key| (key, self.env.svm.get_account(&key)))
        .collect()
    }

    fn land(
        &mut self,
        instructions: &[Instruction],
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        self.env.svm.expire_blockhash();
        let mut all = vec![heap_ix(), cu_ix()];
        all.extend_from_slice(instructions);
        let tx = Transaction::new_signed_with_payer(
            &all,
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer],
            self.env.svm.latest_blockhash(),
        );
        let before: Vec<_> = tx
            .message
            .account_keys
            .iter()
            .map(|&key| (key, self.env.svm.get_account(&key)))
            .collect();
        let result = self.env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(error) => &error.meta,
        };
        self.peak = self.peak.max(meta.compute_units_consumed);
        assert_cu_within(
            "Recovery receipt suffix",
            meta.compute_units_consumed,
            CU_LIMIT,
        );
        if result.is_err() {
            for (key, mut account) in before {
                if key == self.env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -=
                        FeeStructure::default().lamports_per_signature;
                }
                assert_eq!(self.env.svm.get_account(&key), account, "rollback {key}");
            }
        }
        result
    }

    fn custody(&self) {
        let vault = u128::from(self.env.token_amount(self.env.vault));
        assert_eq!(self.env.market_state().1.vault, vault);
        assert_eq!(
            vault
                + self
                    .actors
                    .iter()
                    .map(|a| u128::from(self.env.token_amount(a.token)))
                    .sum::<u128>(),
            SUPPLY
        );
        assert_eq!(self.env.token_amount(self.provider), 0);
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
    }
}

#[test]
fn v16_program_recovery_forfeit_preserves_older_partial_receipts_through_late_expiry() {
    let mut peak = 0;
    for forfeits in [[0, 3], [3, 0]] {
        for order in [[0, 2], [2, 0]] {
            for landing in [40, 47] {
                let mut world = World::new();
                for (actor, quantity) in [(0, 14), (2, 26)] {
                    world.env.trade_asset_with_cu(
                        0,
                        &world.actors[actor].owner,
                        world.actors[actor].portfolio,
                        &world.actors[1].owner,
                        world.actors[1].portfolio,
                        quantity * POS_SCALE as i128,
                        100,
                        0,
                    );
                }
                for (offset, mark) in (105..=150).step_by(5).enumerate() {
                    let slot = 2 + offset as u64;
                    world.env.svm.warp_to_slot(slot);
                    world.env.push_auth_mark_for_asset_as_admin(0, slot, mark);
                    for actor in [1, 0, 2] {
                        world.peak = world.peak.max(world.env.crank(
                            world.actors[actor].portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations(0),
                            },
                        ));
                    }
                }
                for (actor, quantity) in [(0, 14), (2, 26)] {
                    world.env.trade_asset_with_cu(
                        0,
                        &world.actors[actor].owner,
                        world.actors[actor].portfolio,
                        &world.actors[1].owner,
                        world.actors[1].portfolio,
                        -quantity * POS_SCALE as i128,
                        150,
                        0,
                    );
                }
                // A separate Recovery leg has a real gain to forfeit, while the
                // first asset's closed claims remain underfunded and source-backed.
                world.env.trade_asset_with_cu(
                    1,
                    &world.actors[0].owner,
                    world.actors[0].portfolio,
                    &world.actors[3].owner,
                    world.actors[3].portfolio,
                    10 * POS_SCALE as i128,
                    100,
                    0,
                );
                world.env.svm.warp_to_slot(12);
                world.env.push_auth_mark_for_asset_as_admin(1, 12, 105);
                // Asset 1 has been idle since slot 1; the fixture accrues one slot per crank.
                for _ in 0..16 {
                    let debtor = world.env.portfolio_state(world.actors[3].portfolio);
                    if debtor.capital.get() == DEPOSITS[3] - FORFEITED {
                        break;
                    }
                    world.peak = world.peak.max(world.env.crank(
                        world.actors[3].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 12,
                            observations: crank_observations(1),
                        },
                    ));
                }
                let booked = world.env.portfolio_state(world.actors[0].portfolio);
                assert_eq!(booked.pnl.get(), FACES[0] as i128);
                assert_ne!(
                    active_leg_for_asset(&booked, 1).k_snap,
                    world.env.market_state().1.assets[1].k_long
                );
                let debtor = world.env.portfolio_state(world.actors[3].portfolio);
                assert_eq!(debtor.capital.get(), DEPOSITS[3] - FORFEITED);
                assert_eq!(debtor.pnl.get(), 0);
                world.env.update_asset_lifecycle_as_admin_with_cu(
                    processor::ASSET_ACTION_SHUTDOWN,
                    1,
                    12,
                    0,
                );
                assert_eq!(
                    world.env.market_state().1.assets[1].lifecycle,
                    AssetLifecycleV16::Recovery
                );
                for actor in forfeits {
                    world.peak = world.peak.max(world.env.forfeit_recovery_leg_with_cu(
                        &world.actors[actor].owner,
                        world.actors[actor].portfolio,
                        1,
                        u128::MAX,
                    ));
                    for claimant in [0, 2] {
                        assert_eq!(world.env.portfolio_state(world.actors[claimant].portfolio).pnl.get(), FACES[claimant] as i128,
                            "forfeit order cannot consume a booked claim or book the discarded gain");
                    }
                    world.custody();
                }
                for actor in [0, 3] {
                    for _ in 0..4 {
                        if !has_active_leg_for_asset(
                            &world.env.portfolio_state(world.actors[actor].portfolio),
                            1,
                        ) {
                            break;
                        }
                        world.peak = world.peak.max(world.env.crank(
                            world.actors[actor].portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 12,
                                observations: vec![],
                            },
                        ));
                    }
                    assert!(!has_active_leg_for_asset(
                        &world.env.portfolio_state(world.actors[actor].portfolio),
                        1
                    ));
                }
                world.env.svm.warp_to_slot(13);
                world.env.resolve();
                world.env.svm.warp_to_slot(15);
                for actor in [1, 3] {
                    for _ in 0..8 {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            break;
                        }
                        world.land(&[world.payout(actor, false)]).unwrap();
                    }
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                }
                for actor in order {
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        world.land(&[world.payout(actor, false)]).unwrap();
                    }
                    assert_eq!(
                        world.receipt(actor),
                        ResolvedPayoutReceiptV16 {
                            present: true,
                            prior_bound_contribution_num: FACES[actor] * BOUND_SCALE,
                            live_released_face_at_receipt: 0,
                            terminal_positive_claim_face: FACES[actor],
                            paid_effective: entitlement(actor, INITIAL_RESIDUAL),
                            finalized: false,
                        }
                    );
                }
                world.custody();
                let ledger = world.env.market_state().1.resolved_payout_ledger;
                assert_eq!(
                    world.env.market_state().1.source_credit[3].fresh_reserved_backing_num,
                    LATE_RELEASE * BOUND_SCALE
                );
                assert_eq!(ledger.snapshot_slot, 15);
                assert_eq!(ledger.snapshot_residual, INITIAL_RESIDUAL);
                assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                assert_eq!(
                    world.env.market_state().1.source_backing_buckets[3].status,
                    BackingBucketStatusV16::Fresh
                );
                assert_eq!(
                    world.env.market_state().1.source_backing_buckets[3].expiry_slot,
                    40
                );
                assert_eq!(
                    ledger.terminal_claim_exact_receipts_num,
                    DENOMINATOR * BOUND_SCALE
                );
                let original = order.map(|a| world.receipt(a));
                let identities = order.map(|a| {
                    (
                        world.env.portfolio_id(world.actors[a].portfolio),
                        world
                            .env
                            .portfolio_position_epoch(world.actors[a].portfolio),
                    )
                });
                let claims = order.map(|a| world.payout(a, true));
                let before = world.frame();
                world.land(&claims).unwrap();
                assert_eq!(world.frame(), before, "partial receipt zero-due replay");

                world.env.svm.warp_to_slot(landing);
                let mut release = world.payout(order[0], false);
                release.data = ProgInstruction::PermissionlessCrank {
                    now_slot: landing,
                    observations: vec![CrankObservationHint {
                        asset_index: 1,
                        oracle_accounts: 0,
                    }],
                }
                .encode();
                let bundle = [release, claims[1].clone(), claims[0].clone()];
                let before = world.frame();
                let mut aborted = bundle.to_vec();
                aborted.push(Instruction {
                    program_id: solana_sdk::system_program::ID,
                    accounts: vec![],
                    data: vec![],
                });
                let failure = world
                    .land(&aborted)
                    .expect_err("abort expiry and both positive top-ups");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(5, InstructionError::InvalidInstructionData)
                );
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|s| **s == format!("Program {} success", spl_token::ID))
                        .count(),
                    2
                );
                assert_eq!(world.frame(), before);
                world.land(&bundle[..1]).unwrap();
                assert_eq!(
                    order.map(|a| world.receipt(a)),
                    original,
                    "expiry preserves the forfeit-derived receipts"
                );
                let after = world.env.market_state().1.resolved_payout_ledger;
                assert_eq!(
                    world.env.market_state().1.source_backing_buckets[3].status,
                    BackingBucketStatusV16::Expired
                );
                assert_eq!(
                    world.env.market_state().1.source_credit[3].fresh_reserved_backing_num,
                    0
                );
                assert_eq!(after.snapshot_slot, ledger.snapshot_slot);
                assert_eq!(after.snapshot_residual, FINAL_RESIDUAL);
                assert_eq!(after.terminal_claim_bound_unreceipted_num, 0);
                assert_eq!(
                    after.terminal_claim_exact_receipts_num,
                    DENOMINATOR * BOUND_SCALE
                );
                assert_eq!(after.current_payout_rate_num, FINAL_RESIDUAL * BOUND_SCALE);
                assert_eq!(after.current_payout_rate_den, DENOMINATOR * BOUND_SCALE);
                for index in [1, 0] {
                    let actor = order[index];
                    world.land(&[claims[index].clone()]).unwrap();
                    assert_eq!(
                        world.receipt(actor),
                        ResolvedPayoutReceiptV16 {
                            paid_effective: entitlement(actor, FINAL_RESIDUAL),
                            ..original[index]
                        }
                    );
                    assert_eq!(
                        (
                            world.env.portfolio_id(world.actors[actor].portfolio),
                            world
                                .env
                                .portfolio_position_epoch(world.actors[actor].portfolio)
                        ),
                        identities[index]
                    );
                    assert_eq!(
                        u128::from(world.env.token_amount(world.actors[actor].token)),
                        DEPOSITS[actor] + entitlement(actor, FINAL_RESIDUAL)
                    );
                    assert_eq!(
                        world
                            .env
                            .portfolio_state(world.actors[actor].portfolio)
                            .owner,
                        world.actors[actor].owner.pubkey().to_bytes()
                    );
                    world.custody();
                }
                for actor in order.into_iter().chain([1, 3]) {
                    for _ in 0..16 {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            break;
                        }
                        world.land(&[world.payout(actor, false)]).unwrap();
                    }
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                    let before = world.frame();
                    world.land(&[world.payout(actor, true)]).unwrap();
                    assert_eq!(
                        world.frame(),
                        before,
                        "terminal replay cannot restore forfeited or paid value"
                    );
                    let portfolio = world.actors[actor].portfolio;
                    let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    world.peak = world.peak.max(
                        world
                            .env
                            .close_portfolio_with_cu(&world.actors[actor].owner, portfolio),
                    );
                    assert!(world
                        .env
                        .svm
                        .get_account(&portfolio)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + rent
                    );
                    world.custody();
                }
                let rounding = FINAL_RESIDUAL
                    - entitlement(0, FINAL_RESIDUAL)
                    - entitlement(2, FINAL_RESIDUAL);
                assert_eq!(rounding, 1);
                assert_eq!(world.env.market_state().1.vault, rounding);
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                let group = world.env.market_state().1;
                assert_eq!(
                    [
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.insurance,
                        group.source_claim_bound_total_num
                    ],
                    [0; 4]
                );
                let expected = [
                    DEPOSITS[0] + entitlement(0, FINAL_RESIDUAL),
                    0,
                    DEPOSITS[2] + entitlement(2, FINAL_RESIDUAL),
                    DEPOSITS[3] - FORFEITED,
                ];
                assert_eq!(expected, [1_315, 0, 1_585, 50]);
                assert_eq!(
                    world
                        .actors
                        .each_ref()
                        .map(|a| u128::from(world.env.token_amount(a.token))),
                    expected
                );
                assert_cu_within("Recovery receipt history", world.peak, CU_LIMIT);
                peak = peak.max(world.peak);
            }
        }
    }
    println!("INV-067 Recovery receipts: 8 worlds, 8 paid-prefix rollbacks, 16 positive top-ups, 32 portfolio deletions; peak {peak} CU");
}

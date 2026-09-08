//! INV-068 - Receipt uniqueness and monotonic top-ups.
//!
//! This public LiteSVM lifecycle creates an underfunded resolved receipt without writing
//! program-owned bytes. Two independent backing releases raise the terminal payout rate at
//! authenticated slots 13 and 14. `ClaimResolvedPayoutTopup` must increase the same immutable
//! receipt by exactly the SPL/vault payout after each release; an immediate retry after the
//! initial payment and both top-ups must land as an exact no-op. The shared terminal campaign then
//! proves the split schedule does not strand any funded portfolio.
//! Before paying, a one-field account matrix substitutes owner, resolved foreign market, portfolio,
//! destination, vault, and vault authority. Every cell must reject with an exact full-economic-state
//! frame, while the canonical receipt remains claimable.
//! Fresh public portfolio-close attempts at receipt creation and after each partial top-up must also
//! reject exactly. Once terminal settlement finalizes or clears the receipt, close must succeed with
//! exact custody, and the same account address cannot be reinitialized inside the resolved market.
//! Both the generic asset-lifecycle route and the dedicated restart route must reject exactly while
//! a receipt episode exists, excluding Recovery and asset-generation reuse from that episode.
//!
//! The shared route oracle also requires receipt face/prior-bound identity to remain immutable,
//! cumulative paid value to be monotonic, every claim delta to equal its external token delta, and
//! engine/SPL vault custody to reconcile after every successful instruction.
//!
//! The generated terminal-drain product extends INV-038's checkpoint history rather than
//! rebuilding the fixed lifecycle or INV-086's length-two receipt-conflict frontier. Generated
//! provider amounts, expiry spacing and route words cross eager/terminal-only claimant payment
//! with both owner/continuation orders. Every suffix step checks exact owner-local entitlement;
//! all five portfolios dematerialize and subsequent stale claims roll back exactly.
//!
//! The collateral-rail product uses the same receipt seed with a publicly configured secondary
//! mint. Four two-top-up rail schedules cross both payout handlers. One-atom-short secondary
//! reserves reject without consuming the claim; public SPL replenishment pays the exact due.
//! Mark-derived faces and checkpoint custody/senior stocks independently determine entitlement.
//! Per-mint custody, both-rail retries and fixed terminal continuation join INV-066/067 without
//! repeating their claimant-order campaigns. This is not a full setup-history entitlement oracle.

use super::*;

#[test]
fn v16_program_resolved_receipt_accepts_two_exact_topups_and_idempotent_retries() {
    let evidence = verify_resolved_receipt_split_topups()
        .expect("public split resolved-receipt top-up lifecycle");

    assert!(evidence.initial_paid < evidence.first_paid);
    assert!(evidence.first_paid < evidence.second_paid);
    assert!(evidence.second_paid < evidence.receipt_face);
    assert_eq!(
        evidence.first_paid - evidence.initial_paid,
        evidence.first_payout
    );
    assert_eq!(
        evidence.second_paid - evidence.first_paid,
        evidence.second_payout
    );
    assert_eq!(evidence.identity_substitutions_rejected, 6);
    assert_eq!(evidence.premature_close_rejections, 3);
    assert!(evidence.terminal_close_succeeded);
    assert!(evidence.resolved_reinit_rejected);
    assert_eq!(evidence.resolved_lifecycle_rejections, 2);
    assert_eq!(evidence.exact_noop_retries, 3);
    assert_eq!(evidence.terminal_actor_count, 5);
    assert_eq!(evidence.final_engine_vault, evidence.final_spl_vault);
}

#[test]
fn v16_program_generated_receipt_histories_preserve_terminal_drain() {
    super::inv_038_rounding_and_ratio_conservation::verify_generated_receipt_terminal_drain_histories();
}

// Unlike the seeded CU secondary-rail test, this product pays a live partial receipt on
// both rails. Mint creation, reserve funding, and every economic transition are public.
mod collateral_rails {
    use crate::support::{
        fuzz_model::public_resolved_receipt_seed_with_setup,
        reference_math::mul_div_floor_with_remainder,
        v16_svm::{V16Svm, TX_CU_LIMIT},
    };
    use percolator::{ResolvedPayoutReceiptV16, BOUND_SCALE};
    use percolator_prog::{error::PercolatorError, ix::Instruction as ProgInstruction};
    use solana_sdk::{
        account::Account,
        compute_budget::ComputeBudgetInstruction,
        instruction::{AccountMeta, Instruction, InstructionError},
        program_pack::Pack,
        pubkey::Pubkey,
        rent::Rent,
        signature::{keypair_from_seed, Keypair, Signer},
        system_instruction,
        transaction::{Transaction, TransactionError},
    };
    use std::collections::BTreeSet;

    const FACES: [u128; 5] = [20 * 50, 0, 24 * 50, 0, 0];
    const SECONDARY_SUPPLY: u64 = 10_000;
    const RESERVE_BUDGET: u128 = 500;
    const ATA_PROGRAM: Pubkey = solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

    fn secondary_mint() -> Keypair {
        keypair_from_seed(&[0x68; 32]).unwrap()
    }

    fn ata(owner: Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[
                owner.as_ref(),
                spl_token::ID.as_ref(),
                secondary_mint().pubkey().as_ref(),
            ],
            &ATA_PROGRAM,
        )
        .0
    }

    fn send(
        env: &mut V16Svm,
        instructions: Vec<Instruction>,
        extra: &[&Keypair],
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        let payer = Keypair::from_bytes(&env.actors[4].signer.to_bytes()).unwrap();
        env.expire_blockhash();
        let mut all = vec![ComputeBudgetInstruction::set_compute_unit_limit(
            TX_CU_LIMIT as u32,
        )];
        all.extend(instructions);
        let mut signers = vec![&payer];
        signers.extend_from_slice(extra);
        let tx = Transaction::new_signed_with_payer(
            &all,
            Some(&payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        let result = env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(error) => &error.meta,
        };
        assert!(meta.compute_units_consumed < TX_CU_LIMIT);
        result
    }

    fn install_secondary(env: &mut V16Svm) -> Result<(), String> {
        let mint = secondary_mint();
        let payer = env.actors[4].signer.pubkey();
        let decimals =
            spl_token::state::Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                .unwrap()
                .decimals;
        send(
            env,
            vec![
                system_instruction::create_account(
                    &payer,
                    &mint.pubkey(),
                    Rent::default().minimum_balance(spl_token::state::Mint::LEN),
                    spl_token::state::Mint::LEN as u64,
                    &spl_token::ID,
                ),
                spl_token::instruction::initialize_mint2(
                    &spl_token::ID,
                    &mint.pubkey(),
                    &payer,
                    None,
                    decimals,
                )
                .unwrap(),
            ],
            &[&mint],
        )
        .map_err(|error| format!("public secondary mint initialization: {error:?}"))?;
        env.install_secondary_collateral_mint(mint.pubkey())?;
        Ok(())
    }

    struct Reserve {
        source: Pubkey,
        vault: Pubkey,
        destinations: [Pubkey; 5],
        funded: u128,
    }

    impl Reserve {
        fn new(env: &mut V16Svm) -> Self {
            let payer = env.actors[4].signer.pubkey();
            let destinations = std::array::from_fn(|actor| ata(env.actors[actor].signer.pubkey()));
            for owner in env
                .actors
                .iter()
                .map(|actor| actor.signer.pubkey())
                .chain([env.vault_authority])
                .collect::<Vec<_>>()
            {
                send(
                    env,
                    vec![Instruction {
                        program_id: ATA_PROGRAM,
                        accounts: vec![
                            AccountMeta::new(payer, true),
                            AccountMeta::new(ata(owner), false),
                            AccountMeta::new_readonly(owner, false),
                            AccountMeta::new_readonly(secondary_mint().pubkey(), false),
                            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: vec![0],
                    }],
                    &[],
                )
                .expect("public secondary ATA creation");
            }
            // Actor 4 has already received its principal in the shared prefix. Its secondary
            // ATA is solely the reserve supplier, never a terminal payout destination here.
            let source = destinations[4];
            send(
                env,
                vec![spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &secondary_mint().pubkey(),
                    &source,
                    &payer,
                    &[],
                    SECONDARY_SUPPLY,
                )
                .unwrap()],
                &[],
            )
            .expect("public reserve supply minting");
            Self {
                source,
                vault: ata(env.vault_authority),
                destinations,
                funded: 0,
            }
        }

        fn fund(&mut self, env: &mut V16Svm, amount: u128) {
            let before = frame(env, self);
            let payer = env.actors[4].signer.pubkey();
            send(
                env,
                vec![spl_token::instruction::transfer(
                    &spl_token::ID,
                    &self.source,
                    &self.vault,
                    &payer,
                    &[],
                    u64::try_from(amount).unwrap(),
                )
                .unwrap()],
                &[],
            )
            .expect("public secondary reserve transfer");
            self.funded += amount;
            assert_frame_except(&before, env, &[self.source, self.vault]);
        }
    }

    fn frame(env: &V16Svm, reserve: &Reserve) -> Vec<(Pubkey, Option<Account>)> {
        let keys: BTreeSet<_> = env
            .all_economic_account_lamports()
            .into_iter()
            .map(|(key, _)| key)
            .chain(reserve.destinations)
            .chain([reserve.vault, secondary_mint().pubkey()])
            .chain(env.actors[..4].iter().map(|actor| actor.signer.pubkey()))
            .collect();
        // Actor 4 is this test's network-fee payer; all portfolio rent remains in the frame.
        keys.into_iter()
            .map(|key| (key, env.svm.get_account(&key)))
            .collect()
    }

    fn assert_frame_except(before: &[(Pubkey, Option<Account>)], env: &V16Svm, allowed: &[Pubkey]) {
        for (key, account) in before {
            if !allowed.contains(key) {
                assert_eq!(
                    &env.svm.get_account(key),
                    account,
                    "unrelated account {key}"
                );
            }
        }
    }

    fn payout(
        env: &mut V16Svm,
        reserve: &Reserve,
        actor: usize,
        secondary: bool,
        close: bool,
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        let instruction = if close {
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
        } else {
            ProgInstruction::ClaimResolvedPayoutTopup
        };
        send(
            env,
            vec![Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(env.actors[actor].signer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.actors[actor].portfolio, false),
                    AccountMeta::new(
                        if secondary {
                            reserve.destinations[actor]
                        } else {
                            env.actors[actor].destination_token
                        },
                        false,
                    ),
                    AccountMeta::new(if secondary { reserve.vault } else { env.vault }, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: instruction.encode(),
            }],
            &[],
        )
    }

    struct Oracle {
        receipt: ResolvedPayoutReceiptV16,
        residual: u128,
        initial_vault: u128,
        capitals: [u128; 5],
        primary_destinations: [u128; 5],
        paid: [u128; 5],
        cleared: [bool; 5],
    }

    impl Oracle {
        fn new(env: &V16Svm) -> Self {
            let group = env.primary_market_state().1;
            let receipt = env
                .primary_portfolio(0)
                .resolved_payout_receipt
                .try_to_runtime()
                .unwrap();
            assert!(receipt.present && !receipt.finalized);
            assert_eq!(receipt.terminal_positive_claim_face, FACES[0]);
            assert_eq!(env.primary_portfolio(2).pnl.get(), FACES[2] as i128);
            let reserves = group
                .source_credit
                .iter()
                .map(|source| source.fresh_reserved_backing_num)
                .sum::<u128>();
            assert_eq!(reserves % BOUND_SCALE, 0);
            // Recover the checkpoint residual from custody and disjoint senior stocks, adding
            // back the one already paid junior receipt. Never use the deployed payout rate.
            let residual = group.vault + receipt.paid_effective
                - group.c_tot
                - group.insurance
                - group.backing_provider_earnings_total
                - reserves / BOUND_SCALE;
            assert_eq!(residual, group.resolved_payout_ledger.snapshot_residual);
            let out = Self {
                receipt,
                residual,
                initial_vault: group.vault,
                capitals: std::array::from_fn(|actor| env.primary_portfolio(actor).capital.get()),
                primary_destinations: std::array::from_fn(|actor| {
                    u128::from(env.token_amount(env.actors[actor].destination_token))
                }),
                paid: [receipt.paid_effective, 0, 0, 0, 0],
                cleared: [false; 5],
            };
            assert_eq!(out.target(0), receipt.paid_effective);
            assert!(out.target(0) < FACES[0]);
            out
        }

        fn target(&self, actor: usize) -> u128 {
            mul_div_floor_with_remainder(FACES[actor], self.residual, FACES.iter().sum())
                .unwrap()
                .0
        }

        fn check(&mut self, env: &V16Svm, reserve: &Reserve) {
            let group = env.primary_market_state().1;
            let ledger = group.resolved_payout_ledger;
            let bound = FACES.iter().sum::<u128>() * BOUND_SCALE;
            assert_eq!(ledger.snapshot_residual, self.residual);
            assert_eq!(ledger.current_payout_rate_num, self.residual * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, bound);
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num
                    + ledger.terminal_claim_bound_unreceipted_num,
                bound
            );
            let mut unreceipted = 0;
            let mut total = 0;
            let mut secondary_paid = 0;
            for actor in 0..5 {
                let account = env.primary_portfolio(actor);
                let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
                assert!(
                    account.capital.get() == 0 || account.capital.get() == self.capitals[actor]
                );
                let paid = if receipt.present {
                    assert!(!self.cleared[actor]);
                    assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                    assert_eq!(
                        receipt.prior_bound_contribution_num,
                        FACES[actor] * BOUND_SCALE
                    );
                    assert_eq!(receipt.live_released_face_at_receipt, 0);
                    assert!(!receipt.finalized);
                    assert_eq!(account.pnl.get(), 0);
                    if actor == 0 {
                        let mut identity = receipt;
                        identity.paid_effective = self.receipt.paid_effective;
                        assert_eq!(identity, self.receipt);
                    }
                    receipt.paid_effective
                } else if account.pnl.get() != 0 {
                    assert_eq!(account.pnl.get(), FACES[actor] as i128);
                    unreceipted += FACES[actor] * BOUND_SCALE;
                    0
                } else {
                    self.cleared[actor] = true;
                    self.target(actor)
                };
                assert!(self.paid[actor] <= paid && paid <= self.target(actor));
                self.paid[actor] = paid;
                let expected = self.capitals[actor] - account.capital.get() + paid
                    - if actor == 0 {
                        self.receipt.paid_effective
                    } else {
                        0
                    };
                let primary = u128::from(env.token_amount(env.actors[actor].destination_token))
                    - self.primary_destinations[actor];
                let secondary = if actor == 4 {
                    0
                } else {
                    u128::from(env.token_amount(reserve.destinations[actor]))
                };
                assert_eq!(
                    primary + secondary,
                    expected,
                    "owner {actor} entitlement across both rails"
                );
                secondary_paid += secondary;
                total += expected;
            }
            assert_eq!(ledger.terminal_claim_bound_unreceipted_num, unreceipted);
            assert_eq!(group.vault, self.initial_vault - total);
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                self.initial_vault - total + secondary_paid
            );
            assert_eq!(
                u128::from(env.token_amount(reserve.vault)),
                reserve.funded - secondary_paid
            );
            assert_eq!(
                u128::from(env.token_amount(reserve.source)),
                u128::from(SECONDARY_SUPPLY) - reserve.funded
            );
            assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            let mint = spl_token::state::Mint::unpack(
                &env.svm
                    .get_account(&secondary_mint().pubkey())
                    .unwrap()
                    .data,
            )
            .unwrap();
            assert_eq!(mint.supply, SECONDARY_SUPPLY);
            assert_eq!(
                u128::from(env.token_amount(reserve.source))
                    + u128::from(env.token_amount(reserve.vault))
                    + secondary_paid,
                u128::from(mint.supply)
            );
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Outcome {
        market: Vec<u8>,
        portfolios: Vec<Vec<u8>>,
        paid: [u128; 5],
        combined_custody: u128,
        reserve_supplier: u64,
    }

    fn run(rails: [bool; 2], close: bool) -> Outcome {
        let mut env =
            public_resolved_receipt_seed_with_setup([97, 103], 14, Some(install_secondary))
                .unwrap();
        let mut reserve = Reserve::new(&mut env);
        let mut oracle = Oracle::new(&env);
        oracle.check(&env, &reserve);
        for (index, domain) in [3, 5].into_iter().enumerate() {
            env.warp_to_slot(13 + index as u64);
            let released = [97 + 250, 103 + 2 * 5 * 4][index];
            assert_eq!(
                env.primary_market_state().1.source_credit[domain].fresh_reserved_backing_num,
                released * BOUND_SCALE
            );
            let before = frame(&env, &reserve);
            env.close_resolved_primary_signed(2)
                .expect("public backing release");
            assert_frame_except(&before, &env, &[env.market, env.actors[2].portfolio]);
            oracle.residual += released;
            oracle.check(&env, &reserve);
            let due = oracle.target(0) - oracle.paid[0];
            assert!(due > 1 && oracle.target(0) < FACES[0]);
            assert_ne!(
                mul_div_floor_with_remainder(FACES[0], oracle.residual, FACES.iter().sum())
                    .unwrap()
                    .1,
                0
            );
            if rails[index] {
                assert_eq!(env.token_amount(reserve.vault), 0);
                reserve.fund(&mut env, due - 1);
                oracle.check(&env, &reserve);
                let before = frame(&env, &reserve);
                let error = payout(&mut env, &reserve, 0, true, close)
                    .expect_err("one-atom-short selected reserve");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        1,
                        InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
                    )
                );
                assert_eq!(
                    frame(&env, &reserve),
                    before,
                    "liquidity rejection must preserve the pending receipt"
                );
                reserve.fund(&mut env, 1);
                oracle.check(&env, &reserve);
            }
            let before = frame(&env, &reserve);
            payout(&mut env, &reserve, 0, rails[index], close)
                .expect("exact top-up on selected rail");
            assert_frame_except(
                &before,
                &env,
                &[
                    env.market,
                    env.actors[0].portfolio,
                    if rails[index] {
                        reserve.vault
                    } else {
                        env.vault
                    },
                    if rails[index] {
                        reserve.destinations[0]
                    } else {
                        env.actors[0].destination_token
                    },
                ],
            );
            assert_eq!(
                env.primary_portfolio(0)
                    .resolved_payout_receipt
                    .try_to_runtime()
                    .unwrap()
                    .paid_effective,
                oracle.target(0)
            );
            oracle.check(&env, &reserve);
            for secondary in [false, true] {
                let before = frame(&env, &reserve);
                payout(&mut env, &reserve, 0, secondary, false)
                    .expect("opposite-rail receipt retry");
                assert_eq!(
                    frame(&env, &reserve),
                    before,
                    "both rails share the same paid receipt"
                );
                oracle.check(&env, &reserve);
            }
        }

        // Equal final funding keeps supplier endowment and combined custody comparable even
        // though individual mint inventories differ with the selected payout rails.
        assert!(reserve.funded < RESERVE_BUDGET);
        let remainder = RESERVE_BUDGET - reserve.funded;
        reserve.fund(&mut env, remainder);
        oracle.check(&env, &reserve);

        // Keep terminal continuation fixed: only the two partial payments vary their rails.
        // This joins the rail product to the existing terminal obligation without replicating
        // its claimant-order or generated-scheduler campaigns.
        for _ in 0..4 {
            for actor in [2, 0, 1, 3, 4] {
                let before = frame(&env, &reserve);
                match payout(&mut env, &reserve, actor, false, true) {
                    Ok(_) => assert_frame_except(
                        &before,
                        &env,
                        &[
                            env.market,
                            env.actors[actor].portfolio,
                            env.actors[actor].destination_token,
                            env.vault,
                        ],
                    ),
                    Err(error) => {
                        assert_eq!(
                            error.err,
                            TransactionError::InstructionError(
                                1,
                                InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                            )
                        );
                        assert_eq!(frame(&env, &reserve), before);
                    }
                }
                oracle.check(&env, &reserve);
            }
        }
        for actor in 0..5 {
            assert_eq!(oracle.paid[actor], oracle.target(actor));
            let account = env.primary_portfolio(actor);
            assert_eq!(account.capital.get(), 0);
            assert_eq!(account.pnl.get(), 0);
            assert!(
                !account
                    .resolved_payout_receipt
                    .try_to_runtime()
                    .unwrap()
                    .present
            );
            let before = frame(&env, &reserve);
            for secondary in [false, true] {
                payout(&mut env, &reserve, actor, secondary, false)
                    .expect("terminal receipt retry");
                assert_eq!(frame(&env, &reserve), before);
            }
        }
        Outcome {
            market: env.market_data(false),
            portfolios: env.all_primary_portfolio_data(),
            paid: oracle.paid,
            combined_custody: u128::from(env.token_amount(env.vault))
                + u128::from(env.token_amount(reserve.vault)),
            reserve_supplier: env.token_amount(reserve.source),
        }
    }

    #[test]
    fn v16_program_partial_receipt_topups_are_collateral_rail_independent() {
        let mut baseline = None;
        for close in [false, true] {
            for rails in [[false, false], [false, true], [true, false], [true, true]] {
                let outcome = run(rails, close);
                if let Some(expected) = &baseline {
                    assert_eq!(&outcome, expected, "rail schedule {rails:?}, close={close}");
                } else {
                    baseline = Some(outcome);
                }
            }
        }
        eprintln!(
            "INV-066/067/068 collateral rails: 8 worlds, 16 positive partial top-ups, \
            8 exact liquidity rejections, 32 partial and 80 terminal cross-rail no-op retries"
        );
    }
}

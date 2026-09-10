//! INV-039 - Pending-loss obligation durability.
//!
//! This CU/SVM owner invokes the shared public Recovery-order world formerly owned by INV-073.
//! Both owner landing orders create a real zero-basis, nonzero-loss-weight obligation. While it is
//! retained, `ClosePortfolio` must return an instruction error with exact market, portfolio, vault,
//! and lamport rollback. The opposite owner then exits, permissionless cranks release the
//! obligation in bounded work, every loss-weight/count aggregate reaches zero, all users receive
//! their exact terminal entitlement, and all portfolios close. A valid pending obligation does not
//! coexist with `ResetPending`: the pinned engine's retain/release/clear contracts and
//! `proof_v16_public_finalize_side_reset_rejects_each_blocker_without_mutation` own that
//! unreachability and exact reset-gate frame, while the wrapper exposes no direct state writer.
//! INV-088 source-rosters every wrapper-to-engine transition, so a new removal route reopens this
//! composition. Secondary coverage: INV-073. The same run proves both landing orders retain a
//! bounded owner exit and permissionless terminal continuation while preserving exact loss
//! attribution, without duplicating an expensive public lifecycle.
//!
//! The independent two-domain matrix below crosses mirrored sides, debtor settlement order,
//! and payout order. It checks a still-unpaid domain while another domain is fully settled,
//! including late transaction rejection after staged debtor settlement. All construction uses
//! System/SPL/ATA and wrapper instructions. That matrix resolves after both obligations release.
//! The final test instead resolves with a zero-basis obligation and unbooked opposing debt:
//! resolved detach must carry the claim forward without payout, a waiting retry rejects exactly,
//! and permissionless debtor settlement unlocks the exact original entitlement once. Bankruptcy
//! residuals, ADL, and nonzero funding/fees across that resolved boundary remain outside this test.
//! The `close_reopen` sibling instead creates a bankruptcy residual by matched reduction and
//! deletes/recreates the debtor before the flat holder settles B. A composed bystander payout
//! must roll back before booking; recreation after booking preserves the holder's exact debit.

#[test]
fn v16_program_pending_obligation_blocks_close_then_releases() {
    super::inv_073_no_permanent_user_lock::
        verify_recovery_forfeit_orders_preserve_loss_and_terminal_exit();
}

use super::*;

#[path = "inv_039_pending_loss_transfer_route.rs"]
mod transfer_route;

#[path = "inv_039_pending_loss_close_reopen.rs"]
mod close_reopen;

#[path = "inv_039_pending_loss_cohort_reduction.rs"]
mod cohort_reduction;

#[path = "inv_039_pending_loss_resolved_histories.rs"]
mod resolved_histories;

const ATTRIBUTION_DEPOSITS: [u128; 5] = [200_000, 180_000, 300_000, 250_000, 777];
const ATTRIBUTION_PRICE_MOVES: [i128; 2] = [30_000, 20_000];

struct AttributionActor {
    owner: Keypair,
    portfolio: Pubkey,
    token: Pubkey,
}

struct AttributionWorld {
    env: V16CuEnv,
    actors: Vec<AttributionActor>,
    quantities: [i128; 4],
}

impl AttributionWorld {
    fn new(reverse_sides: bool) -> Self {
        let params = V16CuMarketParams {
            max_portfolio_assets: 3,
            max_abs_funding_e9_per_slot: 0,
            liquidation_fee_bps: 0,
            ..production_risk_params()
        };
        Self::new_with_params(reverse_sides, params)
    }

    fn new_with_params(reverse_sides: bool, params: V16CuMarketParams) -> Self {
        let mut svm = LiteSVM::new();
        let program_id = percolator_prog::id();
        for (id, path) in [
            (program_id, program_path()),
            (spl_token::ID, spl_token_program_path()),
            (
                associated_token_program_id(),
                associated_token_program_path(),
            ),
        ] {
            svm.add_program(id, &std::fs::read(path).unwrap());
        }
        let payer = Keypair::new();
        let admin = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
        let mint = Keypair::new();
        system_create_account_for_test(&mut svm, &payer, &mint, Mint::LEN, spl_token::ID);
        send_raw_tx(
            &mut svm,
            &payer,
            spl_token::instruction::initialize_mint2(
                &spl_token::ID,
                &mint.pubkey(),
                &admin.pubkey(),
                None,
                0,
            )
            .unwrap(),
            &[],
        )
        .unwrap();
        let market = Keypair::new();
        system_create_account_for_test(
            &mut svm,
            &payer,
            &market,
            state::market_account_len_for_capacity(3).unwrap(),
            program_id,
        );
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
        let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
        let init_market_cu = send_tx(
            &mut svm,
            program_id,
            &payer,
            init_market_instruction(&params),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(mint.pubkey(), false),
            ],
            &[&admin],
        )
        .unwrap();
        let mut env = V16CuEnv {
            svm,
            program_id,
            payer,
            admin,
            init_market_cu,
            market: market.pubkey(),
            mint: mint.pubkey(),
            vault,
            vault_authority,
            portfolio_account_len: state::portfolio_account_len_for_market_slots(3).unwrap(),
            portfolios: Vec::new(),
        };
        let mut actors = Vec::new();
        for deposit in ATTRIBUTION_DEPOSITS {
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                program_id,
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
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    deposit as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio.pubkey(), deposit),
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
            actors.push(AttributionActor {
                owner,
                portfolio: portfolio.pubkey(),
                token,
            });
        }
        for asset in 0..3 {
            env.configure_auth_mark_for_asset_as_admin(asset, 0, params.initial_price);
        }
        env.configure_permissionless_resolve_with_cu(100, 5);
        let sign = if reverse_sides { -1 } else { 1 };
        let q = POS_SCALE as i128 * sign;
        Self {
            env,
            actors,
            quantities: [q, -q, -2 * q, 2 * q],
        }
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        // The distinct transaction fee payer is the only excluded writable account.
        let mut keys = vec![
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
        ];
        for actor in &self.actors {
            keys.extend([actor.owner.pubkey(), actor.portfolio, actor.token]);
        }
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    fn check(&self, basis: [i128; 4], pending: [bool; 4]) {
        self.check_with_deleted_debtor(basis, pending, None);
    }

    fn check_with_deleted_debtor(
        &self,
        basis: [i128; 4],
        pending: [bool; 4],
        deleted: Option<usize>,
    ) {
        let (_, group) = self.env.market_state();
        let mut capital = 0;
        let mut pnl_positive = 0;
        let mut oi = [[0u128; 2]; 3];
        let mut weight = oi;
        let mut stored = [[0u64; 2]; 3];
        let mut obligations = stored;
        for (i, actor) in self.actors.iter().enumerate() {
            if Some(i) == deleted {
                assert!(
                    i == 1 || i == 3,
                    "only an explicitly settled debtor is absent"
                );
                assert_eq!(basis[i], 0);
                assert!(!pending[i]);
                assert!(self
                    .env
                    .svm
                    .get_account(&actor.portfolio)
                    .map_or(true, |account| account.lamports == 0
                        && account.data.is_empty()));
                continue;
            }
            let account = self.env.portfolio_state(actor.portfolio);
            assert_eq!(account.owner, actor.owner.pubkey().to_bytes());
            capital += account.capital.get();
            pnl_positive += account.pnl.get().max(0) as u128;
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
                .collect();
            let retained = i < 4 && (basis[i] != 0 || pending[i]);
            assert_eq!(
                legs.len(),
                usize::from(retained),
                "actor {i}: exact leg ownership"
            );
            if retained {
                let asset = i / 2 + 1;
                let side = usize::from(self.quantities[i] < 0);
                let leg = legs[0];
                assert_eq!(
                    leg.asset_index as usize, asset,
                    "actor {i}: obligation domain"
                );
                assert_eq!(
                    leg.side,
                    if side == 0 {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(
                    leg.basis_pos_q, basis[i],
                    "actor {i}: pending is not exposure"
                );
                assert_eq!(leg.loss_weight, self.quantities[i].unsigned_abs());
                oi[asset][side] += basis[i].unsigned_abs();
                weight[asset][side] += self.quantities[i].unsigned_abs();
                stored[asset][side] += 1;
                obligations[asset][side] += u64::from(pending[i]);
            }
        }
        assert_eq!(capital, group.c_tot);
        assert_eq!(pnl_positive, group.pnl_pos_tot);
        for asset in 0..3 {
            let a = group.assets[asset];
            assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
            assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[asset]);
            assert_eq!(
                [a.loss_weight_sum_long, a.loss_weight_sum_short],
                weight[asset]
            );
            assert_eq!(
                [a.stored_pos_count_long, a.stored_pos_count_short],
                stored[asset]
            );
            assert_eq!(
                [
                    a.pending_obligation_count_long,
                    a.pending_obligation_count_short
                ],
                obligations[asset]
            );
        }
        assert_eq!(self.env.token_amount(self.env.vault) as u128, group.vault);
        let total = group.vault
            + self
                .actors
                .iter()
                .map(|actor| self.env.token_amount(actor.token) as u128)
                .sum::<u128>();
        assert_eq!(total, ATTRIBUTION_DEPOSITS.iter().sum::<u128>());
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply as u128, total);
    }

    fn forfeit(&mut self, actor: usize) {
        let a = &self.actors[actor];
        let cu = self.env.forfeit_recovery_leg_with_cu(
            &a.owner,
            a.portfolio,
            (actor / 2 + 1) as u16,
            u128::MAX,
        );
        assert_cu_within("INV-039 independent obligation forfeit", cu, CRANK_CU_LIMIT);
    }

    fn debt(&self, pair: usize) -> u128 {
        self.quantities[2 * pair].unsigned_abs() * ATTRIBUTION_PRICE_MOVES[pair] as u128 / POS_SCALE
    }

    fn reject_settlement_then_close(&mut self, pair: usize) {
        let before = self.frame();
        self.env.svm.expire_blockhash();
        let debtor = &self.actors[2 * pair + 1];
        let creditor = &self.actors[2 * pair];
        let settle = Instruction {
            program_id: self.env.program_id,
            data: ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id: self.env.portfolio_id(debtor.portfolio),
                position_epoch: self.env.portfolio_position_epoch(debtor.portfolio),
                asset_index: (pair + 1) as u16,
                b_delta_budget: u128::MAX,
            }
            .encode(),
            accounts: vec![
                AccountMeta::new(debtor.owner.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(debtor.portfolio, false),
            ],
        };
        let close = Instruction {
            program_id: self.env.program_id,
            data: self.env.close_portfolio_ix(creditor.portfolio).encode(),
            accounts: vec![
                AccountMeta::new(creditor.owner.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(creditor.portfolio, false),
            ],
        };
        let error = send_raw_ixs(
            &mut self.env.svm,
            &self.env.payer,
            vec![heap_ix(), cu_ix(), settle, close],
            &[&debtor.owner, &creditor.owner],
        )
        .expect_err("close must reject after a successful staged debt settlement");
        assert!(error.contains("InstructionError(3, Custom(21))"), "{error}");
        assert_eq!(
            self.frame(),
            before,
            "late rejection must restore debt, obligation, sequences, custody, and lamports"
        );
    }

    fn payout(&mut self, actor: usize, claim: bool) -> Result<u64, String> {
        self.env.svm.expire_blockhash();
        let a = &self.actors[actor];
        self.env.send(
            if claim {
                ProgInstruction::ClaimResolvedPayoutTopup
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            },
            vec![
                AccountMeta::new_readonly(a.owner.pubkey(), false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(a.portfolio, false),
                AccountMeta::new(a.token, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
    }
}

#[test]
fn v16_program_pending_obligations_release_only_the_settled_domain_across_payout_orders() {
    for reverse_sides in [false, true] {
        for pair_order in [[0usize, 1], [1, 0]] {
            for reverse_payouts in [false, true] {
                let mut world = AttributionWorld::new(reverse_sides);
                let mut basis = [0; 4];
                let mut pending = [false; 4];
                world.check(basis, pending);
                for pair in 0..2 {
                    let winner = &world.actors[2 * pair];
                    let debtor = &world.actors[2 * pair + 1];
                    world.env.trade_asset_with_cu(
                        (pair + 1) as u16,
                        &winner.owner,
                        winner.portfolio,
                        &debtor.owner,
                        debtor.portfolio,
                        world.quantities[2 * pair],
                        1_000_000,
                        0,
                    );
                    basis[2 * pair] = world.quantities[2 * pair];
                    basis[2 * pair + 1] = world.quantities[2 * pair + 1];
                    world.check(basis, pending);
                }
                world.env.svm.warp_to_slot(20);
                for (pair, move_atoms) in ATTRIBUTION_PRICE_MOVES.into_iter().enumerate() {
                    let mark = 1_000_000 + move_atoms * world.quantities[2 * pair].signum();
                    world
                        .env
                        .push_auth_mark_for_asset_as_admin((pair + 1) as u16, 20, mark as u64);
                }
                world.env.crank(
                    world.actors[4].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 20,
                        observations: crank_observations_for_assets(&[1, 2]),
                    },
                );
                world.check(basis, pending);
                for (pair, price_move) in ATTRIBUTION_PRICE_MOVES.into_iter().enumerate() {
                    assert_eq!(
                        world.env.market_state().1.assets[pair + 1].effective_price,
                        (1_000_000 + price_move * world.quantities[2 * pair].signum()) as u64
                    );
                }
                for pair in 0..2 {
                    // Book credit publicly, but leave the opposing debtor's accrual untouched.
                    world.env.crank(
                        world.actors[2 * pair].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 20,
                            observations: crank_observations((pair + 1) as u16),
                        },
                    );
                    world.check(basis, pending);
                    world.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        (pair + 1) as u16,
                        20,
                        0,
                    );
                    world.forfeit(2 * pair);
                    basis[2 * pair] = 0;
                    pending[2 * pair] = true;
                    world.check(basis, pending);
                    let debtor = world
                        .env
                        .portfolio_state(world.actors[2 * pair + 1].portfolio);
                    assert_eq!(debtor.capital.get(), ATTRIBUTION_DEPOSITS[2 * pair + 1]);
                    assert_eq!(
                        debtor.pnl.get(),
                        0,
                        "opposing economic debt is still unbooked"
                    );
                    assert_eq!(
                        world
                            .env
                            .portfolio_state(world.actors[2 * pair].portfolio)
                            .pnl
                            .get(),
                        world.debt(pair) as i128,
                        "booked claim survives exposure removal"
                    );
                }
                for pair in pair_order {
                    world.reject_settlement_then_close(pair);
                    world.check(basis, pending);
                    let other = 1 - pair;
                    let other_group_before = world.env.market_state().1;
                    let other_accounts_before: Vec<_> = [2 * other, 2 * other + 1]
                        .into_iter()
                        .map(|i| world.env.svm.get_account(&world.actors[i].portfolio))
                        .collect();
                    world.forfeit(2 * pair + 1);
                    basis[2 * pair + 1] = 0;
                    world.check(basis, pending);
                    let cu = world.env.crank(
                        world.actors[2 * pair].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 20,
                            observations: crank_observations((pair + 1) as u16),
                        },
                    );
                    assert_cu_within("INV-039 domain-local release", cu, CRANK_CU_LIMIT);
                    pending[2 * pair] = false;
                    world.check(basis, pending);
                    let after = world.env.market_state().1;
                    assert_eq!(
                        after.assets[other + 1],
                        other_group_before.assets[other + 1]
                    );
                    for domain in 2 * (other + 1)..2 * (other + 2) {
                        assert_eq!(
                            after.source_credit[domain],
                            other_group_before.source_credit[domain]
                        );
                        assert_eq!(
                            after.source_backing_buckets[domain],
                            other_group_before.source_backing_buckets[domain]
                        );
                        assert_eq!(
                            after.insurance_domain_spent[domain],
                            other_group_before.insurance_domain_spent[domain]
                        );
                    }
                    for (i, before) in [2 * other, 2 * other + 1]
                        .into_iter()
                        .zip(other_accounts_before)
                    {
                        assert_eq!(
                            world.env.svm.get_account(&world.actors[i].portfolio),
                            before
                        );
                    }
                }
                let mut expected = ATTRIBUTION_DEPOSITS;
                for i in 0..2 {
                    let debt = world.debt(i);
                    expected[2 * i] += debt;
                    expected[2 * i + 1] -= debt;
                    assert_eq!(
                        world
                            .env
                            .portfolio_state(world.actors[2 * i + 1].portfolio)
                            .capital
                            .get(),
                        ATTRIBUTION_DEPOSITS[2 * i + 1] - debt
                    );
                    assert_eq!(
                        world
                            .env
                            .portfolio_state(world.actors[2 * i].portfolio)
                            .pnl
                            .get(),
                        debt as i128
                    );
                }
                world.env.resolve();
                world.env.svm.warp_to_slot(25);
                world.check(basis, pending);
                let order = if reverse_payouts {
                    [4, 3, 2, 1, 0]
                } else {
                    [0, 1, 2, 3, 4]
                };
                for _ in 0..8 {
                    for actor in order {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            continue;
                        }
                        let before = world.frame();
                        match world.payout(actor, false) {
                            Ok(cu) => {
                                assert_cu_within("INV-039 terminal payout", cu, CUSTODY_CU_LIMIT);
                                assert_ne!(world.frame(), before, "successful exit must progress");
                            }
                            Err(error) => {
                                assert!(is_engine_non_progress_error(&error), "{error}");
                                assert_eq!(world.frame(), before);
                            }
                        }
                        world.check(basis, pending);
                        for (i, limit) in expected.into_iter().enumerate() {
                            assert!(world.env.token_amount(world.actors[i].token) as u128 <= limit);
                        }
                    }
                    if world.actors.iter().enumerate().all(|(i, actor)| {
                        world.env.token_amount(actor.token) as u128 == expected[i]
                    }) {
                        break;
                    }
                }
                for (i, entitlement) in expected.into_iter().enumerate() {
                    assert_eq!(
                        world.env.token_amount(world.actors[i].token) as u128,
                        entitlement
                    );
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[i].portfolio
                    ));
                    let before = world.frame();
                    world.payout(i, true).expect("finalized receipt retry");
                    assert_eq!(world.frame(), before, "settled claim retry is exactly once");
                }
                assert_eq!(world.env.market_state().1.vault, 0);
                for actor in order {
                    let a = &world.actors[actor];
                    let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                    assert_cu_within(
                        "INV-039 mechanical portfolio deletion",
                        cu,
                        CUSTODY_CU_LIMIT,
                    );
                    assert_eq!(world.env.token_amount(a.token) as u128, expected[actor]);
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
            }
        }
    }
    println!("INV-039: 8 worlds, 16 staged-settlement rollbacks, 16 domain-local releases, 40 exact payouts and terminal deletions");
}

#[test]
fn v16_program_resolve_with_pending_obligation_defers_claim_until_debtor_settles() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut peak_close_cu = 0;
    let mut close = |world: &mut AttributionWorld, actor: usize| {
        world.env.svm.expire_blockhash();
        let a = &world.actors[actor];
        let ix = Instruction {
            program_id: world.env.program_id,
            data: ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
            .encode(),
            accounts: vec![
                AccountMeta::new_readonly(a.owner.pubkey(), false),
                AccountMeta::new(world.env.market, false),
                AccountMeta::new(a.portfolio, false),
                AccountMeta::new(a.token, false),
                AccountMeta::new(world.env.vault, false),
                AccountMeta::new_readonly(world.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        };
        // Only the independent fee payer signs, including the debtor's loss settlement.
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), ix],
            Some(&world.env.payer.pubkey()),
            &[&world.env.payer],
            world.env.svm.latest_blockhash(),
        );
        let result = world.env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(failure) => &failure.meta,
        };
        peak_close_cu = peak_close_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "INV-039 pending-obligation resolved continuation",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        result
    };

    for reverse_sides in [false, true] {
        let mut world = AttributionWorld::new(reverse_sides);
        let creditor = world.actors[0].portfolio;
        let debtor = world.actors[1].portfolio;
        let q = world.quantities[0];
        let debt = world.debt(0);
        assert_eq!(debt, 30_000);
        let cu = world.env.trade_asset_with_cu(
            1,
            &world.actors[0].owner,
            creditor,
            &world.actors[1].owner,
            debtor,
            q,
            1_000_000,
            0,
        );
        assert_cu_within("INV-039 pending-obligation opening", cu, TRADE_CU_LIMIT);
        let debtor_unsettled = world.env.svm.get_account(&debtor);
        world.env.svm.warp_to_slot(20);
        let mark = (1_000_000 + ATTRIBUTION_PRICE_MOVES[0] * q.signum()) as u64;
        world.env.push_auth_mark_for_asset_as_admin(1, 20, mark);
        for portfolio in [world.actors[4].portfolio, creditor] {
            let cu = world.env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 20,
                    observations: crank_observations(1),
                },
            );
            assert_cu_within("INV-039 creditor-only accrual", cu, CRANK_CU_LIMIT);
        }
        assert_eq!(world.env.market_state().1.assets[1].effective_price, mark);
        let cu = world.env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            1,
            20,
            0,
        );
        assert_cu_within("INV-039 pending-obligation shutdown", cu, CRANK_CU_LIMIT);
        world.forfeit(0);
        world.check([0, -q, 0, 0], [true, false, false, false]);
        let retained = world.env.portfolio_state(creditor);
        assert_eq!(retained.capital.get(), ATTRIBUTION_DEPOSITS[0]);
        assert_eq!(retained.pnl.get(), debt as i128);
        assert_eq!(world.env.svm.get_account(&debtor), debtor_unsettled);
        assert_eq!(world.env.portfolio_state(debtor).pnl.get(), 0);
        assert_eq!(
            world.env.portfolio_state(debtor).capital.get(),
            ATTRIBUTION_DEPOSITS[1]
        );

        let before_resolve = world.frame();
        let asset_before = world.env.market_state().1.assets[1];
        let cu = send_tx(
            &mut world.env.svm,
            world.env.program_id,
            &world.env.payer,
            ProgInstruction::ResolveMarket {
                asset_generation_frontier: 0,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(world.env.admin.pubkey(), true),
                AccountMeta::new(world.env.market, false),
            ],
            &[&world.env.admin],
        )
        .expect("resolve must preserve the still-unsettled cohort");
        assert_cu_within(
            "INV-039 resolve with retained obligation",
            cu,
            CRANK_CU_LIMIT,
        );
        let resolved = world.env.market_state().1;
        assert_eq!(resolved.mode, MarketModeV16::Resolved);
        assert_eq!(resolved.assets[1], asset_before);
        for (key, account) in &before_resolve {
            if *key != world.env.market {
                assert_eq!(world.env.svm.get_account(key), *account);
            }
        }
        world.check([0, -q, 0, 0], [true, false, false, false]);
        world.env.svm.warp_to_slot(25);

        // Detaching the zero-basis leg is not payment or forgiveness of its cohort's debt.
        close(&mut world, 0).expect("bounded resolved obligation detach");
        world.check([0, -q, 0, 0], [false; 4]);
        let waiting = world.env.portfolio_state(creditor);
        assert_eq!(waiting.capital, retained.capital);
        assert_eq!(waiting.pnl, retained.pnl);
        assert_eq!(waiting.source_domains, retained.source_domains);
        assert!(!resolved_receipt(&waiting).present);
        assert!(!world.env.market_state().1.payout_snapshot_captured);
        assert_eq!(world.env.token_amount(world.actors[0].token), 0);
        assert_eq!(world.env.svm.get_account(&debtor), debtor_unsettled);
        let before_retry = world.frame();
        let failure = close(&mut world, 0).expect_err("unbooked debt must defer claimant payout");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
            )
        );
        assert_eq!(
            world.frame(),
            before_retry,
            "waiting retry must roll back exactly"
        );

        close(&mut world, 1).expect("permissionless debtor settlement remains available");
        world.check([0; 4], [false; 4]);
        assert!(resolved_portfolio_is_terminal(&world.env, debtor));
        assert_eq!(
            world.env.token_amount(world.actors[1].token) as u128,
            ATTRIBUTION_DEPOSITS[1] - debt,
            "the original debtor pays its loss exactly once"
        );
        assert_eq!(world.env.portfolio_state(creditor).pnl.get(), debt as i128);
        close(&mut world, 0).expect("settled opposing debt unlocks the retained claim");
        world.check([0; 4], [false; 4]);
        assert!(resolved_portfolio_is_terminal(&world.env, creditor));
        assert_eq!(
            world.env.token_amount(world.actors[0].token) as u128,
            ATTRIBUTION_DEPOSITS[0] + debt,
            "resolved detach neither loses nor duplicates the original claim"
        );

        for actor in 0..2 {
            let before = world.frame();
            let failure = close(&mut world, actor).expect_err("paid cohort cannot settle twice");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                )
            );
            assert_eq!(world.frame(), before);
        }
        for actor in &world.actors[2..] {
            for key in [actor.owner.pubkey(), actor.portfolio, actor.token] {
                let before = before_resolve.iter().find(|(k, _)| *k == key).unwrap();
                assert_eq!(world.env.svm.get_account(&key), before.1);
            }
        }
        for actor in 2..world.actors.len() {
            close(&mut world, actor).expect("unrelated senior principal remains fully payable");
            assert_eq!(
                world.env.token_amount(world.actors[actor].token) as u128,
                ATTRIBUTION_DEPOSITS[actor]
            );
            world.check([0; 4], [false; 4]);
        }
        assert_eq!(world.env.market_state().1.vault, 0);
        for actor in &world.actors {
            assert!(resolved_portfolio_is_terminal(&world.env, actor.portfolio));
            let cu = world
                .env
                .close_portfolio_with_cu(&actor.owner, actor.portfolio);
            assert_cu_within("INV-039 settled cohort deletion", cu, CUSTODY_CU_LIMIT);
        }
        assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    }
    println!("INV-039: 2 pending-obligation resolve worlds; peak resolved continuation {peak_close_cu} CU");
}

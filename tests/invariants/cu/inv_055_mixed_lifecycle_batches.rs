//! INV-055/065: one allowed sibling exit cannot admit a forbidden lifecycle leg.
//! All economic setup and transitions use System/SPL/ATA/public wrapper instructions.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PRINCIPAL: u128 = 1_000;
const EXIT_Q: u128 = 3 * POS_SCALE;
const RESET_Q: u128 = 2 * POS_SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdmissionState {
    Active,
    DrainOnly,
    ResetPending,
    Recovery,
    Retired,
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 4],
    portfolios: Vec<Pubkey>,
    tokens: Vec<Pubkey>,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    peak_cu: u64,
    accepted_batches: usize,
    rejected_batches: usize,
}

impl World {
    fn new(state: AdmissionState, sign: i128, cpi: bool) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        }
        env.configure_permissionless_resolve_with_cu(100, 5);
        let owners = std::array::from_fn(|_| Keypair::new());
        let mut portfolios = Vec::new();
        let mut tokens = Vec::new();
        for owner in &owners {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&env.payer.pubkey(), &owner.pubkey(), 1_000_000),
                &[],
            )
            .expect("public owner lamport funding");
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
                &[owner],
            )
            .expect("initialize System-created portfolio");
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
                    PRINCIPAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .expect("finite SPL endowment");
            env.send(
                env.deposit_ix(portfolio.pubkey(), PRINCIPAL),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                    AccountMeta::new(token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .expect("deposit real SPL principal");
            portfolios.push(portfolio.pubkey());
            tokens.push(token);
        }
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
        .expect("revoke mint authority");

        let mut world = Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher: None,
            peak_cu: 0,
            accepted_batches: 0,
            rejected_batches: 0,
        };
        world.seed_trade(0, 1, 0, sign * EXIT_Q as i128);
        world.seed_trade(2, 3, 1, sign * RESET_Q as i128);
        if matches!(
            state,
            AdmissionState::ResetPending | AdmissionState::Recovery
        ) {
            world
                .env
                .rebalance_reduce_with_cu(&world.owners[2], world.portfolios[2], 1, RESET_Q);
        } else {
            world.seed_trade(2, 3, 1, -sign * RESET_Q as i128);
        }
        match state {
            AdmissionState::DrainOnly => {
                world.lifecycle(processor::ASSET_ACTION_DRAIN_ONLY, 1, 0);
            }
            AdmissionState::Recovery => {
                world.lifecycle(processor::ASSET_ACTION_SHUTDOWN, 1, 1);
            }
            AdmissionState::Retired => {
                world.lifecycle(processor::ASSET_ACTION_RETIRE, 1, 1);
            }
            _ => {}
        }
        world.lifecycle(processor::ASSET_ACTION_DRAIN_ONLY, 0, 0);
        if cpi {
            let program = Pubkey::new_unique();
            world.env.svm.add_program(
                program,
                &std::fs::read(auth_matcher_program_path()).expect("fresh auth matcher SBF"),
            );
            let (context, delegate, _) = world.env.init_auth_matcher_context_via_system_create(
                program,
                &world.owners[1],
                world.portfolios[1],
            );
            world.matcher = Some((program, context, delegate));
        }
        world.assert_start(state, sign);
        world.assert_principal();
        world
    }

    fn seed_trade(&mut self, first: usize, second: usize, asset: u16, size: i128) {
        self.env.trade_asset_with_cu(
            asset,
            &self.owners[first],
            self.portfolios[first],
            &self.owners[second],
            self.portfolios[second],
            size,
            100,
            0,
        );
    }

    fn lifecycle(&mut self, action: u8, asset: u16, slot: u64) {
        self.env
            .update_asset_lifecycle_as_admin_with_cu(action, asset, slot, 0);
    }

    fn assert_start(&self, state: AdmissionState, sign: i128) {
        let group = self.env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.assets[0].lifecycle, AssetLifecycleV16::DrainOnly);
        assert_eq!(group.assets[0].oi_eff_long_q, EXIT_Q);
        assert_eq!(group.assets[0].oi_eff_short_q, EXIT_Q);
        self.assert_position(0, 0, sign * EXIT_Q as i128);
        self.assert_position(1, 0, -sign * EXIT_Q as i128);
        self.assert_position(0, 1, 0);
        self.assert_position(1, 1, 0);
        let target = group.assets[1];
        assert_eq!(
            target.lifecycle,
            match state {
                AdmissionState::Active | AdmissionState::ResetPending => AssetLifecycleV16::Active,
                AdmissionState::DrainOnly => AssetLifecycleV16::DrainOnly,
                AdmissionState::Recovery => AssetLifecycleV16::Recovery,
                AdmissionState::Retired => AssetLifecycleV16::Retired,
            }
        );
        assert_eq!(target.oi_eff_long_q, 0);
        assert_eq!(target.oi_eff_short_q, 0);
        if state == AdmissionState::Active {
            assert_eq!(target.mode_long, SideModeV16::Normal);
            assert_eq!(target.mode_short, SideModeV16::Normal);
        }
        let pending = matches!(
            state,
            AdmissionState::ResetPending | AdmissionState::Recovery
        );
        let (mode, count) = if sign > 0 {
            (target.mode_short, target.stored_pos_count_short)
        } else {
            (target.mode_long, target.stored_pos_count_long)
        };
        if pending {
            assert_eq!(mode, SideModeV16::ResetPending);
            assert_eq!(count, 1);
            let retained = self.env.portfolio_state(self.portfolios[3]);
            assert_eq!(
                active_leg_for_asset(&retained, 1).basis_pos_q,
                -sign * RESET_Q as i128
            );
            assert_eq!(
                active_leg_for_asset(&retained, 1).epoch_snap.checked_add(1),
                Some(if sign > 0 {
                    target.epoch_short
                } else {
                    target.epoch_long
                }),
                "the stored leg belongs to the prior epoch, not fresh effective risk"
            );
        } else {
            assert_eq!(target.stored_pos_count_long, 0);
            assert_eq!(target.stored_pos_count_short, 0);
        }
    }

    fn assert_position(&self, actor: usize, asset: usize, expected: i128) {
        let account = self.env.portfolio_state(self.portfolios[actor]);
        if expected == 0 {
            assert!(!has_active_leg_for_asset(&account, asset));
        } else {
            let leg = active_leg_for_asset(&account, asset);
            assert_eq!(leg.basis_pos_q, expected);
            assert_eq!(
                reference_current_epoch_effective_abs(&self.env.market_state().1, leg),
                expected.unsigned_abs()
            );
        }
    }

    fn assert_principal(&self) {
        let group = self.env.market_state().1;
        let mut capital = 0;
        let mut paid = 0;
        for actor in 0..4 {
            let account = self.env.portfolio_state(self.portfolios[actor]);
            let wallet = u128::from(self.env.token_amount(self.tokens[actor]));
            assert_eq!(account.capital.get() + wallet, PRINCIPAL);
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(account.reserved_pnl.get(), 0);
            capital += account.capital.get();
            paid += wallet;
        }
        assert_eq!(group.c_tot, capital);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault, capital);
        assert_eq!(
            group.vault,
            u128::from(self.env.token_amount(self.env.vault))
        );
        assert_eq!(group.vault + paid, 4 * PRINCIPAL);
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), 4 * PRINCIPAL);
        assert_eq!(mint.mint_authority, COption::None);
    }

    fn batch(&self, legs: &[(u16, i128)]) -> Instruction {
        let a = self.portfolios[0];
        let b = self.portfolios[1];
        let (instruction, accounts) = if let Some((program, context, delegate)) = self.matcher {
            (
                self.env.batch_trade_cpi_ix_with_caps(
                    a,
                    b,
                    legs.iter()
                        .map(|&(asset_index, size_q)| BatchTradeCpiLeg {
                            asset_index,
                            market_id: self.env.asset_market_id(asset_index),
                            size_q,
                            fee_bps: 0,
                            limit_price: 100,
                        })
                        .collect(),
                    0,
                    0,
                ),
                vec![
                    AccountMeta::new(self.owners[0].pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                    AccountMeta::new_readonly(program, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
            )
        } else {
            (
                self.env.batch_trade_no_cpi_ix(
                    a,
                    b,
                    legs.iter()
                        .map(|&(asset_index, size_q)| BatchTradeLeg {
                            asset_index,
                            market_id: self.env.asset_market_id(asset_index),
                            size_q,
                            exec_price: 100,
                            fee_bps: 0,
                        })
                        .collect(),
                ),
                vec![
                    AccountMeta::new(self.owners[0].pubkey(), true),
                    AccountMeta::new(self.owners[1].pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(a, false),
                    AccountMeta::new(b, false),
                ],
            )
        };
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: instruction.encode(),
        }
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        let mut keys = vec![
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.admin.pubkey(),
            self.env.vault_authority,
        ];
        keys.extend(self.owners.iter().map(Signer::pubkey));
        keys.extend(&self.portfolios);
        keys.extend(&self.tokens);
        if let Some((program, context, delegate)) = self.matcher {
            keys.extend([program, context, delegate]);
        }
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    fn assert_frame(&self, frame: &[(Pubkey, Option<Account>)]) {
        for (key, account) in frame {
            assert_eq!(
                self.env.svm.get_account(key),
                *account,
                "account frame {key}"
            );
        }
    }

    fn execute_batch(&mut self, instruction: Instruction, reject: bool) {
        self.env.svm.expire_blockhash();
        let mut signers = vec![&self.env.payer, &self.owners[0]];
        if self.matcher.is_none() {
            signers.push(&self.owners[1]);
        }
        let transaction = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), instruction],
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        let mut frame = self.frame();
        for key in &transaction.message.account_keys {
            if *key != self.env.payer.pubkey() && !frame.iter().any(|(tracked, _)| tracked == key) {
                frame.push((*key, self.env.svm.get_account(key)));
            }
        }
        let mut payer = self.env.svm.get_account(&self.env.payer.pubkey()).unwrap();
        payer.lamports -= u64::from(transaction.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let result = self.env.svm.send_transaction(transaction);
        let cu = if reject {
            let failed =
                result.expect_err("restricted target cannot ride along with an allowed exit");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                ),
                "must reach the lifecycle admission gate: {:?}",
                failed.meta.logs
            );
            self.assert_frame(&frame);
            self.rejected_batches += 1;
            failed.meta.compute_units_consumed
        } else {
            let accepted = result.expect("admitted public batch must remain live");
            self.accepted_batches += 1;
            accepted.compute_units_consumed
        };
        assert_eq!(
            self.env.svm.get_account(&self.env.payer.pubkey()).unwrap(),
            payer
        );
        assert_cu_within(
            "INV-055 mixed lifecycle batch",
            cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
        self.peak_cu = self.peak_cu.max(cu);
        self.assert_principal();
    }

    fn cleanup_reset(&mut self, state: AdmissionState, sign: i128) {
        let sibling = self.env.market_state().1.assets[0];
        let generation = self.env.asset_market_id(1);
        let peers = self
            .frame()
            .into_iter()
            .filter(|(key, _)| *key != self.env.market && *key != self.portfolios[3])
            .collect::<Vec<_>>();
        let cu = self
            .env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(self.env.payer.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.portfolios[3], false),
                ],
                &[],
            )
            .expect("one permissionless call must detach the prior-epoch leg");
        assert_cu_within("INV-065 reset detach", cu, CRANK_CU_LIMIT);
        self.peak_cu = self.peak_cu.max(cu);
        self.assert_frame(&peers);
        self.assert_position(3, 1, 0);
        assert_eq!(self.env.market_state().1.assets[0], sibling);
        assert_eq!(self.env.asset_market_id(1), generation);
        let cleaned = self.env.market_state().1.assets[1];
        assert_eq!(cleaned.stored_pos_count_long, 0);
        assert_eq!(cleaned.stored_pos_count_short, 0);
        let side = if sign > 0 { 1 } else { 0 };
        assert_eq!(
            if side == 1 {
                cleaned.mode_short
            } else {
                cleaned.mode_long
            },
            SideModeV16::ResetPending
        );
        let cu = self.env.finalize_reset_side_with_cu(1, side);
        assert_cu_within("INV-065 reset finalization", cu, CUSTODY_CU_LIMIT);
        self.peak_cu = self.peak_cu.max(cu);
        self.assert_frame(&peers);
        let group = self.env.market_state().1;
        assert_eq!(self.env.asset_market_id(1), generation);
        assert_eq!(group.assets[0], sibling);
        assert_eq!(group.assets[1].mode_long, SideModeV16::Normal);
        assert_eq!(group.assets[1].mode_short, SideModeV16::Normal);
        assert_eq!(
            group.assets[1].lifecycle,
            if state == AdmissionState::Recovery {
                AssetLifecycleV16::Recovery
            } else {
                AssetLifecycleV16::Active
            }
        );
        self.assert_principal();
    }

    fn withdraw_all(&mut self) {
        for actor in 0..4 {
            for asset in 0..2 {
                self.assert_position(actor, asset, 0);
            }
            self.env.svm.expire_blockhash();
            let cu = self
                .env
                .send(
                    self.env.withdraw_ix(self.portfolios[actor], PRINCIPAL),
                    vec![
                        AccountMeta::new(self.owners[actor].pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(self.portfolios[actor], false),
                        AccountMeta::new(self.tokens[actor], false),
                        AccountMeta::new(self.env.vault, false),
                        AccountMeta::new_readonly(self.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&self.owners[actor]],
                )
                .expect("lifecycle isolation preserves every owner's principal exit");
            assert_cu_within(
                "INV-055/065 final principal withdrawal",
                cu,
                CUSTODY_CU_LIMIT,
            );
            self.peak_cu = self.peak_cu.max(cu);
            assert_eq!(self.env.token_amount(self.tokens[actor]), PRINCIPAL as u64);
            self.assert_principal();
        }
        assert_eq!(self.env.market_state().1.vault, 0);
    }
}

#[test]
fn v16_program_mixed_lifecycle_batches_reject_only_forbidden_risk_and_keep_exit_live() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    let mut batches = (0, 0);
    for state in [
        AdmissionState::Active,
        AdmissionState::DrainOnly,
        AdmissionState::ResetPending,
        AdmissionState::Recovery,
        AdmissionState::Retired,
    ] {
        for cpi in [false, true] {
            for sign in [-1, 1] {
                for exit_first in [false, true] {
                    eprintln!("mixed lifecycle state={state:?} cpi={cpi} sign={sign} exit_first={exit_first}");
                    let mut world = World::new(state, sign, cpi);
                    let exit = (0, -sign * EXIT_Q as i128);
                    let open = (1, -sign * POS_SCALE as i128);
                    let legs = if exit_first {
                        [exit, open]
                    } else {
                        [open, exit]
                    };
                    // Bind all three payloads before the first submission, including owner episodes.
                    let mixed = world.batch(&legs);
                    let retained_exit = world.batch(&[exit]);
                    let standalone_open = world.batch(&[open]);
                    if state == AdmissionState::Active {
                        world.execute_batch(mixed, false);
                        world.assert_position(0, 0, 0);
                        world.assert_position(1, 0, 0);
                        world.assert_position(0, 1, open.1);
                        world.assert_position(1, 1, -open.1);
                        assert_eq!(
                            world.env.market_state().1.assets[1].oi_eff_long_q,
                            POS_SCALE
                        );
                        assert_eq!(
                            world.env.market_state().1.assets[1].oi_eff_short_q,
                            POS_SCALE
                        );
                        world.execute_batch(world.batch(&[(1, -open.1)]), false);
                    } else {
                        world.execute_batch(standalone_open, true);
                        world.execute_batch(mixed, true);
                        world.assert_start(state, sign);
                        let target = world.env.market_state().1.assets[1];
                        let profile = state::read_asset_oracle_profile(
                            &world.env.svm.get_account(&world.env.market).unwrap().data,
                            1,
                        )
                        .unwrap();
                        let unrelated = world
                            .frame()
                            .into_iter()
                            .filter(|(key, _)| {
                                *key != world.env.market
                                    && *key != world.portfolios[0]
                                    && *key != world.portfolios[1]
                                    && world.matcher.is_none_or(|(_, context, _)| *key != context)
                            })
                            .collect::<Vec<_>>();
                        world.execute_batch(retained_exit, false);
                        world.assert_position(0, 0, 0);
                        world.assert_position(1, 0, 0);
                        assert_eq!(world.env.market_state().1.assets[0].oi_eff_long_q, 0);
                        assert_eq!(world.env.market_state().1.assets[0].oi_eff_short_q, 0);
                        world.assert_frame(&unrelated);
                        assert_eq!(world.env.market_state().1.assets[1], target);
                        assert_eq!(
                            state::read_asset_oracle_profile(
                                &world.env.svm.get_account(&world.env.market).unwrap().data,
                                1
                            )
                            .unwrap(),
                            profile
                        );
                        world.assert_position(0, 1, 0);
                        world.assert_position(1, 1, 0);
                        if matches!(
                            state,
                            AdmissionState::ResetPending | AdmissionState::Recovery
                        ) {
                            world.cleanup_reset(state, sign);
                            let open_after_cleanup = world.batch(&[open]);
                            let still_restricted = state == AdmissionState::Recovery;
                            world.execute_batch(open_after_cleanup, still_restricted);
                            if !still_restricted {
                                world.assert_position(0, 1, open.1);
                                world.assert_position(1, 1, -open.1);
                                world.execute_batch(world.batch(&[(1, -open.1)]), false);
                            }
                        }
                    }
                    world.assert_position(0, 0, 0);
                    world.assert_position(1, 0, 0);
                    let group = world.env.market_state().1;
                    for asset in &group.assets[..2] {
                        assert_eq!(asset.oi_eff_long_q, 0);
                        assert_eq!(asset.oi_eff_short_q, 0);
                        assert_eq!(asset.stored_pos_count_long, 0);
                        assert_eq!(asset.stored_pos_count_short, 0);
                    }
                    world.withdraw_all();
                    peak_cu = peak_cu.max(world.peak_cu);
                    batches.0 += world.accepted_batches;
                    batches.1 += world.rejected_batches;
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 40);
    assert_eq!(batches, (64, 72));
    eprintln!("INV-055/065 mixed lifecycle matrix: {worlds} worlds, {} accepted / {} rejected batches, peak CU={peak_cu}", batches.0, batches.1);
}

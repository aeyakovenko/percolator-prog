//! INV-045 / row 425: unilateral owner reduction preserves canonical price carry.
//! Unlike bilateral carry selectors, RebalanceReduce scales the absent peer's ADL
//! exposure. A half reduction before the first price atom must preserve the stored
//! fraction and unprocessed time, and only the remaining lots earn later movement.
//! Both sides, price directions and reduction/crank orders use public SBF routes.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ENTRY: u64 = 100;
const CAP_BPS: u64 = 24;
const PRINCIPAL: [u64; 2] = [100_003, 200_009];
const TOTAL: u64 = PRINCIPAL[0] + PRINCIPAL[1];

struct World {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    direction: i128,
    peak_cu: u64,
    rejections: usize,
}

impl World {
    fn new(direction: i128) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                initial_price: ENTRY,
                max_price_move_bps_per_slot: CAP_BPS,
                max_abs_funding_e9_per_slot: 0,
                max_accrual_dt_slots: 6,
                min_funding_lifetime_slots: 6,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(0);
        env.configure_auth_mark_with_cu(0, ENTRY);
        let owners = [Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 2];
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(
                    &env.payer.pubkey(),
                    &owners[actor].pubkey(),
                    1_000_000,
                ),
                &[],
            )
            .unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[actor] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolios[actor]);
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    PRINCIPAL[actor],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], PRINCIPAL[actor] as u128),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
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
        .unwrap();
        env.trade_with_cu(
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            8 * POS_SCALE as i128,
            ENTRY,
            0,
        );
        env.push_auth_mark_with_cu(0, (ENTRY as i128 + direction * 20) as u64);
        Self {
            env,
            owners,
            portfolios,
            tokens,
            direction,
            peak_cu: 0,
            rejections: 0,
        }
    }

    fn frame(&self) -> Vec<Option<Account>> {
        [
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.portfolios[0],
            self.portfolios[1],
            self.tokens[0],
            self.tokens[1],
            self.owners[0].pubkey(),
            self.owners[1].pubkey(),
            self.env.admin.pubkey(),
            self.env.payer.pubkey(),
        ]
        .map(|key| self.env.svm.get_account(&key))
        .to_vec()
    }

    fn crank_ix(&self, actor: usize, duplicate: bool) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.env.payer.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
            ],
            data: ProgInstruction::PermissionlessCrank {
                now_slot: self.env.svm.get_sysvar::<Clock>().slot,
                observations: if duplicate {
                    crank_observations_for_assets(&[0, 0])
                } else {
                    crank_observations(0)
                },
            }
            .encode(),
        }
    }

    fn reduce_ix(&self, actor: usize) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(self.owners[actor].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
            ],
            data: ProgInstruction::RebalanceReduce {
                portfolio_id: self.env.portfolio_id(self.portfolios[actor]),
                position_epoch: self.env.portfolio_position_epoch(self.portfolios[actor]),
                asset_index: 0,
                reduce_q: 4 * POS_SCALE,
            }
            .encode(),
        }
    }

    #[track_caller]
    fn send(
        &mut self,
        ixs: &[Instruction],
        actor: Option<usize>,
        expected_error: Option<(u8, PercolatorError)>,
    ) {
        self.env.svm.expire_blockhash();
        let mut before = self.frame();
        let mut signers = vec![&self.env.payer];
        if let Some(actor) = actor {
            signers.push(&self.owners[actor]);
        }
        let tx = Transaction::new_signed_with_payer(
            &[
                vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
                ],
                ixs.to_vec(),
            ]
            .concat(),
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        match expected_error {
            Some((index, error)) => {
                // Only the runtime signature charge is outside economic rollback.
                before.last_mut().unwrap().as_mut().unwrap().lamports -=
                    tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
                let failed = self.env.svm.send_transaction(tx).unwrap_err();
                assert_eq!(
                    failed.err,
                    TransactionError::InstructionError(
                        index + 1,
                        InstructionError::Custom(error as u32)
                    ),
                    "{:?}",
                    failed.meta.logs
                );
                assert_eq!(
                    self.frame(),
                    before,
                    "entire rejected public transaction rolls back"
                );
                self.rejections += 1;
            }
            None => {
                let meta = self.env.svm.send_transaction(tx).unwrap();
                self.peak_cu = self.peak_cu.max(meta.compute_units_consumed);
            }
        }
    }

    fn assert_cap(&self, slot: u64, lots: u128) {
        let group = self.env.market_state().1;
        let asset = group.assets[0];
        let profile = state::read_asset_oracle_profile(
            &self.env.svm.get_account(&self.env.market).unwrap().data,
            0,
        )
        .unwrap();
        let numerator = ENTRY * CAP_BPS * slot;
        assert_eq!(asset.slot_last, slot);
        assert_eq!(
            asset.effective_price as i128,
            ENTRY as i128 + self.direction * i128::from(numerator / 10_000)
        );
        assert_eq!(
            u64::from(profile.price_move_remainder_bps_num),
            numerator % 10_000
        );
        assert_eq!(asset.fund_px_last, ENTRY);
        assert_eq!(
            asset.raw_oracle_target_price as i128,
            ENTRY as i128 + self.direction * 20
        );
        assert_eq!(
            (asset.oi_eff_long_q, asset.oi_eff_short_q),
            (lots * POS_SCALE, lots * POS_SCALE)
        );
        assert_eq!(
            (
                asset.f_long_num,
                asset.f_short_num,
                asset.b_long_num,
                asset.b_short_num
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault, TOTAL as u128);
        assert_eq!(self.env.token_amount(self.env.vault), TOTAL);
        assert_eq!(self.tokens.map(|key| self.env.token_amount(key)), [0, 0]);
    }
}

#[test]
fn v16_program_unilateral_reduction_preserves_fractional_cap_and_peer_entitlement() {
    let mut worlds = 0;
    let mut rejections = 0;
    let mut peak_cu = 0;
    for direction in [-1, 1] {
        let mut outcomes = Vec::new();
        for actor in 0..2 {
            for reduce_first in [false, true] {
                let mut world = World::new(direction);
                eprintln!("INV-045 unilateral carry: direction={direction}, actor={actor}, reduce_first={reduce_first}");
                world.assert_cap(0, 8);
                world.env.svm.warp_to_slot(1);
                world.send(&[world.crank_ix(1 - actor, false)], None, None);
                world.assert_cap(1, 8);
                world.env.svm.warp_to_slot(4);
                if !reduce_first {
                    world.send(&[world.crank_ix(actor, false)], None, None);
                    world.assert_cap(4, 8);
                }
                let reduction = world.reduce_ix(actor);
                let mut unsigned = reduction.clone();
                unsigned.accounts[0].is_signer = false;
                world.send(
                    &[unsigned],
                    None,
                    Some((1, PercolatorError::ExpectedSigner)),
                );
                let mut readonly = reduction.clone();
                readonly.accounts[2].is_writable = false;
                world.send(
                    &[readonly],
                    Some(actor),
                    Some((1, PercolatorError::ExpectedWritable)),
                );
                world.send(
                    &[reduction.clone(), world.crank_ix(1 - actor, true)],
                    Some(actor),
                    Some((2, PercolatorError::InvalidInstruction)),
                );

                let peer_before = world.env.svm.get_account(&world.portfolios[1 - actor]);
                world.send(&[reduction], Some(actor), None);
                assert_eq!(
                    world.env.svm.get_account(&world.portfolios[1 - actor]),
                    peer_before,
                    "unilateral reduction never writes the absent peer"
                );
                let asset = world.env.market_state().1.assets[0];
                assert_eq!([asset.a_long, asset.a_short][actor], ADL_ONE);
                assert_eq!([asset.a_long, asset.a_short][1 - actor], ADL_ONE / 2);
                let slot = if reduce_first { 1 } else { 4 };
                world.assert_cap(slot, 4);
                let profile = state::read_asset_oracle_profile(
                    &world.env.svm.get_account(&world.env.market).unwrap().data,
                    0,
                )
                .unwrap();
                assert_eq!(
                    u64::from(profile.price_move_remainder_bps_num) + ENTRY * CAP_BPS * (4 - slot),
                    9_600,
                    "stored carry plus due capacity survives ADL"
                );
                // With no new price atom, ADL alone need not write the peer. Only
                // the reduction-first schedule still has canonical time to commit.
                world.send(
                    &[world.crank_ix(1 - actor, false)],
                    None,
                    (!reduce_first).then_some((1, PercolatorError::EngineNonProgress)),
                );
                world.assert_cap(4, 4);

                world.env.svm.warp_to_slot(6);
                let catchup = world.crank_ix(actor, false);
                world.send(
                    &[catchup.clone(), world.crank_ix(1 - actor, true)],
                    None,
                    Some((2, PercolatorError::InvalidInstruction)),
                );
                world.assert_cap(4, 4);
                world.send(&[catchup], None, None);
                world.assert_cap(6, 4);
                world.send(&[world.crank_ix(1 - actor, false)], None, None);
                world.assert_cap(6, 4);
                let expected = [
                    PRINCIPAL[0] as i128 + 4 * direction,
                    PRINCIPAL[1] as i128 - 4 * direction,
                ];
                for owner in 0..2 {
                    let account = world.env.portfolio_state(world.portfolios[owner]);
                    assert_eq!(
                        account.capital.get() as i128 + account.pnl.get(),
                        expected[owner],
                        "only the remaining four lots earn the first price atom"
                    );
                    let leg = active_leg_for_asset(&account, 0);
                    assert_eq!(
                        reference_current_epoch_effective_abs(&world.env.market_state().1, leg),
                        4 * POS_SCALE
                    );
                }
                let env = &world.env;
                let resolve = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                    data: ProgInstruction::ResolveMarket {
                        asset_generation_frontier: env.market_state().1.next_market_id,
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    }
                    .encode(),
                };
                let cu = send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    resolve,
                    &[&world.env.admin],
                )
                .unwrap();
                world.peak_cu = world.peak_cu.max(cu);
                world.env.svm.warp_to_slot(100);
                for _ in 0..16 {
                    if world
                        .portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&world.env, *key))
                    {
                        break;
                    }
                    for owner in [actor, 1 - actor] {
                        if resolved_portfolio_is_terminal(&world.env, world.portfolios[owner]) {
                            continue;
                        }
                        let env = &world.env;
                        let close = Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new_readonly(world.owners[owner].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(world.portfolios[owner], false),
                                AccountMeta::new(world.tokens[owner], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            data: ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            }
                            .encode(),
                        };
                        world.send(&[close], Some(owner), None);
                        let group = world.env.market_state().1;
                        let profile = state::read_asset_oracle_profile(
                            &world.env.svm.get_account(&world.env.market).unwrap().data,
                            0,
                        )
                        .unwrap();
                        assert_eq!(
                            (group.resolved_slot, group.assets[0].effective_price),
                            (6, (ENTRY as i128 + direction) as u64)
                        );
                        assert_eq!(profile.price_move_remainder_bps_num, 4_400);
                        let paid = world.tokens.map(|key| world.env.token_amount(key));
                        assert_eq!(group.vault, world.env.token_amount(world.env.vault) as u128);
                        assert_eq!(
                            group.vault + paid.iter().map(|&n| n as u128).sum::<u128>(),
                            TOTAL as u128
                        );
                        for owner in 0..2 {
                            assert!(paid[owner] as i128 <= expected[owner]);
                        }
                    }
                }
                assert!(world
                    .portfolios
                    .iter()
                    .all(|key| resolved_portfolio_is_terminal(&world.env, *key)));
                let paid = world.tokens.map(|key| world.env.token_amount(key));
                assert_eq!(paid, expected.map(|n| n as u64));
                let group = world.env.market_state().1;
                assert_eq!(
                    (group.vault, group.c_tot, group.pnl_pos_tot, group.insurance),
                    (0, 0, 0, 0)
                );
                assert_eq!(
                    (
                        group.assets[0].oi_eff_long_q,
                        group.assets[0].oi_eff_short_q
                    ),
                    (0, 0)
                );
                let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                    .unwrap();
                assert_eq!((mint.supply, mint.mint_authority), (TOTAL, COption::None));
                outcomes.push(paid);
                worlds += 1;
                rejections += world.rejections;
                peak_cu = peak_cu.max(world.peak_cu);
            }
        }
        assert!(
            outcomes.windows(2).all(|pair| pair[0] == pair[1]),
            "ADL side and accrual placement preserve each owner's payout"
        );
    }
    assert_eq!((worlds, rejections), (8, 36));
    assert_cu_within(
        "unilateral fractional cap and peer entitlement",
        peak_cu,
        1_400_000,
    );
    eprintln!("INV-045 unilateral carry: {worlds} histories, {rejections} exact rollbacks, 16 owner payouts, peak {peak_cu} CU");
}

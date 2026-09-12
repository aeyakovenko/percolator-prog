//! INV-012 / reopening 414: jointly replaced assets and matcher grants require
//! the conjunction of their current identities, independent of replacement order.
//! INV-002 owns isolated/mixed asset reuse; this product adds same-tuple regrant
//! and every proper subset of repaired bindings. All state is publicly constructed.
//! This bounded composition does not close arbitrary economic-object histories.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

#[path = "inv_012_matcher_program_generation.rs"]
mod matcher_program_generation;

#[path = "inv_012_used_generation_lifecycle.rs"]
mod used_generation_lifecycle;

#[path = "inv_012_retained_scope_product.rs"]
mod retained_scope_product;

#[path = "inv_012_revocation_atomicity.rs"]
mod revocation_atomicity;

#[path = "inv_012_generation_bundle_rollback.rs"]
mod generation_bundle_rollback;

#[path = "inv_012_portfolio_grant_rollback.rs"]
mod portfolio_grant_rollback;

#[path = "inv_012_market_retirement_rollback.rs"]
mod market_retirement_rollback;

#[path = "inv_012_funded_owner_roundtrip.rs"]
mod funded_owner_roundtrip;

const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const SLOT: u64 = 1;
const EXPIRY: u64 = 100;
const FEE_CAP: u16 = 37;
const GRANT: u8 = 4;
const ORDERS: [[u8; 3]; 6] = [
    [1, 2, GRANT],
    [1, GRANT, 2],
    [2, 1, GRANT],
    [2, GRANT, 1],
    [GRANT, 1, 2],
    [GRANT, 2, 1],
];

#[derive(Clone, Copy, Debug)]
enum Route {
    Single(u16),
    Batch,
}

impl Route {
    fn scope(self) -> u8 {
        match self {
            Self::Single(asset) => GRANT | (1 << (asset - 1)),
            Self::Batch => GRANT | 3,
        }
    }
}

const ROUTES: [Route; 3] = [Route::Single(1), Route::Single(2), Route::Batch];

#[derive(Default, Debug)]
struct Evidence {
    worlds: usize,
    live_simulations: usize,
    rejected_simulations: usize,
    rejections: usize,
    fills: usize,
    writer_cu: u64,
    rejection_cu: u64,
    fill_cu: u64,
    custody_cu: u64,
}

struct History {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    matcher: (Pubkey, Pubkey, Pubkey),
    ids: [u64; 3],
    next_id: u64,
    grant_sequence: u64,
    positions: [i128; 3],
    epoch: u64,
    slot: u64,
}

impl History {
    fn new() -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 3,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(SLOT);
        for asset in 0..3 {
            env.configure_auth_mark_for_asset_as_admin(asset, SLOT, PRICE);
        }
        let owners = [Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 2];
        let mut tokens = [Pubkey::default(); 2];
        for i in 0..2 {
            env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[i] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[&owners[i]],
            )
            .expect("initialize System-created portfolio");
            env.portfolios.push(portfolios[i]);
            tokens[i] = create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[i],
                    &env.admin.pubkey(),
                    &[],
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .expect("SPL-mint owner collateral");
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .expect("public collateral deposit");
        }
        let matcher = auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
        env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
            matcher.0,
            &owners[1],
            portfolios[1],
            matcher.1,
            matcher.2,
            1,
            FEE_CAP,
            EXPIRY,
        )
        .expect("install bounded owner grant before retaining requests");
        let grant_sequence = env.portfolio_matcher_sequence(portfolios[1]);
        let history = Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            ids: [1, 2, 3],
            next_id: 4,
            grant_sequence,
            positions: [0; 3],
            epoch: 0,
            slot: SLOT,
        };
        history.assert_state();
        history
    }

    fn frame(&self) -> Vec<Option<Account>> {
        // Only the independent network fee payer is excluded, not owner/rent balances.
        [
            self.env.market,
            self.portfolios[0],
            self.portfolios[1],
            self.tokens[0],
            self.tokens[1],
            self.env.vault,
            self.env.mint,
            self.matcher.1,
            self.matcher.2,
            self.owners[0].pubkey(),
            self.owners[1].pubkey(),
            self.env.admin.pubkey(),
        ]
        .map(|key| self.env.svm.get_account(&key))
        .to_vec()
    }

    fn assert_state(&self) {
        let group = self.env.market_state().1;
        assert_eq!(group.next_market_id, self.next_id);
        for asset in 0..3 {
            assert_eq!(group.assets[asset].market_id, self.ids[asset]);
            assert_eq!(
                group.assets[asset].oi_eff_long_q,
                self.positions[asset].unsigned_abs()
            );
            assert_eq!(
                group.assets[asset].oi_eff_short_q,
                self.positions[asset].unsigned_abs()
            );
        }
        for (actor, portfolio) in self.portfolios.iter().enumerate() {
            let state = self.env.portfolio_state(*portfolio);
            assert_eq!(state.capital.get(), CAPITAL);
            assert_eq!(state.pnl.get(), 0);
            assert_eq!(self.env.portfolio_position_epoch(*portfolio), self.epoch);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&state)) as usize,
                self.positions.iter().filter(|q| **q != 0).count()
            );
            for asset in 0..3 {
                if self.positions[asset] != 0 {
                    let leg = active_leg_for_asset(&state, asset);
                    assert_eq!(leg.market_id, self.ids[asset]);
                    assert_eq!(
                        leg.basis_pos_q,
                        self.positions[asset] * if actor == 0 { 1 } else { -1 }
                    );
                }
            }
            assert_eq!(self.env.token_amount(self.tokens[actor]), 0);
        }
        let config = self.env.portfolio_matcher_config(self.portfolios[1]);
        assert_eq!(config.enabled(), 1);
        assert_eq!(config.trade_fee_cap_bps(), FEE_CAP);
        assert_eq!(config.matcher_program, self.matcher.0.to_bytes());
        assert_eq!(config.matcher_context, self.matcher.1.to_bytes());
        assert_eq!(config.matcher_delegate, self.matcher.2.to_bytes());
        assert_eq!(
            self.env.portfolio_matcher_sequence(self.portfolios[1]),
            self.grant_sequence
        );
        assert_eq!(
            self.env.portfolio_matcher_expiry(self.portfolios[1]),
            EXPIRY
        );
        assert_eq!(self.env.svm.get_sysvar::<Clock>().slot, self.slot);
        assert!(self.slot < EXPIRY);
        assert_eq!(group.c_tot, 2 * CAPITAL);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault, 2 * CAPITAL);
        assert_eq!(self.env.token_amount(self.env.vault) as u128, 2 * CAPITAL);
        assert_eq!(
            Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            2 * CAPITAL
        );
    }

    fn instruction(&self, route: Route, sizes: [i128; 3], reverse: bool) -> ProgInstruction {
        match route {
            Route::Single(asset) => self.env.trade_cpi_ix(
                self.portfolios[0],
                self.portfolios[1],
                asset,
                sizes[asset as usize],
                0,
                PRICE,
            ),
            Route::Batch => {
                let mut legs: Vec<_> = (1..3)
                    .filter(|asset| sizes[*asset] != 0)
                    .map(|asset| BatchTradeCpiLeg {
                        asset_index: asset as u16,
                        market_id: self.ids[asset],
                        size_q: sizes[asset],
                        fee_bps: 0,
                        limit_price: PRICE,
                    })
                    .collect();
                if reverse {
                    legs.reverse();
                }
                self.env.batch_trade_cpi_ix_with_caps(
                    self.portfolios[0],
                    self.portfolios[1],
                    legs,
                    0,
                    0,
                )
            }
        }
    }

    fn sign(&self, ix: &ProgInstruction) -> Transaction {
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                Instruction {
                    program_id: self.env.program_id,
                    accounts: vec![
                        AccountMeta::new(self.owners[0].pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(self.portfolios[0], false),
                        AccountMeta::new(self.portfolios[1], false),
                        AccountMeta::new_readonly(self.matcher.0, false),
                        AccountMeta::new(self.matcher.1, false),
                        AccountMeta::new_readonly(self.matcher.2, false),
                    ],
                    data: ix.encode(),
                },
            ],
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer, &self.owners[0]],
            self.env.svm.latest_blockhash(),
        );
        assert_eq!(
            tx.signatures.len(),
            2,
            "no LP signature on capability consumption"
        );
        assert!(
            bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        tx
    }

    fn replace(&mut self, object: u8, evidence: &mut Evidence) {
        self.replace_without_mark_refresh(object, evidence);
        if object != GRANT {
            for asset in 0..3 {
                let cu = self
                    .env
                    .configure_auth_mark_for_asset_as_admin(asset, self.slot, PRICE);
                evidence.writer_cu = evidence.writer_cu.max(cu);
            }
        }
        self.assert_state();
    }

    fn replace_without_mark_refresh(&mut self, object: u8, evidence: &mut Evidence) {
        let cu = if object == GRANT {
            let cu = self
                .env
                .send(
                    ProgInstruction::SetMatcherConfig {
                        portfolio_id: self.env.portfolio_id(self.portfolios[1]),
                        expected_sequence: self.grant_sequence,
                        enabled: 1,
                        trade_fee_cap_bps: FEE_CAP,
                        expiry_slot: EXPIRY,
                    },
                    vec![
                        AccountMeta::new(self.owners[1].pubkey(), true),
                        AccountMeta::new_readonly(self.env.market, false),
                        AccountMeta::new(self.portfolios[1], false),
                        AccountMeta::new_readonly(self.matcher.0, false),
                        AccountMeta::new_readonly(self.matcher.1, false),
                        AccountMeta::new_readonly(self.matcher.2, false),
                    ],
                    &[&self.owners[1]],
                )
                .expect("same-tuple owner reauthorization");
            self.grant_sequence += 1;
            cu
        } else {
            let asset = object as usize;
            let mut max_cu = 0;
            for action in [
                percolator_prog::processor::ASSET_ACTION_RETIRE,
                percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            ] {
                let activate = action == percolator_prog::processor::ASSET_ACTION_ACTIVATE;
                if activate {
                    self.slot += 1;
                    self.env.svm.warp_to_slot(self.slot);
                }
                let ix = ProgInstruction::UpdateAssetLifecycle {
                    action,
                    asset_index: asset as u16,
                    market_id: if activate {
                        self.next_id
                    } else {
                        self.ids[asset]
                    },
                    authority_epoch: self.env.control_sequences(0).authority_epoch,
                    now_slot: self.slot,
                    initial_price: if activate { PRICE } else { 0 },
                    max_init_fee: 0,
                    insurance_authority: self.env.admin.pubkey().to_bytes(),
                    insurance_operator: self.env.admin.pubkey().to_bytes(),
                    backing_bucket_authority: self.env.admin.pubkey().to_bytes(),
                    oracle_authority: self.env.admin.pubkey().to_bytes(),
                };
                let cu = send_tx(
                    &mut self.env.svm,
                    self.env.program_id,
                    &self.env.payer,
                    ix,
                    vec![
                        AccountMeta::new(self.env.admin.pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                    ],
                    &[&self.env.admin],
                )
                .unwrap_or_else(|error| {
                    panic!("public asset {asset} lifecycle action {action}: {error}")
                });
                max_cu = max_cu.max(cu);
                if activate {
                    self.ids[asset] = self.next_id;
                    self.next_id += 1;
                }
                self.assert_state();
            }
            max_cu
        };
        evidence.writer_cu = evidence.writer_cu.max(cu);
        self.assert_state();
    }

    fn simulate(&mut self, tx: &Transaction, stale: u8, evidence: &mut Evidence) {
        let before = self.frame();
        let result = self.env.svm.simulate_transaction(tx.clone().into());
        if stale == 0 {
            let meta = result.expect("all bound objects remain current");
            assert!(meta
                .logs
                .iter()
                .any(|line| line == &format!("Program {} success", self.matcher.0)));
            evidence.live_simulations += 1;
            evidence.fill_cu = evidence.fill_cu.max(meta.compute_units_consumed);
        } else {
            let failed = result.expect_err("any stale bound object must reject");
            self.assert_rejection(&failed, stale);
            evidence.rejected_simulations += 1;
            evidence.rejection_cu = evidence
                .rejection_cu
                .max(failed.meta.compute_units_consumed);
        }
        assert_eq!(
            self.frame(),
            before,
            "simulation must not change any economic account"
        );
    }

    fn assert_rejection(&self, failed: &litesvm::types::FailedTransactionMetadata, stale: u8) {
        let error = if stale & 3 != 0 {
            PercolatorError::AssetGenerationMismatch
        } else {
            PercolatorError::EngineStale
        };
        assert_eq!(
            failed.err,
            solana_sdk::transaction::TransactionError::InstructionError(
                2,
                solana_sdk::instruction::InstructionError::Custom(error as u32)
            )
        );
        assert!(
            failed
                .meta
                .logs
                .iter()
                .all(|line| !line.starts_with(&format!("Program {} invoke", self.matcher.0))),
            "identity mismatch must reject before matcher CPI"
        );
    }

    fn fill(&mut self, route: Route, sizes: [i128; 3], reverse: bool, evidence: &mut Evidence) {
        let tx = self.sign(&self.instruction(route, sizes, reverse));
        let meta = self
            .env
            .svm
            .send_transaction(tx)
            .expect("fully current capability fills");
        assert!(meta
            .logs
            .iter()
            .any(|line| line == &format!("Program {} success", self.matcher.0)));
        match route {
            Route::Single(asset) => self.positions[asset as usize] += sizes[asset as usize],
            Route::Batch => {
                for asset in 1..3 {
                    self.positions[asset] += sizes[asset];
                }
            }
        }
        self.epoch += 1;
        evidence.fills += 1;
        evidence.fill_cu = evidence.fill_cu.max(meta.compute_units_consumed);
        self.assert_state();
    }

    fn withdraw_all(&mut self, reverse: bool, evidence: &mut Evidence) {
        assert_eq!(self.positions, [0; 3]);
        for actor in if reverse { [1, 0] } else { [0, 1] } {
            let other = self
                .env
                .svm
                .get_account(&self.portfolios[1 - actor])
                .unwrap();
            let cu = self
                .env
                .send(
                    self.env.withdraw_ix(self.portfolios[actor], CAPITAL),
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
                .expect("owner withdraws entire input-derived entitlement");
            evidence.custody_cu = evidence.custody_cu.max(cu);
            assert_eq!(
                self.env
                    .svm
                    .get_account(&self.portfolios[1 - actor])
                    .unwrap(),
                other
            );
            assert_eq!(
                self.env
                    .portfolio_state(self.portfolios[actor])
                    .capital
                    .get(),
                0
            );
            assert_eq!(self.env.token_amount(self.tokens[actor]) as u128, CAPITAL);
        }
        assert_eq!(self.env.market_state().1.c_tot, 0);
        assert_eq!(self.env.market_state().1.vault, 0);
        assert_eq!(self.env.token_amount(self.env.vault), 0);
    }
}

fn repair(ix: &ProgInstruction, history: &History, repaired: u8) -> ProgInstruction {
    let mut ix = ix.clone();
    match &mut ix {
        ProgInstruction::TradeCpi {
            account_b_matcher_sequence,
            asset_index,
            market_id,
            ..
        } => {
            if repaired & GRANT != 0 {
                *account_b_matcher_sequence = history.grant_sequence;
            }
            if repaired & (1 << (*asset_index - 1)) != 0 {
                *market_id = history.ids[*asset_index as usize];
            }
        }
        ProgInstruction::BatchTradeCpi {
            account_b_matcher_sequence,
            legs,
            ..
        } => {
            if repaired & GRANT != 0 {
                *account_b_matcher_sequence = history.grant_sequence;
            }
            for leg in legs {
                if repaired & (1 << (leg.asset_index - 1)) != 0 {
                    leg.market_id = history.ids[leg.asset_index as usize];
                }
            }
        }
        _ => unreachable!(),
    }
    ix
}

#[test]
fn v16_program_joint_replacements_require_every_bound_incarnation() {
    let mut evidence = Evidence::default();
    for (order_index, order) in ORDERS.into_iter().enumerate() {
        for sign in [-1, 1] {
            for reverse in [false, true] {
                let mut history = History::new();
                let sizes = [
                    0,
                    sign * 3 * POS_SCALE as i128,
                    -sign * 7 * POS_SCALE as i128,
                ];
                let originals = ROUTES.map(|route| history.instruction(route, sizes, reverse));
                let retained = originals.each_ref().map(|ix| history.sign(ix));
                let retained_hash = history.env.svm.latest_blockhash();
                let ids = history.portfolios.map(|p| history.env.portfolio_id(p));
                let config = history.env.portfolio_matcher_config(history.portfolios[1]);
                let context = history.env.svm.get_account(&history.matcher.1).unwrap();
                for tx in &retained {
                    history.simulate(tx, 0, &mut evidence);
                }

                let mut replaced = 0;
                for object in order {
                    history.replace(object, &mut evidence);
                    replaced |= object;
                    assert_eq!(history.env.svm.latest_blockhash(), retained_hash);
                    assert_eq!(history.portfolios.map(|p| history.env.portfolio_id(p)), ids);
                    assert_eq!(
                        history.env.portfolio_matcher_config(history.portfolios[1]),
                        config
                    );
                    assert_eq!(
                        history.env.svm.get_account(&history.matcher.1).unwrap(),
                        context
                    );
                    for (route, tx) in ROUTES.iter().zip(&retained) {
                        history.simulate(tx, route.scope() & replaced, &mut evidence);
                    }
                }

                for (index, route) in ROUTES.into_iter().enumerate() {
                    for repaired in 0..=7 {
                        if repaired & !route.scope() != 0 {
                            continue;
                        }
                        let ix = repair(&originals[index], &history, repaired);
                        let tx = if repaired == 0 {
                            retained[index].clone()
                        } else {
                            history.sign(&ix)
                        };
                        let stale = route.scope() & !repaired;
                        if stale == 0 {
                            assert_eq!(
                                ix.encode(),
                                history.instruction(route, sizes, reverse).encode(),
                                "only identity fields differ from the original signed intent"
                            );
                            history.simulate(&tx, 0, &mut evidence);
                        } else {
                            let before = history.frame();
                            let failed = history.env.svm.send_transaction(tx).expect_err(
                                "a proper subset of current bindings cannot authorize the request",
                            );
                            history.assert_rejection(&failed, stale);
                            assert_eq!(history.frame(), before, "exact repair-subset rollback, order={order:?}, route={route:?}, repaired={repaired}");
                            evidence.rejections += 1;
                            evidence.rejection_cu = evidence
                                .rejection_cu
                                .max(failed.meta.compute_units_consumed);
                            history.assert_state();
                        }
                    }
                }

                let entry = ROUTES[order_index % ROUTES.len()];
                history.fill(entry, sizes, reverse, &mut evidence);
                match entry {
                    Route::Batch => {
                        for asset in if reverse { [2, 1] } else { [1, 2] } {
                            history.fill(
                                Route::Single(asset),
                                sizes.map(|q| -q),
                                reverse,
                                &mut evidence,
                            );
                        }
                    }
                    Route::Single(_) => history.fill(
                        Route::Batch,
                        history.positions.map(|q| -q),
                        reverse,
                        &mut evidence,
                    ),
                }
                history.withdraw_all(reverse, &mut evidence);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 24);
    assert_eq!(evidence.live_simulations, 160);
    assert_eq!(evidence.rejected_simulations, 200);
    assert_eq!(evidence.rejections, 24 * (3 + 3 + 7));
    assert_eq!(evidence.fills, 56);
    assert_cu_within(
        "joint incarnation rejection preflight",
        evidence.rejection_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "joint incarnation fresh entry/exit",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "joint incarnation owner withdrawal",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-012 joint incarnation product: {evidence:?}");
}

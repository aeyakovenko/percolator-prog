//! INV-056: derive observation membership from accepted trade/mark inputs, not certificates.
//! A formerly complete roster becomes incomplete when a public close/open reuses a leg slot.
//! All accounts and marks are created by System/SPL/ATA/wrapper instructions; no byte injection,
//! snapshot restoration, engine refresh, or certificate-currentness oracle constructs the model.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const OPEN: u64 = 100;
const PEER_CAPITAL: u128 = 10_000;

struct ObservationBook {
    positions: [i128; 4],
    marks: [u64; 4],
    mark_slots: [u64; 4],
}

impl ObservationBook {
    fn required_assets(&self) -> Vec<u16> {
        self.positions
            .iter()
            .enumerate()
            .filter_map(|(asset, quantity)| (*quantity != 0).then_some(asset as u16))
            .collect()
    }

    fn pnl(&self) -> i128 {
        self.positions
            .iter()
            .zip(self.marks)
            .map(|(quantity, mark)| quantity * (i128::from(mark) - i128::from(OPEN)))
            .sum::<i128>()
            / POS_SCALE as i128
    }

    fn margin(&self, bps: u128) -> u128 {
        self.positions
            .iter()
            .zip(self.marks)
            .map(|(quantity, mark)| {
                let notional = (quantity.unsigned_abs() * u128::from(mark)).div_ceil(POS_SCALE);
                (notional * bps).div_ceil(10_000)
            })
            .sum()
    }
}

fn hints(assets: &[u16]) -> Vec<CrankObservationHint> {
    assets
        .iter()
        .map(|asset| CrankObservationHint {
            asset_index: *asset,
            oracle_accounts: 0,
        })
        .collect()
}

fn refresh_ix(env: &V16CuEnv, portfolio: Pubkey, assets: &[u16], now_slot: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot,
            observations: hints(assets),
        }
        .encode(),
    }
}

fn submit(
    env: &mut V16CuEnv,
    instructions: Vec<Instruction>,
    signers: &[&Keypair],
    tracked: &[Pubkey],
    expected_error: Option<(u8, PercolatorError)>,
    label: &str,
) -> u64 {
    env.svm.expire_blockhash();
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[vec![heap_ix(), cu_ix()], instructions].concat(),
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    );
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let frame: Vec<_> = keys
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let cu = if let Some((index, expected)) = expected_error {
        let error = result.expect_err(label);
        assert_eq!(
            error.err,
            TransactionError::InstructionError(index, InstructionError::Custom(expected as u32)),
            "{label}: {error:?}"
        );
        for (key, before) in frame {
            assert_eq!(env.svm.get_account(&key), before, "{label}: rollback {key}");
        }
        error.meta.compute_units_consumed
    } else {
        result
            .unwrap_or_else(|error| panic!("{label}: {error:?}"))
            .compute_units_consumed
    };
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer,
        "{label}: payer fee"
    );
    assert_cu_within(label, cu, 2 * CRANK_CU_LIMIT + TRADE_CU_LIMIT);
    cu
}

#[test]
fn v16_program_rotated_observation_roster_preserves_favorable_admission() {
    let mut peak_cu = 0;
    let mut rejected = 0;
    for direction in [1_i128, -1] {
        let mut canonical = None;
        for reverse in [false, true] {
            let label = format!("direction={direction}, reverse={reverse}");
            let mut book = ObservationBook {
                positions: [0; 4],
                marks: [OPEN; 4],
                mark_slots: [1; 4],
            };
            let target_marks = [
                OPEN,
                (100 + direction) as u64,
                OPEN,
                (100 - 5 * direction) as u64,
            ];
            // Ten lots on each surviving asset lose 10 and 50 atoms. One new lot costs 10 IM.
            let capital = u128::from(target_marks[1] + target_marks[3]) + 60 + 10;
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 3,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            env.svm.warp_to_slot(1);
            env.activate_asset(3, 1, OPEN);
            for asset in 0..4 {
                env.configure_auth_mark_for_asset_as_admin(asset, 1, OPEN);
            }
            let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
            let portfolios = owners.each_ref().map(|owner| {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key.pubkey(), false),
                    ],
                    &[owner],
                )
                .unwrap();
                env.portfolios.push(key.pubkey());
                key.pubkey()
            });
            let tokens = owners.each_ref().map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
            });
            for actor in 0..2 {
                let amount = [capital, PEER_CAPITAL][actor];
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &tokens[actor],
                        &env.admin.pubkey(),
                        &[],
                        amount as u64,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolios[actor], amount),
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

            let mut retired_roster = vec![];
            let mut retired_slot = None;
            let lot = 10 * POS_SCALE as i128;
            for (asset, delta) in [
                (0, direction * lot),
                (1, -direction * lot),
                (0, -direction * lot),
                (3, direction * lot),
            ] {
                env.svm.expire_blockhash();
                env.trade_asset_with_cu(
                    asset,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    delta,
                    OPEN,
                    0,
                );
                book.positions[asset as usize] += delta;
                let actual = env.portfolio_state(portfolios[0]);
                let mut active: Vec<_> = actual
                    .legs
                    .iter()
                    .filter_map(|leg| {
                        let leg = leg.try_to_runtime().unwrap();
                        leg.active.then_some(leg.asset_index as u16)
                    })
                    .collect();
                active.sort_unstable();
                assert_eq!(
                    active,
                    book.required_assets(),
                    "{label}: public event census"
                );
                if asset == 1 {
                    retired_roster = book.required_assets();
                    retired_slot = actual.legs.iter().position(|leg| {
                        let leg = leg.try_to_runtime().unwrap();
                        leg.active && leg.asset_index == 0
                    });
                }
            }
            assert_eq!(retired_roster, [0, 1]);
            let required = book.required_assets();
            assert_eq!(required, [1, 3]);
            let before = env.portfolio_state(portfolios[0]);
            let replacement = before.legs[retired_slot.unwrap()].try_to_runtime().unwrap();
            assert!(
                replacement.active && replacement.asset_index == 3,
                "{label}: real slot reuse"
            );
            let old_cert = health_cert(&before);
            let before_group = env.market_state().1;
            assert!(old_cert.valid);
            assert_eq!(old_cert.cert_oracle_epoch, before_group.oracle_epoch);
            assert_eq!(old_cert.cert_funding_epoch, before_group.funding_epoch);
            assert_eq!(old_cert.cert_risk_epoch, before_group.risk_epoch);
            assert_eq!(old_cert.cert_asset_set_epoch, before_group.asset_set_epoch);
            assert_eq!(old_cert.active_bitmap_at_cert, active_bitmap(&before));
            assert_eq!(old_cert.certified_equity, capital as i128);
            assert_eq!(old_cert.certified_initial_req, 200);

            let candidate_ix = |env: &V16CuEnv, size_q: i128| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[1], false),
                ],
                data: env
                    .trade_no_cpi_ix(portfolios[0], portfolios[1], 2, size_q, OPEN, 0)
                    .encode(),
            };
            let simulated_frame = inv056_boundary_frame(
                &env,
                &[
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    env.vault,
                    env.mint,
                    env.payer.pubkey(),
                ],
            );
            env.svm
                .simulate_transaction(
                    Transaction::new_signed_with_payer(
                        &[
                            heap_ix(),
                            cu_ix(),
                            candidate_ix(&env, POS_SCALE as i128 + 1),
                        ],
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &owners[0], &owners[1]],
                        env.svm.latest_blockhash(),
                    )
                    .into(),
                )
                .expect(
                    "the identical over-boundary request is live before the authenticated losses",
                );
            inv056_assert_boundary_frame(
                &env,
                &simulated_frame,
                "simulation is not a committed fork",
            );

            env.push_auth_mark_for_asset_as_admin(3, 1, OPEN);
            let old_sequence = env.control_sequences(3);
            let retained_mark = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                data: ProgInstruction::PushAuthMark {
                    asset_index: 3,
                    market_id: env.asset_market_id(3),
                    now_slot: 1,
                    mark_e6: OPEN,
                    observation_sequence: old_sequence.oracle_observation,
                    authority_epoch: old_sequence.authority_epoch,
                }
                .encode(),
            };
            env.svm.warp_to_slot(2);
            for asset in &required {
                env.push_auth_mark_for_asset_as_admin(
                    *asset,
                    u64::MAX,
                    target_marks[*asset as usize],
                );
                book.marks[*asset as usize] = target_marks[*asset as usize];
                book.mark_slots[*asset as usize] = 2;
            }
            // The independent set is unchanged whether the saved certificate is current or stale.
            assert_eq!(book.required_assets(), required);
            assert_eq!(health_cert(&env.portfolio_state(portfolios[0])), old_cert);
            assert!(old_cert.cert_oracle_epoch < env.market_state().1.oracle_epoch);
            assert_eq!(book.pnl(), -60);
            let equity = capital as i128 + book.pnl();
            assert_eq!(equity as u128, book.margin(1_000) + 10);
            let over_notional = ((POS_SCALE + 1) * u128::from(OPEN)).div_ceil(POS_SCALE);
            let over_margin = (over_notional * 1_000).div_ceil(10_000);
            assert_eq!(over_margin, 11);
            assert_eq!(book.margin(1_000) + over_margin, equity as u128 + 1);
            assert!(old_cert.certified_equity as u128 > old_cert.certified_initial_req + 11);
            let market_data = env.svm.get_account(&env.market).unwrap().data;
            for asset in &required {
                let profile =
                    state::read_asset_oracle_profile(&market_data, *asset as usize).unwrap();
                assert_eq!(profile.oracle_authority, env.admin.pubkey().to_bytes());
                assert_eq!(profile.mark_ewma_e6, book.marks[*asset as usize]);
                assert_eq!(
                    profile.last_good_oracle_slot,
                    book.mark_slots[*asset as usize]
                );
                assert_eq!(
                    env.market_state().1.assets[*asset as usize].effective_price,
                    OPEN
                );
            }

            let mut tracked = vec![env.market, env.vault, env.mint, env.admin.pubkey()];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(|owner| owner.pubkey()));
            let custody_keys: Vec<_> = tracked
                .iter()
                .copied()
                .filter(|key| ![env.market, portfolios[0], portfolios[1]].contains(key))
                .collect();
            let custody = inv056_boundary_frame(&env, &custody_keys);
            let honest: Vec<_> = if reverse {
                required.iter().rev().copied().collect()
            } else {
                required.clone()
            };
            let bundle = |env: &V16CuEnv, offered: &[u16], size_q: i128| {
                vec![
                    refresh_ix(env, portfolios[0], offered, 0),
                    refresh_ix(env, portfolios[1], &honest, u64::MAX),
                    candidate_ix(env, size_q),
                ]
            };
            // A genuinely accepted older authority packet cannot replace the adverse mark and
            // restore the admission budget that the pre-loss simulation just demonstrated.
            let oracle_signer = Keypair::from_bytes(&env.admin.to_bytes()).unwrap();
            let mut instructions = vec![retained_mark];
            instructions.extend(bundle(&env, &honest, POS_SCALE as i128 + 1));
            peak_cu = peak_cu.max(submit(
                &mut env,
                instructions,
                &[&oracle_signer, &owners[0], &owners[1]],
                &tracked,
                Some((2, PercolatorError::EngineStale)),
                &format!("{label}: stale authenticated mark"),
            ));
            rejected += 1;
            for (name, offered, error) in [
                ("empty", vec![], PercolatorError::EngineNonProgress),
                ("omit worst", vec![1], PercolatorError::EngineNonProgress),
                (
                    "omit other loss",
                    vec![3],
                    PercolatorError::EngineNonProgress,
                ),
                (
                    "retired roster",
                    retired_roster,
                    PercolatorError::EngineNonProgress,
                ),
                (
                    "duplicate lesser loss",
                    vec![1, 1, 3],
                    PercolatorError::InvalidInstruction,
                ),
                (
                    "unrelated assets",
                    vec![0, 2],
                    PercolatorError::EngineNonProgress,
                ),
            ] {
                let instructions = bundle(&env, &offered, POS_SCALE as i128);
                peak_cu = peak_cu.max(submit(
                    &mut env,
                    instructions,
                    &[&owners[0], &owners[1]],
                    &tracked,
                    Some((2, error)),
                    &format!("{label}: {name}"),
                ));
                rejected += 1;
            }
            // A complete refresh really executes before the one-atom-over trade fails. Its market
            // observations and both recertifications must roll back with the rejected suffix.
            let instructions = bundle(&env, &honest, POS_SCALE as i128 + 1);
            peak_cu = peak_cu.max(submit(
                &mut env,
                instructions,
                &[&owners[0], &owners[1]],
                &tracked,
                Some((4, PercolatorError::EngineInvalidConfig)),
                &format!("{label}: one atom over"),
            ));
            rejected += 1;
            let instructions = bundle(&env, &honest, POS_SCALE as i128);
            peak_cu = peak_cu.max(submit(
                &mut env,
                instructions,
                &[&owners[0], &owners[1]],
                &tracked,
                None,
                &format!("{label}: complete honest boundary"),
            ));
            book.positions[2] = POS_SCALE as i128;
            assert_eq!(book.required_assets(), [1, 2, 3]);
            inv056_assert_boundary_frame(&env, &custody, &label);
            let group = env.market_state().1;
            assert_eq!(group.current_slot, 2);
            assert_eq!(group.vault, capital + PEER_CAPITAL);
            assert_eq!(group.insurance, 0);
            for actor in 0..2 {
                let actual = env.portfolio_state(portfolios[actor]);
                let cert = health_cert(&actual);
                let expected_equity = if actor == 0 {
                    equity
                } else {
                    PEER_CAPITAL as i128 - book.pnl()
                };
                assert_eq!(
                    actual.capital.get() as i128 + actual.pnl.get(),
                    expected_equity
                );
                assert!(cert.valid);
                assert_eq!(cert.cert_oracle_epoch, group.oracle_epoch);
                assert_eq!(cert.cert_funding_epoch, group.funding_epoch);
                assert_eq!(cert.cert_risk_epoch, group.risk_epoch);
                assert_eq!(cert.cert_asset_set_epoch, group.asset_set_epoch);
                assert_eq!(cert.active_bitmap_at_cert, active_bitmap(&actual));
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(&actual)),
                    3
                );
                assert_eq!(cert.certified_equity, expected_equity);
                assert_eq!(cert.certified_initial_req, book.margin(1_000));
                assert_eq!(cert.certified_maintenance_req, book.margin(1_000));
                assert_eq!(cert.certified_worst_case_loss, book.margin(10_000));
                assert_eq!(cert.certified_liq_deficit, 0);
                for asset in book.required_assets() {
                    let leg = active_leg_for_asset(&actual, asset as usize);
                    let sign = if actor == 0 { 1 } else { -1 };
                    assert_eq!(leg.basis_pos_q, sign * book.positions[asset as usize]);
                    assert_eq!(
                        group.assets[asset as usize].effective_price,
                        book.marks[asset as usize]
                    );
                    assert_eq!(
                        group.assets[asset as usize].oi_eff_long_q,
                        book.positions[asset as usize].unsigned_abs()
                    );
                    assert_eq!(
                        group.assets[asset as usize].oi_eff_short_q,
                        book.positions[asset as usize].unsigned_abs()
                    );
                    assert_eq!(
                        [
                            group.assets[asset as usize].a_long,
                            group.assets[asset as usize].a_short
                        ],
                        [ADL_ONE; 2]
                    );
                    assert_eq!(
                        [
                            group.assets[asset as usize].f_long_num,
                            group.assets[asset as usize].f_short_num
                        ],
                        [0; 2]
                    );
                }
                assert!(!has_active_leg_for_asset(&actual, 0));
            }
            let vault =
                TokenAccount::unpack(&env.svm.get_account(&env.vault).unwrap().data).unwrap();
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(u128::from(vault.amount), capital + PEER_CAPITAL);
            assert_eq!(mint.supply, vault.amount);
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(tokens.map(|key| env.token_amount(key)), [0; 3]);
            assert_eq!(
                [
                    group.assets[0].oi_eff_long_q,
                    group.assets[0].oi_eff_short_q
                ],
                [0; 2]
            );
            // Compare decoded outputs only. The sole normalization is each fresh world's random
            // market identity; every economic field, source ledger, epoch and certificate remains.
            let mut comparable_group = group;
            comparable_group.market_group_id = [0; 32];
            let outcome = (
                comparable_group,
                portfolios.map(|key| health_cert(&env.portfolio_state(key))),
                portfolios.map(|key| {
                    let account = env.portfolio_state(key);
                    (account.capital.get(), account.pnl.get())
                }),
            );
            if let Some(reference) = &canonical {
                assert_eq!(&outcome, reference, "{label}: honest order equivalence");
            } else {
                canonical = Some(outcome);
            }
            println!(
                "INV-056 {label}: roster [0,1] -> [1,3], equity={equity}, honest boundary live"
            );
        }
    }
    println!("INV-056 rotated membership: 4 public worlds, {rejected} exact rejections, peak CU={peak_cu}");
}

//! Row 421: a frozen paid prefix does not bind the remaining insurance to that custody.
//! Both insurance roles are absent; fresh beneficiary-owned SPL custody receives the
//! remaining reserve after resolution. The already paid frozen atoms stay unchanged.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::fee::FeeStructure;

const BUDGETS: [[u64; 2]; 2] = [[19, 28], [11, 13]];
const PREFIX: u64 = 7;
const SUPPLY: u64 = 71;
const LIMIT: u64 = 300_000;

fn land(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rent: u64,
) -> u64 {
    env.svm.expire_blockhash();
    let mut batch = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    batch.extend_from_slice(instructions);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &batch,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        signing.len()
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = FeeStructure::default().lamports_per_signature * signing.len() as u64;
    let meta = env
        .svm
        .send_transaction(tx)
        .expect("bounded insurance continuation");
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee + rent;
        } else if changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame {key}"
        );
    }
    assert_cu_within(
        "INV-073 frozen insurance remainder",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn withdrawal(
    env: &V16CuEnv,
    asset: usize,
    owner: Pubkey,
    token: Pubkey,
    amount: u64,
    signed: bool,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner, signed),
            AccountMeta::new(env.market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: env.control_sequences(asset).authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    }
}

#[test]
fn v16_program_frozen_paid_insurance_preserves_unsigned_remainder_and_retirement() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    for target in [0usize, 1] {
        for freeze_before_resolve in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let freezer = Keypair::new();
            let secondary_mint = env.mint;
            let secondary_vault = env.vault;
            let primary = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &primary,
                Mint::LEN,
                spl_token::ID,
            );
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::initialize_mint(
                    &spl_token::ID,
                    &primary.pubkey(),
                    &admin.pubkey(),
                    Some(&freezer.pubkey()),
                    0,
                )
                .unwrap(),
                &[],
            )
            .unwrap();
            env.send(
                ProgInstruction::UpdateBaseUnitMints {
                    primary_mint: primary.pubkey().to_bytes(),
                    secondary_mint: secondary_mint.to_bytes(),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new_readonly(primary.pubkey(), false),
                    AccountMeta::new_readonly(secondary_mint, false),
                    AccountMeta::new_readonly(secondary_vault, false),
                ],
                &[&admin],
            )
            .unwrap();
            env.mint = primary.pubkey();
            env.vault =
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, env.mint);

            let beneficiaries = [Keypair::new(), Keypair::new()];
            let operators = [Keypair::new(), Keypair::new()];
            let wallets = beneficiaries.each_ref().map(Signer::pubkey);
            let operator_keys = operators.each_ref().map(Signer::pubkey);
            for key in wallets.into_iter().chain(operator_keys) {
                env.svm.airdrop(&key, 1_000_000_000).unwrap();
            }
            let originals =
                wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let secondary_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), secondary_mint);
            let assets = [target, 1 - target];
            for actor in 0..2 {
                let asset = assets[actor];
                // The target holder is the live payout operator, then hands off that role.
                for (kind, incoming) in [
                    (processor::ASSET_AUTH_INSURANCE, &beneficiaries[actor]),
                    (
                        processor::ASSET_AUTH_INSURANCE_OPERATOR,
                        if actor == 0 {
                            &beneficiaries[actor]
                        } else {
                            &operators[actor]
                        },
                    ),
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(incoming),
                        asset as u16,
                        kind,
                        incoming.pubkey().to_bytes(),
                    )
                    .unwrap();
                }
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &originals[actor],
                        &admin.pubkey(),
                        &[],
                        BUDGETS[actor].iter().sum(),
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                for side in 0..2 {
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: (2 * asset + side) as u16,
                            market_id: env.asset_market_id(asset as u16),
                            authority_epoch: env.control_sequences(asset).authority_epoch,
                            intent_id: 0,
                            amount: BUDGETS[actor][side].into(),
                        },
                        vec![
                            AccountMeta::new(wallets[actor], true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(originals[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&beneficiaries[actor]],
                    )
                    .unwrap();
                }
            }
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            let prefix = withdrawal(&env, target, wallets[0], originals[0], PREFIX, true);
            let mut peak = [0u64; 4];
            let changed = [env.market, env.vault, originals[0]];
            peak[0] = land(
                &mut env,
                &[prefix],
                &[&beneficiaries[0]],
                &originals,
                &changed,
                0,
            );
            assert_eq!(env.token_amount(originals[0]), PREFIX);
            assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
            env.try_update_per_asset_authority_with_cu(
                &beneficiaries[0],
                Some(&operators[0]),
                target as u16,
                processor::ASSET_AUTH_INSURANCE_OPERATOR,
                operator_keys[0].to_bytes(),
            )
            .unwrap();
            drop((beneficiaries, operators));

            if !freeze_before_resolve {
                env.resolve();
            }
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    spl_token::instruction::freeze_account(
                        &spl_token::ID,
                        &originals[0],
                        &env.mint,
                        &freezer.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &env.mint,
                        None,
                        spl_token::instruction::AuthorityType::FreezeAccount,
                        &freezer.pubkey(),
                        &[],
                    )
                    .unwrap(),
                ],
                &[&freezer],
            )
            .unwrap();
            let freezer_key = freezer.pubkey();
            drop(freezer);
            if freeze_before_resolve {
                env.resolve();
            }
            let frozen_frame = env.svm.get_account(&originals[0]).unwrap();
            let frozen_token = TokenAccount::unpack(&frozen_frame.data).unwrap();
            assert_eq!(
                (frozen_token.state, frozen_token.amount),
                (AccountState::Frozen, PREFIX)
            );
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_frame.data).unwrap();
            assert_eq!(mint.supply, SUPPLY);
            assert_eq!(
                (mint.freeze_authority, mint.mint_authority),
                (COption::None, COption::None)
            );
            let profiles = [0, 1].map(|asset| {
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    asset,
                )
                .unwrap()
            });
            let sequences = [0, 1].map(|asset| env.control_sequences(asset));
            let seeds = ["row421-target", "row421-peer"];
            let destinations = seeds.map(|seed| {
                Pubkey::create_with_seed(&env.payer.pubkey(), seed, &spl_token::ID).unwrap()
            });
            let tracked: Vec<_> = [
                env.market,
                env.vault,
                env.mint,
                secondary_mint,
                secondary_vault,
                admin.pubkey(),
                admin_token,
                secondary_token,
                freezer_key,
            ]
            .into_iter()
            .chain(wallets)
            .chain(operator_keys)
            .chain(originals)
            .chain(destinations)
            .collect();
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            for actor in 0..2 {
                assert!(env.svm.get_account(&destinations[actor]).is_none());
                let creation = [
                    system_instruction::create_account_with_seed(
                        &env.payer.pubkey(),
                        &destinations[actor],
                        &env.payer.pubkey(),
                        seeds[actor],
                        rent,
                        TokenAccount::LEN as u64,
                        &spl_token::ID,
                    ),
                    spl_token::instruction::initialize_account3(
                        &spl_token::ID,
                        &destinations[actor],
                        &env.mint,
                        &wallets[actor],
                    )
                    .unwrap(),
                ];
                peak[1] = peak[1].max(land(
                    &mut env,
                    &creation,
                    &[],
                    &tracked,
                    &[destinations[actor]],
                    rent,
                ));
            }

            let custody_keys = [destinations[0], destinations[1], env.vault];
            let custody_frames = custody_keys.map(|key| env.svm.get_account(&key).unwrap());
            let stock = |env: &V16CuEnv, paid: [u64; 2]| {
                let frame = env.svm.get_account(&env.market).unwrap();
                let group = env.market_state().1;
                let remaining = SUPPLY - paid.iter().sum::<u64>();
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.materialized_portfolio_count,
                        group.pnl_pos_tot
                    ),
                    (0, 0, 0)
                );
                assert_eq!(
                    (
                        group.vault,
                        group.insurance,
                        group.insurance_domain_budget_remaining_total
                    ),
                    (remaining.into(), remaining.into(), remaining.into())
                );
                let mut expected = vec![0u128; 4];
                for actor in 0..2 {
                    expected[2 * assets[actor]] =
                        BUDGETS[actor][0].saturating_sub(paid[actor]).into();
                    expected[2 * assets[actor] + 1] =
                        (BUDGETS[actor][1] - paid[actor].saturating_sub(BUDGETS[actor][0])).into();
                    assert_eq!(
                        profiles[assets[actor]].insurance_authority,
                        wallets[actor].to_bytes()
                    );
                    assert_eq!(
                        profiles[assets[actor]].insurance_operator,
                        operator_keys[actor].to_bytes()
                    );
                    let account = env.svm.get_account(&destinations[actor]).unwrap();
                    let token = TokenAccount::unpack(&account.data).unwrap();
                    assert_ne!(
                        destinations[actor],
                        canonical_vault_ata(wallets[actor], env.mint)
                    );
                    assert_eq!((account.owner, account.lamports), (spl_token::ID, rent));
                    assert_eq!(
                        (token.owner, token.mint, token.state),
                        (wallets[actor], env.mint, AccountState::Initialized)
                    );
                    assert_eq!(
                        (token.delegate, token.close_authority, token.is_native),
                        (COption::None, COption::None, COption::None)
                    );
                    assert_eq!(
                        token.amount,
                        paid[actor] - if actor == 0 { PREFIX } else { 0 }
                    );
                }
                assert_eq!(group.insurance_domain_budget, expected);
                for ((key, frame), amount) in custody_keys.iter().zip(&custody_frames).zip([
                    paid[0] - PREFIX,
                    paid[1],
                    remaining,
                ]) {
                    let mut expected = frame.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(key), Some(expected));
                }
                assert!(group
                    .insurance_domain_spent
                    .iter()
                    .all(|amount| *amount == 0));
                assert_market_stock_census(
                    "frozen insurance remainder",
                    &group,
                    &frame.data,
                    &[],
                    env.token_amount(env.vault).into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("frozen insurance remainder", &group, &[])
                    .unwrap();
                assert_eq!(env.token_amount(env.vault), remaining);
                assert_eq!(
                    env.token_amount(env.vault)
                        + destinations
                            .map(|key| env.token_amount(key))
                            .iter()
                            .sum::<u64>()
                        + PREFIX,
                    SUPPLY
                );
                assert_eq!(env.token_amount(originals[1]), 0);
                assert_eq!(env.token_amount(admin_token), 0);
                assert_eq!(env.token_amount(secondary_vault), 0);
                assert_eq!(env.token_amount(secondary_token), 0);
                assert_eq!(
                    env.svm.get_account(&originals[0]),
                    Some(frozen_frame.clone())
                );
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                for asset in 0..2 {
                    assert_eq!(
                        state::read_asset_oracle_profile(&frame.data, asset).unwrap(),
                        profiles[asset]
                    );
                    assert_eq!(env.control_sequences(asset), sequences[asset]);
                }
            };
            let mut paid = [PREFIX, 0];
            stock(&env, paid);
            for (actor, amount) in [(0, 13), (1, 24), (0, 27)] {
                let ix = withdrawal(
                    &env,
                    assets[actor],
                    wallets[actor],
                    destinations[actor],
                    amount,
                    false,
                );
                assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
                let before = env.market_state().1.insurance;
                let changed = [env.market, env.vault, destinations[actor]];
                peak[2] = peak[2].max(land(&mut env, &[ix], &[], &tracked, &changed, 0));
                paid[actor] += amount;
                stock(&env, paid);
                assert_eq!(before - env.market_state().1.insurance, amount.into());
            }
            assert_eq!(paid, [47, 24]);
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(secondary_vault, false),
                    AccountMeta::new(secondary_token, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: sequences[0].authority_epoch,
                }
                .encode(),
            };
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = [env.market, env.vault, secondary_vault]
                .map(|key| env.svm.get_account(&key).unwrap().lamports)
                .iter()
                .sum::<u64>()
                - tombstone_rent;
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            expected_admin.lamports += refund;
            let mut close_calls = 0;
            loop {
                assert!(
                    close_calls < 4,
                    "two assets have bounded mechanical retirement"
                );
                let cursor = env.market_state().0.terminal_slab_scan_progress;
                let changed = [env.market, env.vault, secondary_vault, admin.pubkey()];
                peak[3] = peak[3].max(land(
                    &mut env,
                    &[close.clone()],
                    &[&admin],
                    &tracked,
                    &changed,
                    0,
                ));
                close_calls += 1;
                let frame = env.svm.get_account(&env.market).unwrap();
                if frame.data.len() == percolator_prog::constants::HEADER_LEN {
                    assert_closed_market_tombstone(&frame);
                    assert_eq!(frame.lamports, tombstone_rent);
                    break;
                }
                stock(&env, paid);
                assert!(env.market_state().0.terminal_slab_scan_progress > cursor);
            }
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            for key in [env.vault, secondary_vault] {
                assert!(env
                    .svm
                    .get_account(&key)
                    .is_none_or(|account| account.lamports == 0
                        && account.data.iter().all(|byte| *byte == 0)));
            }
            assert_eq!(env.svm.get_account(&originals[0]), Some(frozen_frame));
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
            assert_eq!(destinations.map(|key| env.token_amount(key)), [40, 24]);
            println!("INV-073 frozen paid insurance: target={target}, freeze_before_resolve={freeze_before_resolve}, paid={paid:?}, frozen={PREFIX}, unsigned_payouts=3, close_calls={close_calls}/4, CU[live,creation,unsigned,close]={peak:?}");
        }
    }
}

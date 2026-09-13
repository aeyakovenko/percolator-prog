//! INV-073/070/024, row433: unsigned reserve claims survive a change of quote rail.
//! Two backing domains and insurance share dual SPL/native custody. A distinct
//! keeper pays partial claims on one rail and their remainders on the other;
//! primary stock displaced by secondary payment remains raw surplus for CloseSlab.
//! Four fixed histories cover both native-rail placements and reserve orders.
//! No user liabilities, earned fees, custody repair, or absent-admin close is claimed.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::fee::FeeStructure;

const CLAIMS: [u64; 3] = [401, 307, 67];
const PREFIX: [u64; 3] = [101, 59, 17];
const FUNDED: u64 = CLAIMS[0] + CLAIMS[1] + CLAIMS[2];
const SECONDARY: u64 = 997;
const CU_LIMIT: u64 = 150_000;

struct Rail {
    mint: Pubkey,
    vault: Pubkey,
    recipients: [Pubkey; 2],
    admin_token: Pubkey,
    empty: [Account; 4],
    mint_frame: Account,
}

fn execute(
    env: &mut V16CuEnv,
    ix: Instruction,
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
) -> u64 {
    env.svm.expire_blockhash();
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
            ix,
        ],
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        1 + signers.len()
    );
    assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = signing.len() as u64 * FeeStructure::default().lamports_per_signature;
    let meta = env
        .svm
        .send_transaction(tx)
        .expect("public dual-quote reserve progress");
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        } else if changed.contains(&key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(&key),
            expected,
            "complete Account frame {key}"
        );
    }
    assert_cu_within(
        "row433 dual-quote reserve progress",
        meta.compute_units_consumed,
        CU_LIMIT,
    );
    meta.compute_units_consumed
}

fn token_image(empty: &Account, amount: u64) -> Account {
    // Build an expected image only; it is never installed into LiteSVM.
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&empty.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.state, AccountState::Initialized);
    assert_eq!(
        (token.delegate, token.close_authority),
        (COption::None, COption::None)
    );
    if let COption::Some(rent) = token.is_native {
        assert_eq!(token.mint, spl_token::native_mint::ID);
        assert_eq!(empty.lamports, rent);
        expected.lamports += amount;
    }
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

#[test]
fn v16_program_unsigned_dual_quote_reserves_preserve_domain_claims_and_terminal_surplus() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let mut worlds = 0;
    let mut payments = 0;
    let mut peak = [0u64; 2];
    for native_rail in 0..2 {
        for reverse in [false, true] {
            let mut env = inv081_public_native_market();
            let admin = env.admin.insecure_clone();
            let added_mint = inv018_create_public_spl_mint(
                &mut env.svm,
                &env.payer,
                admin.pubkey(),
                spl_token::native_mint::DECIMALS,
            );
            let mints = if native_rail == 0 {
                [env.mint, added_mint]
            } else {
                [added_mint, env.mint]
            };
            env.send(
                ProgInstruction::UpdateBaseUnitMints {
                    primary_mint: mints[0].to_bytes(),
                    secondary_mint: mints[1].to_bytes(),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new_readonly(mints[0], false),
                    AccountMeta::new_readonly(mints[1], false),
                    AccountMeta::new_readonly(env.vault, false),
                ],
                &[&admin],
            )
            .unwrap();
            // Only host handles change after the public mint configuration succeeds.
            env.mint = mints[0];
            let native_vault = env.vault;
            let provider = Keypair::new();
            let beneficiary = Keypair::new();
            let operator = Keypair::new();
            let wallets = [provider.pubkey(), beneficiary.pubkey(), operator.pubkey()];
            assert!(!wallets.contains(&env.payer.pubkey()));
            assert!(!wallets.contains(&admin.pubkey()));
            for (role, holder) in [
                (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
                (processor::ASSET_AUTH_INSURANCE, &beneficiary),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
            ] {
                env.svm.airdrop(&holder.pubkey(), 1_000_000_000).unwrap();
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    0,
                    role,
                    holder.pubkey().to_bytes(),
                )
                .unwrap();
            }
            let rails = mints.map(|mint| {
                let vault = if mint == spl_token::native_mint::ID {
                    native_vault
                } else {
                    create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, mint)
                };
                let recipients = [wallets[0], wallets[1]]
                    .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, mint));
                let admin_token =
                    create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint);
                let empty = [vault, recipients[0], recipients[1], admin_token]
                    .map(|key| env.svm.get_account(&key).unwrap());
                for (account, owner) in
                    empty
                        .iter()
                        .zip([env.vault_authority, wallets[0], wallets[1], admin.pubkey()])
                {
                    let token = TokenAccount::unpack(&account.data).unwrap();
                    assert_eq!(account.owner, spl_token::ID);
                    assert_eq!((token.mint, token.owner, token.amount), (mint, owner, 0));
                }
                Rail {
                    mint,
                    vault,
                    recipients,
                    admin_token,
                    empty,
                    mint_frame: env.svm.get_account(&mint).unwrap(),
                }
            });
            env.vault = rails[0].vault;
            let mut rails = rails;
            for (rail, custody) in rails.iter_mut().enumerate() {
                let funding = if rail == 0 {
                    vec![
                        (custody.recipients[0], CLAIMS[0] + CLAIMS[1]),
                        (custody.recipients[1], CLAIMS[2]),
                    ]
                } else {
                    vec![(custody.admin_token, SECONDARY)]
                };
                for (destination, amount) in funding {
                    let ixs = if rail == native_rail {
                        vec![
                            system_instruction::transfer(&admin.pubkey(), &destination, amount),
                            spl_token::instruction::sync_native(&spl_token::ID, &destination)
                                .unwrap(),
                        ]
                    } else {
                        vec![spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &custody.mint,
                            &destination,
                            &admin.pubkey(),
                            &[],
                            amount,
                        )
                        .unwrap()]
                    };
                    send_raw_ixs(&mut env.svm, &env.payer, ixs, &[&admin]).unwrap();
                }
                if rail != native_rail {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &custody.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                }
                custody.mint_frame = env.svm.get_account(&custody.mint).unwrap();
                let mint = Mint::unpack(&custody.mint_frame.data).unwrap();
                assert_eq!(
                    mint.supply,
                    if rail == native_rail {
                        0
                    } else if rail == 0 {
                        FUNDED
                    } else {
                        SECONDARY
                    }
                );
                assert_eq!(
                    (mint.mint_authority, mint.freeze_authority),
                    (COption::None, COption::None)
                );
            }
            env.svm.warp_to_slot(1);
            for (domain, amount) in CLAIMS[..2].iter().copied().enumerate() {
                env.send(
                    ProgInstruction::TopUpBackingBucket {
                        domain: domain as u16,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        intent_id: 0,
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: amount.into(),
                        expiry_slot: 100,
                    },
                    vec![
                        AccountMeta::new(wallets[0], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(rails[0].recipients[0], false),
                        AccountMeta::new(rails[0].vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&provider],
                )
                .unwrap();
            }
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    intent_id: 0,
                    amount: CLAIMS[2].into(),
                },
                vec![
                    AccountMeta::new(wallets[1], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(rails[0].recipients[1], false),
                    AccountMeta::new(rails[0].vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&beneficiary],
            )
            .unwrap();
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &rails[1].admin_token,
                    &rails[1].vault,
                    &admin.pubkey(),
                    &[],
                    SECONDARY,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            drop((provider, beneficiary, operator));
            env.resolve();
            let sequences = env.control_sequences(0);
            let resolved_market = env.svm.get_account(&env.market).unwrap();
            let profile = state::read_asset_oracle_profile(&resolved_market.data, 0).unwrap();
            assert_eq!(profile.backing_bucket_authority, wallets[0].to_bytes());
            assert_eq!(profile.insurance_authority, wallets[1].to_bytes());
            let tracked = [env.market, env.vault_authority, admin.pubkey()]
                .into_iter()
                .chain(wallets)
                .chain(rails.iter().flat_map(|rail| {
                    [
                        rail.mint,
                        rail.vault,
                        rail.recipients[0],
                        rail.recipients[1],
                        rail.admin_token,
                    ]
                }))
                .collect::<Vec<_>>();
            let check = |env: &V16CuEnv, paid: [[u64; 2]; 3], closed: bool| {
                let remaining: [u64; 3] =
                    std::array::from_fn(|kind| CLAIMS[kind] - paid[kind].iter().sum::<u64>());
                let paid_by_rail: [u64; 2] =
                    std::array::from_fn(|rail| paid.iter().map(|kind| kind[rail]).sum());
                let physical = [FUNDED - paid_by_rail[0], SECONDARY - paid_by_rail[1]];
                let outstanding = remaining.iter().sum::<u64>();
                assert_eq!(
                    physical[0],
                    outstanding + paid_by_rail[1],
                    "secondary payouts leave displaced primary stock as surplus"
                );
                assert_eq!(physical.iter().sum::<u64>(), outstanding + SECONDARY);
                for (rail, custody) in rails.iter().enumerate() {
                    assert_eq!(
                        env.svm.get_account(&custody.mint),
                        Some(custody.mint_frame.clone())
                    );
                    let amounts = [
                        physical[rail],
                        paid[0][rail] + paid[1][rail],
                        paid[2][rail],
                        if closed { physical[rail] } else { 0 },
                    ];
                    for (i, key) in [
                        custody.vault,
                        custody.recipients[0],
                        custody.recipients[1],
                        custody.admin_token,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if closed && i == 0 {
                            if let Some(account) = env.svm.get_account(&key) {
                                assert_eq!(account.lamports, 0);
                                assert!(account.data.iter().all(|byte| *byte == 0));
                            }
                        } else {
                            assert_eq!(
                                env.svm.get_account(&key),
                                Some(token_image(&custody.empty[i], amounts[i])),
                                "exact custody image on rail {rail}, account {i}"
                            );
                        }
                    }
                    assert_eq!(
                        [
                            custody.recipients[0],
                            custody.recipients[1],
                            custody.admin_token
                        ]
                        .into_iter()
                        .map(|key| env.token_amount(key))
                        .sum::<u64>()
                            + if closed {
                                0
                            } else {
                                env.token_amount(custody.vault)
                            },
                        if rail == 0 { FUNDED } else { SECONDARY },
                    );
                }
                let market = env.svm.get_account(&env.market).unwrap();
                if closed {
                    assert_eq!(remaining, [0; 3]);
                    assert_closed_market_tombstone(&market);
                    return;
                }
                assert_eq!(market.lamports, resolved_market.lamports);
                let (cfg, group) = state::read_market(&market.data).unwrap();
                assert_eq!(
                    (cfg.collateral_mint, cfg.secondary_collateral_mint),
                    (mints[0].to_bytes(), mints[1].to_bytes())
                );
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(group.vault, outstanding.into());
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(group.backing_provider_earnings_total, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.insurance, remaining[2].into());
                assert_eq!(
                    group.insurance_domain_budget_remaining_total,
                    remaining[2].into()
                );
                assert_eq!(group.insurance_domain_budget[0], remaining[2].into());
                assert!(group.insurance_domain_budget[1..]
                    .iter()
                    .all(|value| *value == 0));
                assert!(group.insurance_domain_spent.iter().all(|value| *value == 0));
                for (domain, unpaid) in remaining[..2].iter().copied().enumerate() {
                    let bucket = group.source_backing_buckets[domain];
                    let source = group.source_credit[domain];
                    assert_eq!(
                        bucket.fresh_unliened_backing_num,
                        u128::from(unpaid) * BOUND_SCALE
                    );
                    assert_eq!(
                        source.fresh_reserved_backing_num,
                        u128::from(unpaid) * BOUND_SCALE
                    );
                    assert_eq!(bucket.expiry_slot, if unpaid == 0 { 0 } else { 100 });
                    assert_eq!(
                        bucket.status,
                        if unpaid == 0 {
                            BackingBucketStatusV16::Empty
                        } else {
                            BackingBucketStatusV16::Fresh
                        }
                    );
                    assert_eq!(
                        (
                            bucket.valid_liened_backing_num,
                            bucket.consumed_liened_backing_num
                        ),
                        (0, 0)
                    );
                    assert_eq!(
                        (source.provider_receivable_num, source.spent_backing_num),
                        (0, 0)
                    );
                }
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                // The census covers logical custody; the rail equations above separately
                // classify all raw surplus without importing it into reserve claims.
                assert_market_stock_census(
                    "row433 dual-quote reserves",
                    &group,
                    &market.data,
                    &[],
                    outstanding.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("row433 dual-quote reserves", &group, &[])
                    .unwrap();
                let mut data = market.data.clone();
                state::market_view_mut(&mut data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            let mut paid = [[0u64; 2]; 3];
            check(&env, paid, false);
            let order = if reverse { [2, 1, 0] } else { [0, 1, 2] };
            for round in 0..2 {
                for kind in order {
                    let rail = (kind + round) % 2;
                    let amount = if round == 0 {
                        PREFIX[kind]
                    } else {
                        CLAIMS[kind] - PREFIX[kind]
                    };
                    let recipient = usize::from(kind == 2);
                    let instruction = if kind == 2 {
                        ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: 0,
                            market_id: env.asset_market_id(0),
                            authority_epoch: sequences.authority_epoch,
                            amount: amount.into(),
                        }
                    } else {
                        ProgInstruction::WithdrawBackingBucket {
                            domain: kind as u16,
                            market_id: env.asset_market_id(0),
                            authority_epoch: sequences.authority_epoch,
                            amount: amount.into(),
                        }
                    };
                    let ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new_readonly(wallets[recipient], false),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(rails[rail].recipients[recipient], false),
                            AccountMeta::new(rails[rail].vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: instruction.encode(),
                    };
                    assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
                    assert!(!ix
                        .accounts
                        .iter()
                        .any(|meta| meta.pubkey == wallets[2] || meta.pubkey == admin.pubkey()));
                    let before = env.market_state().1.vault;
                    let changed = [
                        env.market,
                        rails[rail].vault,
                        rails[rail].recipients[recipient],
                    ];
                    peak[0] = peak[0].max(execute(&mut env, ix, &[], &tracked, &changed));
                    paid[kind][rail] += amount;
                    payments += 1;
                    assert_eq!(before - env.market_state().1.vault, amount.into());
                    check(&env, paid, false);
                }
            }
            assert_eq!(env.market_state().1.vault, 0);
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            let close = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(rails[0].vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(rails[0].admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(rails[1].vault, false),
                    AccountMeta::new(rails[1].admin_token, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            let changed = [
                env.market,
                admin.pubkey(),
                rails[0].vault,
                rails[1].vault,
                rails[0].admin_token,
                rails[1].admin_token,
            ];
            peak[1] = peak[1].max(execute(&mut env, close, &[&admin], &tracked, &changed));
            check(&env, paid, true);
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                tombstone_rent
            );
            let mut expected_admin = admin_before;
            expected_admin.lamports += resolved_market.lamports - tombstone_rent
                + rails.iter().map(|rail| rail.empty[0].lamports).sum::<u64>();
            assert_eq!(
                env.svm.get_account(&admin.pubkey()),
                Some(expected_admin),
                "native principal is transferred to custody, never refunded as rent"
            );
            worlds += 1;
        }
    }
    assert_eq!((worlds, payments), (4, 24));
    eprintln!("row433 dual-quote reserve progress: worlds={worlds}, unsigned_payments={payments}, closures={worlds}, peak_cu={peak:?}");
}

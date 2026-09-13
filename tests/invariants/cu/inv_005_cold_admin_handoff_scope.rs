//! INV-005/024/027/055: cold-admin ABA and renunciation preserve funded-role scope.
//! A finite public-route product, with independent stock and recipient accounting.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_005_funded_role_zero_transition.rs"]
mod funded_role_zero_transition;

const ROLES: [u8; 3] = [
    processor::ASSET_AUTH_INSURANCE,
    processor::ASSET_AUTH_INSURANCE_OPERATOR,
    processor::ASSET_AUTH_BACKING_BUCKET,
];
const BACKING: [u128; 4] = [101, 103, 107, 109];
const INSURANCE: [u128; 4] = [17, 19, 23, 29];
const USER: u128 = 211;
const SUPPLY: u128 = 719;

fn handoff(
    env: &V16CuEnv,
    asset: usize,
    kind: u8,
    from: Pubkey,
    to: Option<Pubkey>,
    epoch: u64,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(from, true),
            AccountMeta::new(to.unwrap_or(env.payer.pubkey()), to.is_some()),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: epoch,
            kind,
            new_pubkey: to.map(|key| key.to_bytes()).unwrap_or_default(),
        }
        .encode(),
    }
}

fn signed(env: &V16CuEnv, instructions: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(instructions);
    let mut signatures = vec![&env.payer];
    signatures.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    tx
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn land(
    env: &mut V16CuEnv,
    tx: Transaction,
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
    spl_successes: usize,
) -> u64 {
    // Include compiled accounts as well as economic sentinels absent from the message.
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before = frame(env, &keys);
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let rejected = rejection.is_some();
    let wrapper_successes = rejection
        .as_ref()
        .map(|(index, _)| usize::from(*index) - 2)
        .unwrap_or(tx.message.instructions.len() - 2);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = rejection {
        let failed = result.expect_err("authority scope must reject atomically");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "{failed:?}"
        );
        failed.meta
    } else {
        result.expect("current authorized public continuation remains live")
    };
    for (key, mut account) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !changed.contains(&key) || key == env.payer.pubkey() {
            assert_eq!(env.svm.get_account(&key), account, "complete Account {key}");
        }
    }
    for (program, count) in [
        (env.program_id, wrapper_successes),
        (spl_token::ID, spl_successes),
    ] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count,
            "completed public prefixes: {:?}",
            meta.logs
        );
    }
    if spl_successes == 0 {
        assert!(!meta
            .logs
            .iter()
            .any(|line| line.starts_with(&format!("Program {} invoke", spl_token::ID))));
    }
    assert_cu_within(
        "cold-admin handoff scope",
        meta.compute_units_consumed,
        600_000,
    );
    meta.compute_units_consumed
}

fn profile(env: &V16CuEnv, asset: usize) -> state::AssetOracleProfileV16 {
    state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, asset)
        .unwrap()
}

fn set_holder(profile: &mut state::AssetOracleProfileV16, kind: u8, key: Pubkey) {
    match kind {
        processor::ASSET_AUTH_INSURANCE => profile.insurance_authority = key.to_bytes(),
        processor::ASSET_AUTH_INSURANCE_OPERATOR => profile.insurance_operator = key.to_bytes(),
        processor::ASSET_AUTH_BACKING_BUCKET => profile.backing_bucket_authority = key.to_bytes(),
        _ => unreachable!(),
    }
}

struct Stock {
    backing: [u128; 4],
    insurance: [u128; 4],
    wallets: [u128; 9],
}

impl Stock {
    fn assert(&self, env: &V16CuEnv, wallets: &[Pubkey; 9]) {
        let (_, group) = env.market_state();
        let insurance = self.insurance.iter().sum::<u128>();
        let vault = USER + insurance + self.backing.iter().sum::<u128>();
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.c_tot, USER);
        assert_eq!(group.insurance, insurance);
        assert_eq!(group.vault, vault);
        assert_eq!(env.token_amount(env.vault) as u128, vault);
        assert_eq!(&group.insurance_domain_budget[..4], &self.insurance);
        for (domain, amount) in self.backing.iter().enumerate() {
            let bucket = &group.source_backing_buckets[domain];
            assert_eq!(bucket.fresh_unliened_backing_num, amount * BOUND_SCALE);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(bucket.utilization_fee_earnings, 0);
        }
        for (wallet, amount) in wallets.iter().zip(self.wallets) {
            assert_eq!(env.token_amount(*wallet) as u128, amount);
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply as u128, SUPPLY);
        assert_eq!(vault + self.wallets.iter().sum::<u128>(), SUPPLY);
    }
}

fn payout(
    env: &V16CuEnv,
    from: Pubkey,
    wallet: Pubkey,
    domain: usize,
    backing: bool,
    amount: u128,
) -> Instruction {
    let asset = domain / 2;
    let epoch = env.control_sequences(asset).authority_epoch;
    let market_id = env.asset_market_id(asset as u16);
    let ix = if backing {
        ProgInstruction::WithdrawBackingBucket {
            domain: domain as u16,
            market_id,
            authority_epoch: epoch,
            amount,
        }
    } else {
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id,
            authority_epoch: epoch,
            amount,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(from, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ix.encode(),
    }
}

#[test]
fn v16_program_cold_admin_aba_and_burn_preserve_funded_handoff_scope_and_value() {
    assert_eq!(
        SUPPLY,
        USER + BACKING.iter().sum::<u128>() + INSURANCE.iter().sum::<u128>()
    );
    let mut peak = [0; 3]; // management, rejected bundles, payouts
    let mut worlds = 0;
    for (role_index, kind) in ROLES.into_iter().enumerate() {
        for subject in 0..2usize {
            for drain_only in [false, true] {
                let peer = 1 - subject;
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let cold = [Keypair::new(), Keypair::new()];
                let interim = Keypair::new();
                let holders = [Keypair::new(), Keypair::new(), Keypair::new()];
                let successor = Keypair::new();
                let user = Keypair::new();
                let actors = [
                    &cold[0],
                    &cold[1],
                    &interim,
                    &holders[0],
                    &holders[1],
                    &holders[2],
                    &successor,
                    &user,
                    &admin,
                ];
                for actor in &actors[..8] {
                    env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
                }
                let wallets = actors.map(|actor| {
                    create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
                });
                for (index, amount) in [
                    (3, INSURANCE.iter().sum::<u128>()),
                    (5, BACKING.iter().sum()),
                    (7, USER),
                ] {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &wallets[index],
                            &admin.pubkey(),
                            &[],
                            amount as u64,
                        )
                        .unwrap(),
                        &[&admin],
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
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                for asset in 0..2 {
                    for (role, holder) in ROLES.into_iter().zip(&holders) {
                        env.try_update_per_asset_authority_with_cu(
                            &admin,
                            Some(holder),
                            asset,
                            role,
                            holder.pubkey().to_bytes(),
                        )
                        .unwrap();
                    }
                }
                let portfolio_key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &portfolio_key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                let portfolio = portfolio_key.pubkey();
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(user.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[&user],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolio, USER),
                    vec![
                        AccountMeta::new(user.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(wallets[7], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&user],
                )
                .unwrap();
                for domain in 0..4 {
                    let asset = domain / 2;
                    for backing in [false, true] {
                        let seq = env.control_sequences(asset);
                        let market_id = env.asset_market_id(asset as u16);
                        let (ix, holder, wallet) = if backing {
                            (
                                ProgInstruction::TopUpBackingBucket {
                                    domain: domain as u16,
                                    market_id,
                                    authority_epoch: seq.authority_epoch,
                                    intent_id: next_control_sequence(seq.backing_top_up),
                                    backing_fee_bps: 0,
                                    insurance_share_bps: 0,
                                    amount: BACKING[domain],
                                    expiry_slot: 10_000,
                                },
                                &holders[2],
                                wallets[5],
                            )
                        } else {
                            (
                                ProgInstruction::TopUpInsuranceDomain {
                                    domain: domain as u16,
                                    market_id,
                                    authority_epoch: seq.authority_epoch,
                                    intent_id: next_control_sequence(seq.insurance_top_up),
                                    amount: INSURANCE[domain],
                                },
                                &holders[0],
                                wallets[3],
                            )
                        };
                        env.send(
                            ix,
                            vec![
                                AccountMeta::new(holder.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(wallet, false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[holder],
                        )
                        .unwrap();
                    }
                }
                if drain_only {
                    env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_DRAIN_ONLY,
                        subject as u16,
                        0,
                        0,
                    );
                }
                for (asset, key) in cold.iter().enumerate() {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(key),
                        asset as u16,
                        processor::ASSET_AUTH_ADMIN,
                        key.pubkey().to_bytes(),
                    )
                    .unwrap();
                }
                let mut tracked = vec![
                    env.market,
                    env.vault,
                    env.mint,
                    env.vault_authority,
                    portfolio,
                    env.payer.pubkey(),
                ];
                tracked.extend(wallets);
                tracked.extend(actors.map(Signer::pubkey));
                let market = env.market;
                let initial_economics = env.market_state();
                assert_eq!(
                    initial_economics.1.assets[subject].lifecycle,
                    if drain_only {
                        AssetLifecycleV16::DrainOnly
                    } else {
                        AssetLifecycleV16::Active
                    }
                );
                assert_eq!(
                    initial_economics.1.assets[peer].lifecycle,
                    AssetLifecycleV16::Active
                );
                let mut profiles = [profile(&env, 0), profile(&env, 1)];
                let mut sequences = [env.control_sequences(0), env.control_sequences(1)];
                let mut stock = Stock {
                    backing: BACKING,
                    insurance: INSURANCE,
                    wallets: [0; 9],
                };
                stock.assert(&env, &wallets);
                let check_management = |env: &V16CuEnv,
                                        profiles: &[state::AssetOracleProfileV16; 2],
                                        sequences: &[_; 2]| {
                    assert_eq!(
                        env.market_state(),
                        initial_economics,
                        "management preserves the entire decoded economy"
                    );
                    assert_eq!([profile(env, 0), profile(env, 1)], *profiles);
                    assert_eq!(
                        [env.control_sequences(0), env.control_sequences(1)],
                        *sequences
                    );
                    stock.assert(env, &wallets);
                };

                // Retain signatures, blockhash, metas and bytes before either cold-admin rotation.
                env.svm.expire_blockhash();
                let retained_ixs = [peer, subject].map(|asset| {
                    handoff(
                        &env,
                        asset,
                        kind,
                        holders[role_index].pubkey(),
                        Some(successor.pubkey()),
                        sequences[asset].authority_epoch,
                    )
                });
                let retained_pair =
                    signed(&env, &retained_ixs, &[&holders[role_index], &successor]);
                let retained_peer = signed(
                    &env,
                    &retained_ixs[..1],
                    &[&holders[role_index], &successor],
                );
                let retained_subject = signed(
                    &env,
                    &retained_ixs[1..],
                    &[&holders[role_index], &successor],
                );
                let before_simulation = frame(&env, &tracked);
                env.svm
                    .simulate_transaction(retained_pair.clone().into())
                    .expect("both funded incumbents can transfer before cold-admin ABA");
                assert_eq!(frame(&env, &tracked), before_simulation);
                for (from, to) in [(&cold[subject], &interim), (&interim, &cold[subject])] {
                    let ix = handoff(
                        &env,
                        subject,
                        processor::ASSET_AUTH_ADMIN,
                        from.pubkey(),
                        Some(to.pubkey()),
                        sequences[subject].authority_epoch,
                    );
                    let tx = signed(&env, &[ix], &[from, to]);
                    peak[0] = peak[0].max(land(&mut env, tx, &tracked, &[market], None, 0));
                    profiles[subject].asset_admin = to.pubkey().to_bytes();
                    sequences[subject].authority_epoch += 1;
                    check_management(&env, &profiles, &sequences);
                }
                for (tx, index) in [(retained_subject, 2), (retained_pair, 3)] {
                    peak[1] = peak[1].max(land(
                        &mut env,
                        tx,
                        &tracked,
                        &[],
                        Some((index, PercolatorError::EngineStale)),
                        0,
                    ));
                    check_management(&env, &profiles, &sequences);
                }

                // A live peer handoff executes, but the correctly signed cold-admin suffix
                // cannot replace a funded incumbent. Both profile and epoch writes roll back.
                let cold_replacement = handoff(
                    &env,
                    subject,
                    kind,
                    cold[subject].pubkey(),
                    Some(successor.pubkey()),
                    sequences[subject].authority_epoch,
                );
                let tx = signed(
                    &env,
                    &[retained_ixs[0].clone(), cold_replacement],
                    &[&holders[role_index], &successor, &cold[subject]],
                );
                peak[1] = peak[1].max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((3, PercolatorError::EngineLockActive)),
                    0,
                ));
                check_management(&env, &profiles, &sequences);

                let burn = handoff(
                    &env,
                    subject,
                    processor::ASSET_AUTH_ADMIN,
                    cold[subject].pubkey(),
                    None,
                    sequences[subject].authority_epoch,
                );
                let tx = signed(&env, &[burn], &[&cold[subject]]);
                peak[0] = peak[0].max(land(&mut env, tx, &tracked, &[market], None, 0));
                profiles[subject].asset_admin = [0; 32];
                sequences[subject].authority_epoch += 1;
                check_management(&env, &profiles, &sequences);
                let revoked = handoff(
                    &env,
                    subject,
                    kind,
                    cold[subject].pubkey(),
                    Some(successor.pubkey()),
                    sequences[subject].authority_epoch,
                );
                let tx = signed(&env, &[revoked], &[&cold[subject], &successor]);
                peak[1] = peak[1].max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((2, PercolatorError::Unauthorized)),
                    0,
                ));
                check_management(&env, &profiles, &sequences);

                // The sibling's original signed message stays live; only the subject's
                // epoch is renewed. Each transfer requires its unchanged funded incumbent.
                peak[0] = peak[0].max(land(&mut env, retained_peer, &tracked, &[market], None, 0));
                set_holder(&mut profiles[peer], kind, successor.pubkey());
                sequences[peer].authority_epoch += 1;
                check_management(&env, &profiles, &sequences);
                let renewed = handoff(
                    &env,
                    subject,
                    kind,
                    holders[role_index].pubkey(),
                    Some(successor.pubkey()),
                    sequences[subject].authority_epoch,
                );
                assert_eq!(renewed.accounts, retained_ixs[1].accounts);
                let tx = signed(&env, &[renewed], &[&holders[role_index], &successor]);
                peak[0] = peak[0].max(land(&mut env, tx, &tracked, &[market], None, 0));
                set_holder(&mut profiles[subject], kind, successor.pubkey());
                sequences[subject].authority_epoch += 1;
                check_management(&env, &profiles, &sequences);

                // Insurance policy succession does not confer the independent hot
                // operator's payout right. Other transferred roles revoke that right.
                let wrong = if role_index == 0 { 6 } else { role_index + 3 };
                let ix = payout(
                    &env,
                    actors[wrong].pubkey(),
                    wallets[wrong],
                    subject * 2,
                    role_index == 2,
                    1,
                );
                let tx = signed(&env, &[ix], &[actors[wrong]]);
                peak[1] = peak[1].max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((2, PercolatorError::Unauthorized)),
                    0,
                ));
                check_management(&env, &profiles, &sequences);

                for asset in [peer, subject] {
                    for backing in [false, true] {
                        let recipient =
                            if (backing && role_index == 2) || (!backing && role_index == 1) {
                                6
                            } else if backing {
                                5
                            } else {
                                4
                            };
                        let domains = if backing {
                            vec![asset * 2, asset * 2 + 1]
                        } else {
                            vec![asset * 2]
                        };
                        for domain in domains {
                            let amount = if backing {
                                stock.backing[domain]
                            } else {
                                stock.insurance[domain] + stock.insurance[domain + 1]
                            };
                            let ix = payout(
                                &env,
                                actors[recipient].pubkey(),
                                wallets[recipient],
                                domain,
                                backing,
                                amount,
                            );
                            let tx = signed(&env, &[ix], &[actors[recipient]]);
                            let changed = [market, env.vault, wallets[recipient]];
                            peak[2] = peak[2].max(land(&mut env, tx, &tracked, &changed, None, 1));
                            if backing {
                                stock.backing[domain] = 0;
                            } else {
                                stock.insurance[domain] = 0;
                                stock.insurance[domain + 1] = 0;
                            }
                            stock.wallets[recipient] += amount;
                            stock.assert(&env, &wallets);
                            assert_eq!([profile(&env, 0), profile(&env, 1)], profiles);
                            assert_eq!(
                                [env.control_sequences(0), env.control_sequences(1)],
                                sequences
                            );
                        }
                    }
                }
                assert_eq!(env.token_amount(env.vault) as u128, USER);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 12);
    eprintln!("INV-005 cold-admin scope: worlds={worlds}, prevalidated_pairs=12, exact_rejections=60, committed_handoffs=24, reserve_payouts=72, peak CU [management, rejection, payout]={peak:?}");
}

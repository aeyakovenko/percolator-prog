//! Row 433 / INV-073: one asset's insurance succession must not redirect the
//! peer asset when their previously shared custody is reassigned to the successor.
//! Four public SPL histories cross the transferred asset and reserve payout order.
//! Distinct asset ledgers, unchanged peer consent, keeper-created alternate custody,
//! exact rollback and per-asset exhaustion are checked through signed slab closure.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const DOMAIN_BUDGETS: [[u64; 2]; 2] = [[19, 28], [31, 40]];
const CLAIMS: [u64; 2] = [47, 71];
const PAID_PREFIX: [u64; 2] = [17, 23];
const SUPPLY: u64 = 118;
const LIMIT: u64 = 300_000;

#[derive(Default)]
struct Evidence {
    peak: u64,
    rollbacks: usize,
}

impl Evidence {
    #[allow(clippy::too_many_arguments)]
    fn land(
        &mut self,
        env: &mut V16CuEnv,
        ixs: &[Instruction],
        signers: &[&Keypair],
        tracked: &[Pubkey],
        changed: &[Pubkey],
        rent: u64,
        error: Option<(u8, u32, usize)>,
    ) {
        let mut keys = tracked.to_vec();
        keys.push(env.payer.pubkey());
        keys.extend(
            ixs.iter()
                .flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey)),
        );
        keys.sort_unstable();
        keys.dedup();
        let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let cu = insurance_succession_tx(env, ixs, signers, tracked, error);
        self.peak = self.peak.max(cu);
        assert_cu_within("row433 split beneficiary custody", cu, LIMIT);
        if error.is_some() {
            self.rollbacks += 1;
            return;
        }
        for (key, mut account) in keys.into_iter().zip(before) {
            if key == env.payer.pubkey() {
                account.as_mut().unwrap().lamports -= rent
                    + (1 + signers.len()) as u64 * FeeStructure::default().lamports_per_signature;
            } else if changed.contains(&key) {
                continue;
            }
            assert_eq!(env.svm.get_account(&key), account, "success frame {key}");
        }
    }
}

fn payout(
    env: &V16CuEnv,
    asset: usize,
    owner: Pubkey,
    destination: Pubkey,
    ledger: Pubkey,
    amount: u64,
    epoch: u64,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: epoch,
            amount: amount.into(),
        }
        .encode(),
    }
}

fn token_image(empty: &Account, owner: Pubkey, amount: u64) -> Account {
    let mut image = empty.clone();
    let mut token = TokenAccount::unpack(&image.data).unwrap();
    token.owner = owner;
    token.amount = amount;
    TokenAccount::pack(token, &mut image.data).unwrap();
    image
}

#[test]
fn v16_program_split_insurance_succession_replaces_shared_custody_without_redirecting_peer() {
    let mut evidence = Evidence::default();
    for target in 0..2 {
        for target_first in [false, true] {
            run(target, target_first, &mut evidence);
        }
    }
    assert_eq!(evidence.rollbacks, 32);
    eprintln!(
        "row433 split custody: worlds=4, rollbacks={}, unsigned_payments=16, closures=4, peak_CU={}, limit={LIMIT}",
        evidence.rollbacks, evidence.peak
    );
}

fn run(target: usize, target_first: bool, evidence: &mut Evidence) {
    let peer = 1 - target;
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.insecure_clone();
    let a = Keypair::new();
    let b = Keypair::new();
    let operator = Keypair::new();
    let wallets = [a.pubkey(), b.pubkey(), operator.pubkey(), admin.pubkey()];
    assert!(!wallets.contains(&env.payer.pubkey()));
    for key in &wallets[..3] {
        env.svm.airdrop(key, 1_000_000_000).unwrap();
    }
    for asset in 0..2 {
        for (role, holder) in [
            (processor::ASSET_AUTH_INSURANCE, &a),
            (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
        ] {
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
    let shared = create_ata_for_test(&mut env.svm, &env.payer, wallets[0], env.mint);
    let admin_token = create_ata_for_test(&mut env.svm, &env.payer, wallets[3], env.mint);
    let empty_shared = env.svm.get_account(&shared).unwrap();
    let empty_vault = env.svm.get_account(&env.vault).unwrap();
    let empty_admin = env.svm.get_account(&admin_token).unwrap();
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &shared,
                &wallets[3],
                &[],
                SUPPLY,
            )
            .unwrap(),
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &wallets[3],
                &[],
            )
            .unwrap(),
        ],
        &[&admin],
    )
    .unwrap();
    for (asset, budgets) in DOMAIN_BUDGETS.into_iter().enumerate() {
        assert_eq!(budgets.iter().sum::<u64>(), CLAIMS[asset]);
        for (side, amount) in budgets.into_iter().enumerate() {
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: (2 * asset + side) as u16,
                    market_id: env.asset_market_id(asset as u16),
                    authority_epoch: env.control_sequences(asset).authority_epoch,
                    intent_id: side as u64 + 1,
                    amount: amount.into(),
                },
                vec![
                    AccountMeta::new(wallets[0], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(shared, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&a],
            )
            .unwrap();
        }
    }
    let ledgers: [Pubkey; 3] = std::array::from_fn(|_| {
        let key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            state::insurance_ledger_account_len(),
            env.program_id,
        );
        key.pubkey()
    });
    let empty_ledgers = ledgers.map(|key| env.svm.get_account(&key).unwrap());
    env.resolve();
    let initial = env.market_state();
    let controls = [0, 1].map(|asset| env.control_sequences(asset));
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let profiles =
        [0, 1].map(|asset| state::read_asset_oracle_profile(&market_frame.data, asset).unwrap());
    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    let mint = Mint::unpack(&mint_frame.data).unwrap();
    assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
    let seed = "row433-peer-custody";
    let replacement = Pubkey::create_with_seed(&env.payer.pubkey(), seed, &spl_token::ID).unwrap();
    assert_ne!(replacement, ata(wallets[0], env.mint));
    let rent = empty_shared.lamports;
    let creation = [
        system_instruction::create_account_with_seed(
            &env.payer.pubkey(),
            &replacement,
            &env.payer.pubkey(),
            seed,
            rent,
            TokenAccount::LEN as u64,
            &spl_token::ID,
        ),
        spl_token::instruction::initialize_account3(
            &spl_token::ID,
            &replacement,
            &env.mint,
            &wallets[0],
        )
        .unwrap(),
    ];
    let tracked: Vec<_> = [
        env.market,
        env.vault,
        env.mint,
        shared,
        replacement,
        admin_token,
    ]
    .into_iter()
    .chain(wallets)
    .chain(ledgers)
    .collect();
    let mut paid = [0; 2];
    for asset in 0..2 {
        let ix = payout(
            &env,
            asset,
            wallets[0],
            shared,
            ledgers[asset],
            PAID_PREFIX[asset],
            controls[asset].authority_epoch,
        );
        evidence.land(
            &mut env,
            &[ix],
            &[],
            &tracked,
            &[tracked[0], tracked[1], shared, ledgers[asset]],
            0,
            None,
        );
        paid[asset] = PAID_PREFIX[asset];
    }
    let tails = [0, 1].map(|asset| CLAIMS[asset] - PAID_PREFIX[asset]);
    let retained_peer = payout(
        &env,
        peer,
        wallets[0],
        shared,
        ledgers[peer],
        tails[peer],
        controls[peer].authority_epoch + 1,
    );
    let handoff = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(wallets[0], true),
            AccountMeta::new(wallets[1], true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: target as u16,
            market_id: env.asset_market_id(target as u16),
            authority_epoch: controls[target].authority_epoch + 1,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: wallets[1].to_bytes(),
        }
        .encode(),
    };
    let reassign = spl_token::instruction::set_authority(
        &spl_token::ID,
        &shared,
        Some(&wallets[1]),
        spl_token::instruction::AuthorityType::AccountOwner,
        &wallets[0],
        &[],
    )
    .unwrap();
    let target_payment = payout(
        &env,
        target,
        wallets[1],
        shared,
        ledgers[2],
        tails[target],
        controls[target].authority_epoch + 2,
    );
    // Succession and a real successor payout complete before the unchanged peer
    // request rejects the shared token account's new owner. Consent rolls back too.
    evidence.land(
        &mut env,
        &[
            handoff.clone(),
            reassign.clone(),
            target_payment.clone(),
            retained_peer.clone(),
        ],
        &[&a, &b],
        &tracked,
        &[],
        0,
        Some((5, PercolatorError::InvalidTokenAccount as u32, 2)),
    );
    evidence.land(
        &mut env,
        &[handoff, reassign],
        &[&a, &b],
        &tracked,
        &[tracked[0], shared],
        0,
        None,
    );
    drop((a, b, operator));
    let wallet_frames = wallets.map(|key| env.svm.get_account(&key));

    let check = |env: &V16CuEnv, paid: [u64; 2], created: bool| {
        let total_paid = paid.iter().sum::<u64>();
        let mut expected = initial.clone();
        expected.1.vault = (SUPPLY - total_paid).into();
        expected.1.insurance = expected.1.vault;
        expected.1.insurance_domain_budget_remaining_total = expected.1.insurance;
        for asset in 0..2 {
            let [long, short] = DOMAIN_BUDGETS[asset];
            expected.1.insurance_domain_budget[2 * asset] = long.saturating_sub(paid[asset]).into();
            expected.1.insurance_domain_budget[2 * asset + 1] =
                (short - paid[asset].saturating_sub(long)).into();
            let mut control = controls[asset];
            control.authority_epoch +=
                1 + u64::from(asset == target) + u64::from(paid[asset] == CLAIMS[asset]);
            assert_eq!(env.control_sequences(asset), control);
            let mut profile = profiles[asset];
            if asset == target {
                profile.insurance_authority = wallets[1].to_bytes();
            }
            assert_eq!(
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    asset
                )
                .unwrap(),
                profile
            );
        }
        assert_eq!(env.market_state(), expected);
        assert_eq!(expected.1.mode, MarketModeV16::Resolved);
        assert_eq!(
            (expected.1.c_tot, expected.1.materialized_portfolio_count),
            (0, 0)
        );
        assert!(expected
            .1
            .insurance_domain_spent
            .iter()
            .all(|value| *value == 0));
        let market = env.svm.get_account(&env.market).unwrap();
        assert_market_stock_census(
            "split beneficiary",
            &expected.1,
            &market.data,
            &[],
            (SUPPLY - total_paid).into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("split beneficiary", &expected.1, &[]).unwrap();
        assert_eq!(
            env.svm.get_account(&env.vault),
            Some(token_image(
                &empty_vault,
                env.vault_authority,
                SUPPLY - total_paid
            ))
        );
        assert_eq!(
            env.svm.get_account(&shared),
            Some(token_image(
                &empty_shared,
                wallets[1],
                PAID_PREFIX.iter().sum::<u64>() + paid[target] - PAID_PREFIX[target]
            ))
        );
        if created {
            assert_eq!(
                env.svm.get_account(&replacement),
                Some(token_image(
                    &empty_shared,
                    wallets[0],
                    paid[peer] - PAID_PREFIX[peer]
                ))
            );
        } else {
            assert_eq!(env.svm.get_account(&replacement), None);
        }
        assert_eq!(env.svm.get_account(&admin_token), Some(empty_admin.clone()));
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
        for index in 0..3 {
            let mut image = empty_ledgers[index].clone();
            let (owner, withdrawn, observed) = if index < 2 {
                let withdrawn = if index == peer {
                    paid[index]
                } else {
                    PAID_PREFIX[index]
                };
                (wallets[0], withdrawn, CLAIMS[index] - withdrawn)
            } else {
                let amount = paid[target] - PAID_PREFIX[target];
                if amount == 0 {
                    assert_eq!(env.svm.get_account(&ledgers[index]), Some(image));
                    continue;
                }
                (wallets[1], amount, CLAIMS[target] - paid[target])
            };
            state::init_insurance_ledger(
                &mut image.data,
                &state::InsuranceLedgerAccountV16 {
                    market_group: env.market.to_bytes(),
                    authority: owner.to_bytes(),
                    total_withdrawn_atoms: withdrawn.into(),
                    last_observed_insurance_atoms: observed.into(),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(env.svm.get_account(&ledgers[index]), Some(image));
        }
    };
    check(&env, paid, false);
    // The peer epoch and instruction bytes are unchanged by the target handoff.
    evidence.land(
        &mut env,
        &[retained_peer.clone()],
        &[],
        &tracked,
        &[],
        0,
        Some((2, PercolatorError::InvalidTokenAccount as u32, 0)),
    );
    check(&env, paid, false);
    let mut peer_payment = retained_peer;
    peer_payment.accounts[2].pubkey = replacement;
    let mut wrong_record = target_payment.clone();
    wrong_record.accounts[6].pubkey = ledgers[target];
    let batch = [creation.to_vec(), vec![peer_payment.clone(), wrong_record]].concat();
    evidence.land(
        &mut env,
        &batch,
        &[],
        &tracked,
        &[],
        0,
        Some((5, PercolatorError::Unauthorized as u32, 1)),
    );
    check(&env, paid, false);
    let mut stale_target = target_payment.clone();
    stale_target.data = ProgInstruction::WithdrawInsuranceAsset {
        asset_index: target as u16,
        market_id: env.asset_market_id(target as u16),
        authority_epoch: controls[target].authority_epoch + 1,
        amount: tails[target].into(),
    }
    .encode();
    let batch = [creation.to_vec(), vec![peer_payment.clone(), stale_target]].concat();
    evidence.land(
        &mut env,
        &batch,
        &[],
        &tracked,
        &[],
        0,
        Some((5, PercolatorError::EngineStale as u32, 1)),
    );
    check(&env, paid, false);

    let payments = if target_first {
        [target_payment, peer_payment]
    } else {
        [peer_payment, target_payment]
    };
    let order = if target_first {
        [target, peer]
    } else {
        [peer, target]
    };
    let first = [creation.to_vec(), vec![payments[0].clone()]].concat();
    let first_ledger = if order[0] == target {
        ledgers[2]
    } else {
        ledgers[peer]
    };
    evidence.land(
        &mut env,
        &first,
        &[],
        &tracked,
        &[tracked[0], tracked[1], shared, replacement, first_ledger],
        rent,
        None,
    );
    paid[order[0]] = CLAIMS[order[0]];
    check(&env, paid, true);
    // Positive peer stock remains liquid, but cannot finance a second debit of
    // the exhausted asset even with current authority and valid custody.
    let mut overclaim = payments[0].clone();
    overclaim.data = ProgInstruction::WithdrawInsuranceAsset {
        asset_index: order[0] as u16,
        market_id: env.asset_market_id(order[0] as u16),
        authority_epoch: env.control_sequences(order[0]).authority_epoch,
        amount: 1,
    }
    .encode();
    assert_eq!(env.token_amount(env.vault), tails[order[1]]);
    evidence.land(
        &mut env,
        &[overclaim],
        &[],
        &tracked,
        &[],
        0,
        Some((2, PercolatorError::EngineLockActive as u32, 0)),
    );
    check(&env, paid, true);
    let close = |epoch, signed| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(wallets[3], signed),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: epoch,
        }
        .encode(),
    };
    let epoch = env.control_sequences(0).authority_epoch;
    let premature = close(epoch, true);
    let final_epoch = epoch + u64::from(order[1] == 0);
    let final_close = close(final_epoch, true);
    let unsigned_close = close(final_epoch, false);
    evidence.land(
        &mut env,
        &[premature],
        &[&admin],
        &tracked,
        &[],
        0,
        Some((2, PercolatorError::EngineLockActive as u32, 0)),
    );
    check(&env, paid, true);
    let impossible = system_instruction::transfer(&env.payer.pubkey(), &wallets[3], u64::MAX);
    evidence.land(
        &mut env,
        &[payments[1].clone(), final_close.clone(), impossible],
        &[&admin],
        &tracked,
        &[],
        0,
        Some((4, 1, 2)),
    );
    check(&env, paid, true);
    let last_ledger = if order[1] == target {
        ledgers[2]
    } else {
        ledgers[peer]
    };
    evidence.land(
        &mut env,
        &[payments[1].clone()],
        &[],
        &tracked,
        &[tracked[0], tracked[1], shared, replacement, last_ledger],
        0,
        None,
    );
    paid[order[1]] = CLAIMS[order[1]];
    check(&env, paid, true);
    evidence.land(
        &mut env,
        &[unsigned_close],
        &[],
        &tracked,
        &[],
        0,
        Some((2, PercolatorError::ExpectedSigner as u32, 0)),
    );
    check(&env, paid, true);
    let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
    let vault_rent = env.svm.get_account(&env.vault).unwrap().lamports;
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut admin_after = env.svm.get_account(&wallets[3]).unwrap();
    admin_after.lamports += market_rent + vault_rent - tombstone_rent;
    evidence.land(
        &mut env,
        &[final_close],
        &[&admin],
        &tracked,
        &[tracked[0], tracked[1], wallets[3]],
        0,
        None,
    );
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|a| a.lamports == 0 && a.data.iter().all(|b| *b == 0)));
    assert_eq!(env.svm.get_account(&wallets[3]), Some(admin_after));
    assert_eq!(
        env.token_amount(shared),
        PAID_PREFIX.iter().sum::<u64>() + tails[target]
    );
    assert_eq!(env.token_amount(replacement), tails[peer]);
    assert_eq!(
        env.token_amount(shared) + env.token_amount(replacement),
        SUPPLY
    );
    eprintln!("row433 split custody: target={target}, target_first={target_first}, successor_custody={}, incumbent_custody={}, peak_CU={}", env.token_amount(shared), env.token_amount(replacement), evidence.peak);
}

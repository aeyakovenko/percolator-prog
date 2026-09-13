//! INV-073: bounded public reserve completion with unavailable signing keys and
//! generated wallet/expiry/payment schedules. INV-018/021 cover complete custody
//! images and rent; INV-026/027/063/067/070 cover principal, fee and expiry
//! disposition; INV-080/081 cover rollback and checked successful prefixes.
//! This one-domain solvent family does not establish generic terminal liveness,
//! receipt/pending-loss progress, recredit, or administrator-independent retirement.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use terminal_public_reserves::reserve_payout;
use terminal_reserve_destination_recovery::land;

const LIMIT: u64 = 600_000;

#[derive(Clone, Debug)]
struct ClaimBook {
    due: [u64; 3],
    paid: [u64; 3],
    expired: bool,
}

impl ClaimBook {
    fn remaining(&self) -> [u64; 3] {
        std::array::from_fn(|kind| {
            if kind == 0 && self.expired {
                0
            } else {
                self.due[kind] - self.paid[kind]
            }
        })
    }

    fn rank(&self) -> u64 {
        self.remaining().iter().sum()
    }

    fn custody(&self) -> u64 {
        self.due.iter().sum::<u64>() - self.paid.iter().sum::<u64>()
    }
}

fn token_image(
    initial: &solana_sdk::account::Account,
    amount: u64,
) -> solana_sdk::account::Account {
    let mut expected = initial.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    if token.is_native.is_some() {
        expected.lamports = expected.lamports - token.amount + amount;
    }
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

pub(crate) fn verify_generated_reserve_wallets() {
    let mut worlds = 0;
    let mut payments = 0;
    let mut repairs = 0;
    let mut normalizations = 0;
    let mut late_fees = 0;
    let mut depleted_with_fees = 0;
    let mut retirements = 0;
    let mut native_residue_endpoints = 0;
    let mut peak = 0;
    let mut outcomes = std::collections::BTreeMap::new();
    for seed in 0x4e00..0x4e04 {
        let mut rng = StdRng::seed_from_u64(seed);
        let share_bps = rng.gen_range(1_000..9_000);
        let insurance_fee = EARNINGS * u64::from(share_bps) / 10_000;
        let due = [BACKING, EARNINGS - insurance_fee, INSURANCE + insurance_fee];
        let prefix = due.map(|amount| rng.gen_range(1..amount));
        let mut word = [0, 0, 1, 1, 2];
        word.shuffle(&mut rng);
        // Keep an insurance tail after every expiry frontier so normalization
        // cannot also retire the market before the remaining payment schedule.
        let word: Vec<_> = word.into_iter().chain([2]).collect();
        for native in [false, true] {
            for absent_mask in 0..4 {
                for expiry_before in [0, 2, 4, usize::MAX] {
                    let (
                        TerminalEarningsWorld {
                            mut env,
                            admin,
                            incumbent,
                            successor,
                            mut wallets,
                            mut tokens,
                            portfolios,
                            mint_frame,
                        },
                        users,
                    ) = terminal_earnings_world_with_quote(true, None, share_bps, native);
                    drop(users);
                    let beneficiary = Keypair::new();
                    env.svm
                        .airdrop(&beneficiary.pubkey(), 1_000_000_000)
                        .unwrap();
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(&beneficiary),
                        0,
                        processor::ASSET_AUTH_INSURANCE,
                        beneficiary.pubkey().to_bytes(),
                    )
                    .unwrap();
                    let admin_token = tokens[4];
                    wallets[4] = beneficiary.pubkey();
                    tokens[4] = create_ata_for_test(&mut env.svm, &env.payer, wallets[4], env.mint);
                    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
                    let vault_frame = env.svm.get_account(&env.vault).unwrap();
                    let mut created = [true; 2];
                    for (index, (actor, owner)) in
                        [(2, &incumbent), (4, &beneficiary)].into_iter().enumerate()
                    {
                        if absent_mask & (1 << index) == 0 {
                            continue;
                        }
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::close_account(
                                &spl_token::ID,
                                &tokens[actor],
                                &wallets[actor],
                                &wallets[actor],
                                &[],
                            )
                            .unwrap(),
                            &[owner],
                        )
                        .unwrap();
                        let balance = env.svm.get_account(&wallets[actor]).unwrap().lamports;
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            system_instruction::transfer(
                                &wallets[actor],
                                &env.payer.pubkey(),
                                balance,
                            ),
                            &[owner],
                        )
                        .unwrap();
                        for key in [wallets[actor], tokens[actor]] {
                            assert!(env.svm.get_account(&key).is_none_or(|account| {
                                account.lamports == 0
                                    && account.data.is_empty()
                                    && account.owner == solana_sdk::system_program::ID
                                    && !account.executable
                            }));
                        }
                        created[index] = false;
                    }
                    let operator_balance = env.svm.get_account(&wallets[3]).unwrap().lamports;
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        system_instruction::transfer(
                            &wallets[3],
                            &env.payer.pubkey(),
                            operator_balance,
                        ),
                        &[&successor],
                    )
                    .unwrap();
                    drop((incumbent, successor, beneficiary));
                    assert!(!wallets.contains(&env.payer.pubkey()));
                    assert!(!wallets.contains(&admin.pubkey()));

                    let ledger = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &ledger,
                        state::backing_domain_ledger_account_len(),
                        env.program_id,
                    );
                    let ledger = ledger.pubkey();
                    let ledger_frame = env.svm.get_account(&ledger).unwrap();
                    let absent_custody =
                        [tokens[2], tokens[4]].map(|key| env.svm.get_account(&key));
                    let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
                    let market_frame = env.svm.get_account(&env.market).unwrap();
                    let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
                    let sequences = env.control_sequences(0);
                    let rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                    let tracked = [
                        env.market,
                        env.vault,
                        env.mint,
                        ledger,
                        admin.pubkey(),
                        admin_token,
                    ]
                    .into_iter()
                    .chain(wallets)
                    .chain(tokens)
                    .chain(portfolios)
                    .collect::<Vec<_>>();
                    let close = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new(admin_token, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(env.mint, false),
                        ],
                        data: ProgInstruction::CloseSlab {
                            authority_epoch: sequences.authority_epoch,
                        }
                        .encode(),
                    };
                    let mut unsigned_close = close.clone();
                    unsigned_close.accounts[0].is_signer = false;
                    let check = |env: &V16CuEnv, book: &ClaimBook, created: [bool; 2]| {
                        let group = env.market_state().1;
                        let remaining = book.remaining();
                        assert_eq!(group.mode, MarketModeV16::Resolved);
                        assert_eq!(
                            (
                                group.c_tot,
                                group.pnl_pos_tot,
                                group.materialized_portfolio_count
                            ),
                            (0, 0, 0)
                        );
                        assert_eq!(group.source_claim_bound_total_num, 0);
                        assert_eq!(group.vault, u128::from(book.custody()));
                        assert_eq!(group.insurance, u128::from(remaining[2]));
                        assert_eq!(
                            group.insurance_domain_budget_remaining_total,
                            group.insurance
                        );
                        assert_eq!(
                            group.backing_provider_earnings_total,
                            u128::from(remaining[1])
                        );
                        let bucket = group.source_backing_buckets[1];
                        let source = group.source_credit[1];
                        assert_eq!(bucket.utilization_fee_earnings, u128::from(remaining[1]));
                        assert_eq!(
                            bucket.fresh_unliened_backing_num,
                            u128::from(remaining[0]) * BOUND_SCALE
                        );
                        assert_eq!(
                            source.fresh_reserved_backing_num,
                            bucket.fresh_unliened_backing_num
                        );
                        assert_eq!(bucket.valid_liened_backing_num, 0);
                        assert_eq!(
                            bucket.consumed_liened_backing_num,
                            u128::from(PROFIT) * BOUND_SCALE
                        );
                        assert_eq!(
                            source.provider_receivable_num,
                            u128::from(PROFIT) * BOUND_SCALE
                        );
                        assert_eq!(source.spent_backing_num, u128::from(PROFIT) * BOUND_SCALE);
                        assert_eq!(
                            bucket.status,
                            if remaining[0] == 0 {
                                BackingBucketStatusV16::Expired
                            } else {
                                BackingBucketStatusV16::Fresh
                            }
                        );
                        assert_eq!(
                            env.svm.get_account(&env.vault),
                            Some(token_image(&vault_frame, book.custody()))
                        );
                        let amounts = [
                            PAYOUTS[0],
                            PAYOUTS[1],
                            book.paid[0] + book.paid[1],
                            0,
                            book.paid[2],
                        ];
                        assert_eq!(amounts.iter().sum::<u64>() + book.custody(), SUPPLY);
                        for actor in 0..5 {
                            let role = if actor == 2 {
                                Some(0)
                            } else if actor == 4 {
                                Some(1)
                            } else {
                                None
                            };
                            let expected = if role.is_some_and(|index| !created[index]) {
                                assert_eq!(amounts[actor], 0);
                                absent_custody[role.unwrap()].clone()
                            } else {
                                Some(token_image(&token_frames[actor], amounts[actor]))
                            };
                            assert_eq!(env.svm.get_account(&tokens[actor]), expected);
                        }
                        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
                        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                        assert_eq!(env.token_amount(admin_token), 0);
                        let market = env.svm.get_account(&env.market).unwrap();
                        assert_eq!(market.lamports, market_frame.lamports);
                        assert_eq!(
                            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                            profile
                        );
                        assert_eq!(env.control_sequences(0), sequences);
                        assert_market_stock_census(
                            "generated reserve wallets",
                            &group,
                            &market.data,
                            &[],
                            book.custody().into(),
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census(
                            "generated reserve wallets",
                            &group,
                            &[],
                        )
                        .unwrap();
                        state::market_view_mut(&mut market.data.clone())
                            .unwrap()
                            .1
                            .validate_shape()
                            .unwrap();
                        let account = env.svm.get_account(&ledger).unwrap();
                        if book.paid[1] == 0 {
                            assert_eq!(account, ledger_frame);
                        } else {
                            let record = state::read_backing_domain_ledger(&account.data).unwrap();
                            assert_eq!(record.market_group, env.market.to_bytes());
                            assert_eq!(record.authority, wallets[2].to_bytes());
                            assert_eq!(
                                record.total_earnings_withdrawn_atoms,
                                u128::from(book.paid[1])
                            );
                            assert_eq!(
                                record.last_observed_bucket_earnings_atoms,
                                u128::from(remaining[1])
                            );
                            assert_eq!(account.lamports, ledger_frame.lamports);
                        }
                    };
                    let mut book = ClaimBook {
                        due,
                        paid: [0; 3],
                        expired: false,
                    };
                    let mut seen = [false; 3];
                    check(&env, &book, created);
                    for (step, &kind) in word.iter().enumerate() {
                        if step == expiry_before {
                            env.svm.warp_to_slot(100 + seed % 2);
                            if book.remaining()[0] > 0 {
                                let before_rank = book.rank();
                                let market_key = env.market;
                                peak = peak.max(land(
                                    &mut env,
                                    &[close.clone()],
                                    &[&admin],
                                    &tracked,
                                    &[market_key],
                                    0,
                                    None,
                                    None,
                                ));
                                book.expired = true;
                                assert!(book.rank() < before_rank);
                                normalizations += 1;
                                check(&env, &book, created);
                            }
                        }
                        if kind == 0 && book.expired {
                            continue;
                        }
                        let amount = if seen[kind] {
                            due[kind] - prefix[kind]
                        } else {
                            prefix[kind]
                        };
                        seen[kind] = true;
                        let role = usize::from(kind == 2);
                        let actor = if kind == 2 { 4 } else { 2 };
                        let mut batch = Vec::new();
                        if !created[role] {
                            batch.push(Instruction {
                                program_id: associated_token_program_id(),
                                accounts: vec![
                                    AccountMeta::new(env.payer.pubkey(), true),
                                    AccountMeta::new(tokens[actor], false),
                                    AccountMeta::new_readonly(wallets[actor], false),
                                    AccountMeta::new_readonly(env.mint, false),
                                    AccountMeta::new_readonly(
                                        solana_sdk::system_program::ID,
                                        false,
                                    ),
                                    AccountMeta::new_readonly(spl_token::ID, false),
                                ],
                                data: vec![1],
                            });
                        }
                        let payout = reserve_payout(&env, wallets, tokens, ledger, kind, amount);
                        assert!(payout.accounts.iter().all(|meta| !meta.is_signer));
                        batch.push(payout);
                        let mut rejected = batch.clone();
                        rejected.push(unsigned_close.clone());
                        peak = peak.max(land(
                            &mut env,
                            &rejected,
                            &[],
                            &tracked,
                            &[],
                            0,
                            None,
                            Some((2 + batch.len() as u8, PercolatorError::ExpectedSigner)),
                        ));
                        check(&env, &book, created);
                        let mut allowed = vec![env.market, env.vault, tokens[actor]];
                        if kind == 1 {
                            allowed.push(ledger);
                        }
                        peak = peak.max(land(
                            &mut env,
                            &batch,
                            &[],
                            &tracked,
                            &allowed,
                            if created[role] { 0 } else { rent },
                            None,
                            None,
                        ));
                        repairs += usize::from(!created[role]);
                        created[role] = true;
                        let before_rank = book.rank();
                        book.paid[kind] += amount;
                        assert_eq!(before_rank - book.rank(), amount);
                        payments += 1;
                        late_fees += usize::from(kind == 1 && book.expired);
                        depleted_with_fees +=
                            usize::from(book.remaining()[0] == 0 && book.remaining()[1] > 0);
                        check(&env, &book, created);
                    }
                    assert_eq!(book.rank(), 0);
                    assert_eq!(book.paid[1..], due[1..]);
                    let key = (seed, expiry_before);
                    if let Some(expected) = outcomes.insert(key, book.paid) {
                        assert_eq!(
                            book.paid, expected,
                            "rail and wallet availability cannot alter entitlement"
                        );
                    }
                    worlds += 1;
                    // Like the existing quote-capacity owner, this probe excludes
                    // native principal burning. All beneficiary claims are paid;
                    // the exact expired raw residue remains checked in custody.
                    if native && book.custody() > 0 {
                        assert!(book.expired);
                        assert_eq!(book.custody(), BACKING - book.paid[0]);
                        native_residue_endpoints += 1;
                        continue;
                    }
                    let final_tokens = tokens.map(|key| env.svm.get_account(&key));
                    let final_ledger = env.svm.get_account(&ledger);
                    let tombstone_rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                    let refund = market_frame.lamports
                        + env.svm.get_account(&env.vault).unwrap().lamports
                        - tombstone_rent;
                    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                    expected_admin.lamports += refund;
                    let allowed = [env.market, env.vault, env.mint];
                    peak = peak.max(land(
                        &mut env,
                        &[close],
                        &[&admin],
                        &tracked,
                        &allowed,
                        0,
                        Some((admin.pubkey(), refund)),
                        None,
                    ));
                    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                    let tombstone = env.svm.get_account(&env.market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(tombstone.lamports, tombstone_rent);
                    assert!(env
                        .svm
                        .get_account(&env.vault)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                    assert_eq!(tokens.map(|key| env.svm.get_account(&key)), final_tokens);
                    assert_eq!(env.svm.get_account(&ledger), final_ledger);
                    assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
                    assert_eq!(env.token_amount(admin_token), 0);
                    let mut expected_mint = mint_frame;
                    if !native {
                        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                        mint.supply -= book.custody();
                        Mint::pack(mint, &mut expected_mint.data).unwrap();
                    }
                    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
                    retirements += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 128);
    assert_eq!(retirements + native_residue_endpoints, worlds);
    assert!(native_residue_endpoints > 0 && retirements > 64);
    assert!(
        payments > 512
            && repairs > 0
            && normalizations > 0
            && late_fees > 0
            && depleted_with_fees > 0
    );
    assert_cu_within("INV-073 generated reserve wallet matrix", peak, LIMIT);
    eprintln!("INV-073 generated reserve wallets: worlds={worlds}, payments={payments}, exact_rollbacks={payments}, repairs={repairs}, normalizations={normalizations}, late_fees={late_fees}, depleted_with_fees={depleted_with_fees}, retirements={retirements}, native_residue_endpoints={native_residue_endpoints}, peak={peak} CU");
}

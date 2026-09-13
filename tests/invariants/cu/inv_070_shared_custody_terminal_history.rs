//! Row 418: owner-attributed terminal value across shared custody incarnations.
//! Generated sibling portfolios share two payout ATAs. Accumulating payments and
//! disposing/recreating custody between claims must deliver the same owner value.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use rand::{seq::SliceRandom, Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const LIMIT: u64 = 500_000;
const ENTRY: u64 = 100;
const SUFFIX_ERROR: u32 = system_instruction::SystemError::ResultWithNegativeLamports as u32;

#[derive(Debug)]
struct History {
    capital: Vec<u64>,
    lots: Vec<u64>,
    movement: u64,
    winner: usize,
    surplus: u64,
    order: Vec<usize>,
}

impl History {
    fn generate(seed: u64, pairs: usize) -> Self {
        let mut rng = XorShiftRng::seed_from_u64(seed);
        let winner = (seed % 2) as usize;
        let mut order = Vec::new();
        // Both losing cohorts settle before winners; each owner's sibling order varies.
        for owner in [1 - winner, winner] {
            let mut siblings: Vec<_> = (0..pairs).map(|pair| 2 * pair + owner).collect();
            siblings.shuffle(&mut rng);
            order.extend(siblings);
        }
        Self {
            capital: (0..2 * pairs)
                .map(|_| rng.gen_range(10_000..30_000))
                .collect(),
            lots: (0..pairs).map(|_| rng.gen_range(1..10)).collect(),
            movement: rng.gen_range(1..5),
            winner,
            surplus: rng.gen_range(1..100),
            order,
        }
    }

    fn entitlement(&self, i: usize) -> u64 {
        let pnl = self.lots[i / 2] * self.movement;
        if i % 2 == self.winner {
            self.capital[i] + pnl
        } else {
            self.capital[i] - pnl
        }
    }
}

#[derive(Default)]
struct Evidence {
    commits: usize,
    rollbacks: usize,
    successful_aborted_wrappers: usize,
    payments: usize,
    detach_only: usize,
    custody_closes: usize,
    recreations: usize,
    peak: u64,
}

fn step(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rent_paid: u64,
    rejection: Option<(usize, u32, usize)>,
    evidence: &mut Evidence,
) {
    env.svm.expire_blockhash();
    let mut instructions = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    instructions.extend_from_slice(ixs);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        signing.len()
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = signing.len() as u64 * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let frame: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let lamports_before: u128 = frame.iter().flatten().map(|a| u128::from(a.lamports)).sum();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, code, successes)) = rejection {
        let failed = result.expect_err("terminal prerequisite or aborted suffix");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError((index + 2) as u8, InstructionError::Custom(code)),
            "logs={:?}",
            failed.meta.logs
        );
        assert_eq!(
            failed
                .meta
                .logs
                .iter()
                .filter(|line| { **line == format!("Program {} success", env.program_id) })
                .count(),
            successes,
            "successful wrapper prefixes must execute before rollback"
        );
        evidence.rollbacks += 1;
        evidence.successful_aborted_wrappers += successes;
        failed.meta
    } else {
        evidence.commits += 1;
        result.expect("bounded public terminal continuation")
    };
    for (key, mut expected) in keys.iter().zip(frame) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -=
                fee + if rejection.is_none() { rent_paid } else { 0 };
        } else if rejection.is_none() && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame {key}"
        );
    }
    let lamports_after: u128 = keys
        .iter()
        .filter_map(|key| env.svm.get_account(key))
        .map(|a| u128::from(a.lamports))
        .sum();
    assert_eq!(lamports_after + u128::from(fee), lamports_before);
    assert_cu_within(
        "shared terminal custody",
        meta.compute_units_consumed,
        LIMIT,
    );
    evidence.peak = evidence.peak.max(meta.compute_units_consumed);
}

fn absent(env: &V16CuEnv, key: Pubkey) {
    assert!(env
        .svm
        .get_account(&key)
        .is_none_or(|a| { a.lamports == 0 && a.data.iter().all(|byte| *byte == 0) }));
}

fn token_frame(env: &V16CuEnv, key: Pubkey, initial: &Account, amount: u64, native: bool) {
    let mut expected = initial.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.state, AccountState::Initialized);
    assert_eq!(token.delegate, COption::None);
    assert_eq!(token.close_authority, COption::None);
    assert_eq!(
        token.is_native,
        if native {
            COption::Some(initial.lamports)
        } else {
            COption::None
        }
    );
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    if native {
        expected.lamports += amount;
    }
    // Packing the oracle's clone never writes a LiteSVM account.
    assert_eq!(
        env.svm.get_account(&key),
        Some(expected),
        "token/lamport disposition {key}"
    );
}

fn run(
    history: &History,
    decimals: Option<u8>,
    recycle: bool,
    evidence: &mut Evidence,
) -> [u64; 2] {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let native = decimals.is_none();
    let mut env = decimals.map_or_else(inv081_public_native_market, inv018_public_spl_market);
    let admin = env.admin.insecure_clone();
    let owners = [Keypair::new(), Keypair::new()];
    let owner_keys = owners.each_ref().map(Signer::pubkey);
    let destinations =
        owner_keys.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
    let empty_destinations = destinations.map(|key| env.svm.get_account(&key).unwrap());
    let empty_vault = env.svm.get_account(&env.vault).unwrap();
    let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    let empty_admin_token = env.svm.get_account(&admin_token).unwrap();
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let banks = [Keypair::new(), Keypair::new()];
    let bank_keys = banks.each_ref().map(Signer::pubkey);
    for owner in 0..2 {
        env.svm.airdrop(&owner_keys[owner], 1_000_000_000).unwrap();
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::create_account(
                    &env.payer.pubkey(),
                    &bank_keys[owner],
                    rent,
                    TokenAccount::LEN as u64,
                    &spl_token::ID,
                ),
                spl_token::instruction::initialize_account3(
                    &spl_token::ID,
                    &bank_keys[owner],
                    &env.mint,
                    &owner_keys[owner],
                )
                .unwrap(),
            ],
            &[&banks[owner]],
        )
        .unwrap();
    }
    let empty_banks = bank_keys.map(|key| env.svm.get_account(&key).unwrap());
    for owner in 0..3 {
        let (destination, amount, funder) = if owner == 2 {
            (admin_token, history.surplus, &admin)
        } else {
            (
                destinations[owner],
                history
                    .capital
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % 2 == owner)
                    .map(|(_, amount)| *amount)
                    .sum(),
                &owners[owner],
            )
        };
        let funding = if native {
            vec![
                system_instruction::transfer(&funder.pubkey(), &destination, amount),
                spl_token::instruction::sync_native(&spl_token::ID, &destination).unwrap(),
            ]
        } else {
            vec![spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &destination,
                &admin.pubkey(),
                &[],
                amount,
            )
            .unwrap()]
        };
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            funding,
            &[if native { funder } else { &admin }],
        )
        .unwrap();
    }
    if !native {
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
    }
    let mut portfolios = Vec::new();
    for (i, amount) in history.capital.iter().enumerate() {
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            env.program_id,
        );
        let key = portfolio.pubkey();
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner_keys[i % 2], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key, false),
            ],
            &[&owners[i % 2]],
        )
        .unwrap();
        env.portfolios.push(key);
        portfolios.push(key);
        env.send(
            env.deposit_ix(key, (*amount).into()),
            vec![
                AccountMeta::new(owner_keys[i % 2], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key, false),
                AccountMeta::new(destinations[i % 2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[i % 2]],
        )
        .unwrap();
    }
    env.configure_auth_mark_for_asset_as_admin(0, 0, ENTRY);
    env.configure_permissionless_resolve_with_cu(100, 3);
    for (pair, lots) in history.lots.iter().enumerate() {
        env.trade_with_cu(
            &owners[0],
            portfolios[2 * pair],
            &owners[1],
            portfolios[2 * pair + 1],
            i128::from(*lots) * POS_SCALE as i128,
            ENTRY,
            0,
        );
    }
    env.svm.warp_to_slot(1);
    let exit = if history.winner == 0 {
        ENTRY + history.movement
    } else {
        ENTRY - history.movement
    };
    env.push_auth_mark_with_cu(1, exit);
    env.crank(
        portfolios[1 - history.winner],
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    evidence.peak = evidence.peak.max(env.resolve());
    env.svm.warp_to_slot(4);
    let claims_frame: Vec<_> = std::iter::once(env.market)
        .chain(portfolios.iter().copied())
        .map(|key| env.svm.get_account(&key))
        .collect();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &admin_token,
            &env.vault,
            &admin.pubkey(),
            &[],
            history.surplus,
        )
        .unwrap(),
        &[&admin],
    )
    .unwrap();
    assert_eq!(
        std::iter::once(env.market)
            .chain(portfolios.iter().copied())
            .map(|key| env.svm.get_account(&key))
            .collect::<Vec<_>>(),
        claims_frame
    );
    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    if let Some(decimals) = decimals {
        let mint = Mint::unpack(&mint_frame.data).unwrap();
        assert_eq!(mint.decimals, decimals);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(
            mint.supply,
            history.capital.iter().sum::<u64>() + history.surplus
        );
    }
    let owner_frame = owner_keys.map(|key| env.svm.get_account(&key).unwrap());
    let mut tracked = vec![
        env.market,
        env.vault,
        env.mint,
        env.vault_authority,
        admin_token,
        admin.pubkey(),
    ];
    tracked.extend(owner_keys);
    tracked.extend(destinations);
    tracked.extend(bank_keys);
    tracked.extend_from_slice(&portfolios);
    let payouts: Vec<_> = portfolios
        .iter()
        .enumerate()
        .map(|(i, portfolio)| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(owner_keys[i % 2], false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(*portfolio, false),
                AccountMeta::new(destinations[i % 2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: if (i / 2 % 2 == 0) == recycle {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            } else {
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: vec![],
                }
            }
            .encode(),
        })
        .collect();
    let create = |owner: usize| Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(destinations[owner], false),
            AccountMeta::new_readonly(owner_keys[owner], false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
        ],
        data: vec![],
    };
    let creations = [create(0), create(1)];
    let suffix = system_instruction::transfer(&env.payer.pubkey(), &admin.pubkey(), u64::MAX);
    let slab = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        }
        .encode(),
    };
    let mut paid = vec![false; portfolios.len()];
    let mut detached = vec![false; portfolios.len()];
    let mut deleted = vec![false; portfolios.len()];
    let mut archived = [0u64; 2];
    let mut custody = [0u64; 2];
    let mut closes = [0u64; 2];
    let mut present = [true; 2];
    let mut prior: [Option<usize>; 2] = [None; 2];
    let check = |env: &V16CuEnv,
                 paid: &[bool],
                 detached: &[bool],
                 deleted: &[bool],
                 archived: [u64; 2],
                 custody: [u64; 2],
                 closes: [u64; 2],
                 present: [bool; 2]| {
        let market = env.svm.get_account(&env.market).unwrap();
        let (cfg, group) = env.market_state();
        let remaining: u64 = (0..paid.len())
            .filter(|i| !paid[*i])
            .map(|i| history.entitlement(i))
            .sum();
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(cfg.terminal_slab_scan_progress, 0);
        assert_eq!(group.assets[0].effective_price, exit);
        assert_eq!(group.vault, remaining.into());
        assert_eq!(
            (group.insurance, group.backing_provider_earnings_total),
            (0, 0)
        );
        assert_eq!(
            group.materialized_portfolio_count as usize,
            deleted.iter().filter(|d| !**d).count()
        );
        let ps: Vec<_> = portfolios
            .iter()
            .enumerate()
            .filter(|(i, _)| !deleted[*i])
            .map(|(_, key)| env.portfolio_state(*key))
            .collect();
        assert_market_stock_census(
            "shared custody terminal ledger",
            &group,
            &market.data,
            &ps,
            remaining.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("shared custody terminal ledger", &group, &ps)
            .unwrap();
        for owner in 0..2 {
            let expected_paid: u64 = (owner..paid.len())
                .step_by(2)
                .filter(|i| paid[*i])
                .map(|i| history.entitlement(i))
                .sum();
            assert_eq!(archived[owner] + custody[owner], expected_paid);
            if present[owner] {
                token_frame(
                    env,
                    destinations[owner],
                    &empty_destinations[owner],
                    custody[owner],
                    native,
                );
            } else {
                assert_eq!(custody[owner], 0);
                absent(env, destinations[owner]);
            }
            token_frame(
                env,
                bank_keys[owner],
                &empty_banks[owner],
                if native { 0 } else { archived[owner] },
                native,
            );
            let mut wallet = owner_frame[owner].clone();
            wallet.lamports += closes[owner] * rent + if native { archived[owner] } else { 0 };
            assert_eq!(env.svm.get_account(&owner_keys[owner]), Some(wallet));
            let oi: u128 = (owner..paid.len())
                .step_by(2)
                .filter(|i| !detached[*i])
                .map(|i| u128::from(history.lots[i / 2]) * POS_SCALE)
                .sum();
            assert_eq!(
                if owner == 0 {
                    group.assets[0].oi_eff_long_q
                } else {
                    group.assets[0].oi_eff_short_q
                },
                oi
            );
        }
        for (i, portfolio) in portfolios.iter().enumerate() {
            if deleted[i] {
                absent(env, *portfolio);
            } else {
                assert_eq!(resolved_portfolio_is_terminal(env, *portfolio), paid[i]);
                if detached[i] && !paid[i] {
                    let account = env.portfolio_state(*portfolio);
                    assert_eq!(account.capital.get(), history.capital[i].into());
                    assert_eq!(
                        account.pnl.get(),
                        i128::from(history.lots[i / 2] * history.movement)
                    );
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                }
            }
        }
        token_frame(
            env,
            env.vault,
            &empty_vault,
            remaining + history.surplus,
            native,
        );
        token_frame(env, admin_token, &empty_admin_token, 0, native);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        let mut data = market.data;
        let (_, view) = state::market_view_mut(&mut data).unwrap();
        view.validate_shape().unwrap();
        for (i, key) in portfolios.iter().enumerate() {
            if !deleted[i] {
                let mut data = env.svm.get_account(key).unwrap().data;
                state::portfolio_view_mut_for_market_slots(&mut data, 1)
                    .unwrap()
                    .validate_with_market(&view.as_view())
                    .unwrap();
            }
        }
    };
    check(
        &env, &paid, &detached, &deleted, archived, custody, closes, present,
    );
    step(
        &mut env,
        &[slab.clone()],
        &[&admin],
        &tracked,
        &[],
        0,
        Some((0, PercolatorError::EngineLockActive as u32, 0)),
        evidence,
    );
    // One fresh leg per portfolio: the first pass detaches every leg. Earlier
    // winners then receive their exact claims in a second, finite pass.
    let mut schedule = history.order.clone();
    schedule.extend(
        history
            .order
            .iter()
            .copied()
            .filter(|i| i % 2 == history.winner)
            .take(history.lots.len() - 1),
    );
    for &i in &schedule {
        let owner = i % 2;
        let mut prefix = Vec::new();
        let needs_creation = !present[owner];
        if needs_creation {
            prefix.push(creations[owner].clone());
            let previous = prior[owner].expect("a disposed destination has an older paid claim");
            let mut replay = prefix.clone();
            replay.push(payouts[previous].clone());
            step(
                &mut env,
                &replay,
                &[],
                &tracked,
                &[],
                0,
                Some((1, PercolatorError::EngineNonProgress as u32, 0)),
                evidence,
            );
            check(
                &env, &paid, &detached, &deleted, archived, custody, closes, present,
            );
        }
        prefix.push(payouts[i].clone());
        let mut aborted = prefix.clone();
        aborted.push(suffix.clone());
        step(
            &mut env,
            &aborted,
            &[],
            &tracked,
            &[],
            0,
            Some((prefix.len(), SUFFIX_ERROR, 1)),
            evidence,
        );
        check(
            &env, &paid, &detached, &deleted, archived, custody, closes, present,
        );
        let changed = [env.market, portfolios[i], env.vault, destinations[owner]];
        step(
            &mut env,
            &prefix,
            &[],
            &tracked,
            &changed,
            if needs_creation { rent } else { 0 },
            None,
            evidence,
        );
        evidence.recreations += usize::from(needs_creation);
        detached[i] = true;
        let pays = owner != history.winner || detached.iter().all(|d| *d);
        if pays {
            evidence.payments += 1;
            paid[i] = true;
            custody[owner] += history.entitlement(i);
            prior[owner] = Some(i);
        } else {
            evidence.detach_only += 1;
        }
        present[owner] = true;
        check(
            &env, &paid, &detached, &deleted, archived, custody, closes, present,
        );
        if recycle && pays {
            dispose(
                &mut env,
                owner,
                native,
                &owners,
                destinations,
                bank_keys,
                &tracked,
                &mut custody,
                &mut archived,
                &mut closes,
                &mut present,
                evidence,
            );
            check(
                &env, &paid, &detached, &deleted, archived, custody, closes, present,
            );
        }
    }
    if !recycle {
        for owner in 0..2 {
            dispose(
                &mut env,
                owner,
                native,
                &owners,
                destinations,
                bank_keys,
                &tracked,
                &mut custody,
                &mut archived,
                &mut closes,
                &mut present,
                evidence,
            );
            check(
                &env, &paid, &detached, &deleted, archived, custody, closes, present,
            );
        }
    }
    for owner in 0..2 {
        step(
            &mut env,
            &[
                creations[owner].clone(),
                payouts[prior[owner].unwrap()].clone(),
            ],
            &[],
            &tracked,
            &[],
            0,
            Some((1, PercolatorError::EngineNonProgress as u32, 0)),
            evidence,
        );
        check(
            &env, &paid, &detached, &deleted, archived, custody, closes, present,
        );
    }
    let mut deletion_order = history.order.clone();
    if recycle {
        deletion_order.reverse();
    }
    for (position, &i) in deletion_order.iter().enumerate() {
        let deletion = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[i], false),
            ],
            data: env.close_portfolio_ix(portfolios[i]).encode(),
        };
        let final_close = position + 1 == portfolios.len();
        let mut prefix = vec![deletion];
        if final_close {
            prefix.push(slab.clone());
        }
        let mut aborted = prefix.clone();
        aborted.push(suffix.clone());
        step(
            &mut env,
            &aborted,
            &[&admin],
            &tracked,
            &[],
            0,
            Some((prefix.len(), SUFFIX_ERROR, prefix.len())),
            evidence,
        );
        check(
            &env, &paid, &detached, &deleted, archived, custody, closes, present,
        );
        let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
        let portfolio_rent = env.svm.get_account(&portfolios[i]).unwrap().lamports;
        let market_before = env.svm.get_account(&env.market).unwrap();
        let changed = [
            env.market,
            portfolios[i],
            admin.pubkey(),
            env.vault,
            admin_token,
        ];
        step(
            &mut env,
            &prefix,
            &[&admin],
            &tracked,
            &changed,
            0,
            None,
            evidence,
        );
        deleted[i] = true;
        let mut expected_admin = admin_before;
        if final_close {
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            expected_admin.lamports +=
                portfolio_rent + market_before.lamports + rent - tombstone.lamports;
            absent(&env, env.vault);
            for portfolio in &portfolios {
                absent(&env, *portfolio);
            }
            token_frame(
                &env,
                admin_token,
                &empty_admin_token,
                history.surplus,
                native,
            );
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        } else {
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_before.lamports + portfolio_rent
            );
            check(
                &env, &paid, &detached, &deleted, archived, custody, closes, present,
            );
        }
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    }
    assert_eq!(
        archived.iter().sum::<u64>(),
        history.capital.iter().sum::<u64>()
    );
    assert_cu_within("shared terminal history peak", evidence.peak, LIMIT);
    archived
}

#[allow(clippy::too_many_arguments)]
fn dispose(
    env: &mut V16CuEnv,
    owner: usize,
    native: bool,
    owners: &[Keypair; 2],
    destinations: [Pubkey; 2],
    banks: [Pubkey; 2],
    tracked: &[Pubkey],
    custody: &mut [u64; 2],
    archived: &mut [u64; 2],
    closes: &mut [u64; 2],
    present: &mut [bool; 2],
    evidence: &mut Evidence,
) {
    let mut ixs = Vec::new();
    if !native {
        ixs.push(
            spl_token::instruction::transfer(
                &spl_token::ID,
                &destinations[owner],
                &banks[owner],
                &owners[owner].pubkey(),
                &[],
                custody[owner],
            )
            .unwrap(),
        );
    }
    ixs.push(
        spl_token::instruction::close_account(
            &spl_token::ID,
            &destinations[owner],
            &owners[owner].pubkey(),
            &owners[owner].pubkey(),
            &[],
        )
        .unwrap(),
    );
    step(
        env,
        &ixs,
        &[&owners[owner]],
        tracked,
        &[destinations[owner], banks[owner], owners[owner].pubkey()],
        0,
        None,
        evidence,
    );
    archived[owner] += custody[owner];
    custody[owner] = 0;
    closes[owner] += 1;
    present[owner] = false;
    evidence.custody_closes += 1;
}

#[test]
fn v16_program_generated_shared_custody_recreation_preserves_terminal_owner_value() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    let mut winning_sides = 0;
    for (seed, pairs) in [(418, 2), (418_070, 3), (418_025, 4), (418_081, 3)] {
        let history = History::generate(seed, pairs);
        winning_sides |= 1 << history.winner;
        let expected: [u64; 2] = std::array::from_fn(|owner| {
            (owner..history.capital.len())
                .step_by(2)
                .map(|i| history.entitlement(i))
                .sum()
        });
        eprintln!("row418 generated seed={seed}, {history:?}");
        for decimals in [Some(0), Some(6), Some(9), None] {
            for recycle in [false, true] {
                let outcome = run(&history, decimals, recycle, &mut evidence);
                assert_eq!(
                    outcome, expected,
                    "seed={seed}, decimals={decimals:?}, recycle={recycle}"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 32);
    assert_eq!(winning_sides, 3);
    assert_eq!(evidence.payments, 192);
    assert_eq!(evidence.detach_only, 64);
    assert_eq!(evidence.recreations, 64);
    assert_eq!(evidence.custody_closes, 128);
    assert_eq!(evidence.rollbacks, 608);
    assert_eq!(evidence.successful_aborted_wrappers, 480);
    println!("row418 shared custody: worlds={worlds}, payments={}, detach_only={}, custody_closes={}, recreations={}, rollbacks={}, aborted_wrapper_successes={}, commits={}, peak_CU={}, limit={LIMIT}",
        evidence.payments, evidence.detach_only, evidence.custody_closes, evidence.recreations, evidence.rollbacks,
        evidence.successful_aborted_wrappers, evidence.commits, evidence.peak);
}

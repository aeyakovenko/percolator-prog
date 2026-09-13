//! INV-070 / row 424: generated source deadlines across persisted scan boundaries.
//! The input-owned oracle covers Fresh/Expired actionability and external custody.
//! Insurance recredit, new earlier obligations and arbitrary invalidation remain open.

use super::*;
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, RngAlgorithm, TestRng, TestRunner},
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CHUNK: usize = percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL;
const LIMIT: u64 = 900_000;
const SETUP_SLOT: u64 = 1_000;

#[derive(Clone, Debug)]
struct History {
    first: usize,
    gap: usize,
    amounts: [u64; 4],
    deadlines: [u64; 4],
    surplus: u64,
    group: usize,
    late: u64,
}

impl History {
    fn domains(&self) -> [usize; 4] {
        [
            2 * self.first,
            2 * self.first + 1,
            2 * (self.first + self.gap),
            2 * (self.first + self.gap) + 1,
        ]
    }

    fn slots(&self) -> usize {
        self.first + self.gap + 2
    }

    fn backing(&self) -> u64 {
        self.amounts.iter().sum()
    }

    fn deadline(&self, source: usize) -> u64 {
        SETUP_SLOT + self.deadlines[source]
    }
}

#[derive(Clone, Debug)]
struct Oracle {
    expired: [bool; 4],
    cursor: usize,
    engine_slot: u64,
    donated: bool,
    closed: bool,
}

impl Oracle {
    fn new() -> Self {
        Self {
            expired: [false; 4],
            cursor: 0,
            engine_slot: SETUP_SLOT + 10,
            donated: false,
            closed: false,
        }
    }

    fn rank(&self, history: &History) -> (usize, usize) {
        (
            self.expired.iter().filter(|expired| !**expired).count(),
            history.slots() - self.cursor,
        )
    }

    // A source table, built solely from public funding inputs, determines the next
    // actionable asset. A later overdue source cannot bypass an earlier live asset.
    fn advance(&mut self, history: &History, clock: u64) -> bool {
        assert!(!self.closed);
        let domains = history.domains();
        assert!((0..4).all(|i| self.expired[i] || domains[i] / 2 >= self.cursor));
        let end = (self.cursor + CHUNK).min(history.slots());
        let first = (0..4).find(|i| !self.expired[*i] && domains[*i] / 2 < end);
        if let Some(first) = first {
            let asset = domains[first] / 2;
            let due = (0..4).find(|i| {
                !self.expired[*i] && domains[*i] / 2 == asset && history.deadline(*i) <= clock
            });
            if let Some(due) = due {
                self.expired[due] = true;
            } else if asset == self.cursor {
                return false;
            }
            self.cursor = asset;
        } else if end < history.slots() {
            self.cursor = end;
        } else {
            assert!(self.expired.iter().all(|expired| *expired));
            self.closed = true;
        }
        self.engine_slot = clock;
        true
    }

    fn check(&self, world: &World, history: &History) {
        assert!(!self.closed);
        let env = &world.env;
        let (cfg, group) = env.market_state();
        let market = env.svm.get_account(&env.market).unwrap();
        let header = market_group_header_bytes(&market.data);
        let domains = history.domains();
        let mut fresh = 0;
        for (domain, (bucket, source)) in group
            .source_backing_buckets
            .iter()
            .zip(&group.source_credit)
            .enumerate()
        {
            let index = domains.iter().position(|value| *value == domain);
            let amount = index
                .filter(|i| !self.expired[*i])
                .map_or(0, |i| u128::from(history.amounts[i]) * BOUND_SCALE);
            assert_eq!(
                bucket.status,
                index.map_or(BackingBucketStatusV16::Empty, |i| if self.expired[i] {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                })
            );
            if let Some(i) = index {
                assert_eq!(bucket.expiry_slot, history.deadline(i));
            }
            assert_eq!(bucket.fresh_unliened_backing_num, amount);
            assert_eq!(source.fresh_reserved_backing_num, amount);
            assert_eq!(
                (
                    bucket.valid_liened_backing_num,
                    bucket.utilization_fee_earnings,
                    source.valid_liened_backing_num,
                    source.positive_claim_bound_num,
                    source.provider_receivable_num
                ),
                (0, 0, 0, 0, 0)
            );
            fresh += amount;
        }
        assert_eq!(cfg.terminal_slab_scan_progress, self.cursor as u128);
        assert_eq!(group.current_slot, self.engine_slot);
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.insurance,
                group.materialized_portfolio_count
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert!(group
            .insurance_domain_budget
            .iter()
            .all(|amount| *amount == 0));
        assert!(group
            .insurance_domain_spent
            .iter()
            .all(|amount| *amount == 0));
        assert!(group
            .assets
            .iter()
            .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
        assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
        assert_eq!(header.insurance_domain_budget_remaining_total.get(), 0);
        let expired: u64 = (0..4)
            .filter(|i| self.expired[*i])
            .map(|i| history.amounts[i])
            .sum();
        assert_eq!(group.vault, u128::from(history.backing()));
        assert_eq!(group.vault - fresh / BOUND_SCALE, u128::from(expired));
        let external = if self.donated { history.surplus } else { 0 };
        assert_eq!(env.token_amount(env.vault), history.backing() + external);
        assert_eq!(
            env.token_amount(world.donor_token),
            history.surplus - external
        );
        assert_eq!(env.token_amount(world.destination), 0);
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, history.backing() + history.surplus);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.freeze_authority, COption::None);
        let decoded_rank = (
            group
                .source_backing_buckets
                .iter()
                .filter(|bucket| bucket.status == BackingBucketStatusV16::Fresh)
                .count(),
            history.slots() - cfg.terminal_slab_scan_progress as usize,
        );
        assert_eq!(decoded_rank, self.rank(history));
        assert!(
            group.source_backing_buckets[..2 * self.cursor]
                .iter()
                .all(|bucket| bucket.status != BackingBucketStatusV16::Fresh),
            "cached prefixes must exclude every time-sensitive source, including overdue siblings"
        );
        crate::support::fuzz_model::assert_market_stock_census(
            "generated prefix actionability",
            &group,
            &market.data,
            &[],
            group.vault,
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "generated prefix actionability",
            &group,
            &[],
        )
        .unwrap();
    }
}

struct World {
    env: V16CuEnv,
    donor: Keypair,
    donor_token: Pubkey,
    destination: Pubkey,
}

impl World {
    fn new(history: &History) -> Self {
        use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;
        let mut env = inv018_public_spl_market_with_capacity(
            0,
            V16CuMarketParams::default(),
            history.slots(),
        );
        let admin = env.admin.insecure_clone();
        for asset in 1..history.slots() {
            env.activate_asset(asset as u16, asset as u64 + 1, 100);
        }
        let donor = Keypair::new();
        env.svm.airdrop(&donor.pubkey(), 1_000_000_000).unwrap();
        let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let donor_token = create_ata_for_test(&mut env.svm, &env.payer, donor.pubkey(), env.mint);
        for (token, amount) in [
            (destination, history.backing()),
            (donor_token, history.surplus),
        ] {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &admin.pubkey(),
                    &[],
                    amount,
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
        env.svm.warp_to_slot(SETUP_SLOT);
        for (i, domain) in history.domains().into_iter().enumerate() {
            env.top_up_backing_bucket_from_admin_token_with_cu(
                destination,
                domain as u16,
                history.amounts[i].into(),
                history.deadline(i),
            );
        }
        env.svm.warp_to_slot(SETUP_SLOT + 10);
        env.resolve();
        Self {
            env,
            donor,
            donor_token,
            destination,
        }
    }

    fn close(&self) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.env.admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new(self.destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(self.env.mint, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: self.env.control_sequences(0).authority_epoch,
            }
            .encode(),
        }
    }

    fn land(
        &mut self,
        ixs: &[Instruction],
        donor: bool,
        allowed: &[Pubkey],
        rejection: Option<(usize, InstructionError)>,
        successes: (usize, usize),
        evidence: &mut Evidence,
    ) {
        let env = &mut self.env;
        env.svm.expire_blockhash();
        let mut instructions = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        ];
        instructions.extend_from_slice(ixs);
        let mut signers = vec![&env.payer];
        if ixs.iter().any(|ix| {
            ix.accounts
                .iter()
                .any(|account| account.pubkey == env.admin.pubkey() && account.is_signer)
        }) {
            signers.push(&env.admin);
        }
        if donor {
            signers.push(&self.donor);
        }
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&env.payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let mut keys = tx.message.account_keys.clone();
        keys.extend([
            env.market,
            env.vault,
            env.mint,
            self.destination,
            self.donor_token,
            self.donor.pubkey(),
            solana_sdk::sysvar::clock::ID,
        ]);
        keys.sort_unstable();
        keys.dedup();
        let mut before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let payer = keys
            .iter()
            .position(|key| *key == env.payer.pubkey())
            .unwrap();
        before[payer].as_mut().unwrap().lamports -=
            u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
        let result = env.svm.send_transaction(tx);
        let meta = if let Some((index, error)) = rejection {
            assert!(allowed.is_empty());
            let failure = result.expect_err("expected atomic refusal");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index as u8, error)
            );
            evidence.rollbacks += 1;
            evidence.prefix_rollbacks += usize::from(successes.0 > 0);
            evidence.custody_rollbacks += usize::from(successes.1 > 0);
            failure.meta
        } else {
            evidence.commits += 1;
            result.expect("generated public continuation")
        };
        for (key, account) in keys.iter().zip(before) {
            if !allowed.contains(key) {
                assert_eq!(
                    env.svm.get_account(key),
                    account,
                    "complete Account frame: {key}"
                );
            }
        }
        for (program, count) in [(env.program_id, successes.0), (spl_token::ID, successes.1)] {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count
            );
        }
        assert_cu_within(
            "generated prefix transaction",
            meta.compute_units_consumed,
            LIMIT,
        );
        evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
    }
}

#[derive(Default)]
struct Evidence {
    worlds: usize,
    commits: usize,
    rollbacks: usize,
    prefix_rollbacks: usize,
    custody_rollbacks: usize,
    peak_cu: u64,
}

fn drive(
    world: &mut World,
    history: &History,
    oracle: &mut Oracle,
    group: usize,
    evidence: &mut Evidence,
) {
    let clock = world.env.svm.get_sysvar::<Clock>().slot;
    let close = world.close();
    for _ in 0..(history.slots().div_ceil(CHUNK) + 10) {
        let mut next = oracle.clone();
        let mut count = 0;
        let mut waits = false;
        for _ in 0..group {
            let rank = next.rank(history);
            if !next.advance(history, clock) {
                waits = true;
                break;
            }
            count += 1;
            if next.closed {
                break;
            }
            assert!(
                next.rank(history) < rank,
                "every planned scan reduces source/prefix rank"
            );
        }
        let instructions = vec![close.clone(); count];
        let mut rejected = instructions.clone();
        let error = if waits {
            rejected.push(close.clone());
            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
        } else {
            rejected.push(Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![],
            });
            InstructionError::InvalidInstructionData
        };
        let spl = if next.closed { 3 } else { 0 };
        world.land(
            &rejected,
            false,
            &[],
            Some((2 + count, error)),
            (count, spl),
            evidence,
        );
        oracle.check(world, history);
        if count == 0 {
            return;
        }
        let market = world.env.svm.get_account(&world.env.market).unwrap();
        let vault = world.env.svm.get_account(&world.env.vault).unwrap();
        let mut admin = world
            .env
            .svm
            .get_account(&world.env.admin.pubkey())
            .unwrap();
        let mut expected_mint = world.env.svm.get_account(&world.env.mint).unwrap();
        let allowed = if next.closed {
            vec![
                world.env.market,
                world.env.vault,
                world.env.mint,
                world.destination,
                world.env.admin.pubkey(),
            ]
        } else {
            vec![world.env.market]
        };
        world.land(&instructions, false, &allowed, None, (count, spl), evidence);
        if next.closed {
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                world
                    .env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            admin.lamports += market.lamports - tombstone.lamports + vault.lamports;
            assert_eq!(
                world.env.svm.get_account(&world.env.admin.pubkey()),
                Some(admin)
            );
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= history.backing();
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(
                world.env.svm.get_account(&world.env.mint),
                Some(expected_mint)
            );
            assert_eq!(world.env.token_amount(world.destination), history.surplus);
            assert_eq!(world.env.token_amount(world.donor_token), 0);
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(
                    |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
                ));
        } else {
            next.check(world, history);
            let start =
                MARKET_GROUP_OFF + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
            let end = start
                + oracle.cursor
                    * std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
            assert_eq!(
                &world.env.svm.get_account(&world.env.market).unwrap().data[start..end],
                &market.data[start..end],
                "previously scanned slots remain exact"
            );
        }
        *oracle = next;
        if oracle.closed || waits {
            return;
        }
    }
    panic!("finite source/cursor rank did not reach a wait or terminal state");
}

fn run(history: &History, overdue: bool, evidence: &mut Evidence) -> (u64, u64) {
    let mut world = World::new(history);
    let mut oracle = Oracle::new();
    oracle.check(&world, history);
    drive(&mut world, history, &mut oracle, 1, evidence);
    assert_eq!(oracle.cursor, history.first);
    assert!(!oracle.expired.iter().any(|expired| *expired));
    let donation = spl_token::instruction::transfer(
        &spl_token::ID,
        &world.donor_token,
        &world.env.vault,
        &world.donor.pubkey(),
        &[],
        history.surplus,
    )
    .unwrap();
    let close = world.close();
    world.land(
        &[donation.clone(), close.clone()],
        true,
        &[],
        Some((
            3,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        )),
        (0, 1),
        evidence,
    );
    oracle.check(&world, history);
    world.land(
        &[donation],
        true,
        &[world.donor_token, world.env.vault],
        None,
        (0, 1),
        evidence,
    );
    oracle.donated = true;
    oracle.check(&world, history);
    let mut clocks = if overdue {
        vec![*history.deadlines.iter().max().unwrap() + history.late]
    } else {
        history
            .deadlines
            .iter()
            .flat_map(|deadline| [deadline - 1, *deadline, deadline + history.late])
            .collect::<Vec<_>>()
    };
    clocks.sort_unstable();
    clocks.dedup();
    for clock in clocks.into_iter().map(|slot| SETUP_SLOT + slot) {
        let before = world.env.svm.get_account(&world.env.market);
        world.env.svm.warp_to_slot(clock);
        assert_eq!(
            world.env.svm.get_account(&world.env.market),
            before,
            "Clock reclassifies eligibility without rewriting persisted state"
        );
        oracle.check(&world, history);
        drive(
            &mut world,
            history,
            &mut oracle,
            if overdue { history.group } else { 1 },
            evidence,
        );
        if oracle.closed {
            break;
        }
    }
    assert!(
        oracle.closed,
        "all source deadlines passed; bounded cleanup must terminate"
    );
    evidence.worlds += 1;
    (
        Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
            .unwrap()
            .supply,
        world.env.token_amount(world.destination),
    )
}

#[test]
fn v16_program_generated_scans_recompute_actionability_across_source_deadlines_and_prefixes() {
    let evidence = std::cell::RefCell::new(Evidence::default());
    let verify = |history: History| {
        let mut evidence = evidence.borrow_mut();
        let cadence = run(&history, false, &mut evidence);
        let overdue = run(&history, true, &mut evidence);
        assert_eq!(
            cadence, overdue,
            "clock cadence / transaction partition: {history:?}"
        );
        assert_eq!(cadence, (history.surplus, history.surplus));
    };
    for (first, gap, deadlines) in [
        (1, 1, [24, 20, 21, 20]),
        (CHUNK - 1, CHUNK + 1, [20; 4]),
        (CHUNK + 1, CHUNK, [20, 22, 24, 21]),
    ] {
        verify(History {
            first,
            gap,
            amounts: [17, 31, 43, 59],
            deadlines,
            surplus: 19,
            group: 3,
            late: 1,
        });
    }
    let strategy = (
        prop::sample::select(vec![1usize, CHUNK - 1, CHUNK, CHUNK + 1]),
        prop::sample::select(vec![1usize, CHUNK - 1, CHUNK, CHUNK + 1]),
        prop::array::uniform4(1u64..=257),
        prop::array::uniform4(20u64..=29),
        1u64..=101,
        1usize..=3,
        0u64..=3,
    )
        .prop_map(
            |(first, gap, amounts, deadlines, surplus, group, late)| History {
                first,
                gap,
                amounts,
                deadlines,
                surplus,
                group,
                late,
            },
        );
    TestRunner::new_with_rng(
        Config {
            cases: 24,
            max_shrink_iters: 64,
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/inv_070_generated_prefix_actionability.txt",
            ))),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x70; 32]),
    )
    .run(&strategy, |history| {
        verify(history);
        Ok(())
    })
    .unwrap();
    let evidence = evidence.into_inner();
    assert!(evidence.prefix_rollbacks > 0 && evidence.custody_rollbacks > 0);
    println!(
        "INV-070 row424: {} worlds, {} commits, {} rollbacks ({} scanner prefixes, \
        {} custody prefixes), peak {} CU",
        evidence.worlds,
        evidence.commits,
        evidence.rollbacks,
        evidence.prefix_rollbacks,
        evidence.custody_rollbacks,
        evidence.peak_cu
    );
}

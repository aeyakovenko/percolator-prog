//! INV-070/088: bind terminal-scan discovery metadata to its actual obligation.
//! The generic INV-079 gates own runnable evidence and mounts. A different
//! executable INV-070 test cannot replace the time-dependent prefix witness.

const FINDINGS: &str = include_str!("../open_findings.tsv");
const REOPENINGS: &str = include_str!("../coverage_reopenings.tsv");
const DISCOVERIES: &str = include_str!("../independent_discoveries.tsv");

fn row424(tsv: &str, width: usize) -> Result<Vec<&str>, &'static str> {
    let rows: Vec<Vec<_>> = tsv
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect())
        .filter(|fields: &Vec<_>| fields[0] == "424")
        .collect();
    if rows.len() != 1 || rows[0].len() != width {
        return Err("row 424 must have exactly one entry with the expected schema");
    }
    Ok(rows.into_iter().next().unwrap())
}

fn validate_scan_evidence(
    findings: &str,
    reopenings: &str,
    discoveries: &str,
) -> Result<(), &'static str> {
    let finding = row424(findings, 6)?;
    let reopening = row424(reopenings, 8)?;
    if finding[1..5] != ["LoF", "PRIVILEGED", "INV-070", "independent-discovery"]
        || reopening[1..4] != finding[1..4]
    {
        return Err("row 424 must retain its reviewed terminal-scan classification");
    }
    for invariant in [
        "INV-063", "INV-069", "INV-070", "INV-071", "INV-086", "INV-088",
    ] {
        if !reopening[4].split(',').any(|id| id == invariant) {
            return Err("row 424 lost an affected lifecycle or summary invariant");
        }
    }
    if reopening[5] != "persisted-cursor+x-time-reclassification+x-earlier-slot" {
        return Err("row 424 must retain the cursor/time/earlier-slot product");
    }
    if reopening[6]
        != "any-environmental-transition-that-changes-actionability-invalidates-scanned-prefixes"
    {
        return Err("row 424 must retain general actionability invalidation");
    }
    if reopening[7] != "OPEN" {
        return Err("bounded expiry evidence cannot close all environmental histories");
    }

    let expected = [
        "INV-070",
        "terminal-scan/backing-expiry-prefix",
        "v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry",
        "terminal-scan-must-rediscover-earlier-actionable-reserves-after-expiry",
    ];
    let retained = discoveries
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .any(|fields| {
            fields.len() == 5
                && fields[4].split(',').any(|id| id == "424")
                && fields[..4] == expected
        });
    if !retained {
        return Err("row 424 lost its scan-restart fingerprint, selector, or oracle");
    }
    Ok(())
}

#[test]
fn v16_row424_metadata_retains_terminal_scan_invalidation_evidence() {
    validate_scan_evidence(FINDINGS, REOPENINGS, DISCOVERIES).unwrap();
}

#[test]
fn v16_row424_metadata_guard_rejects_unrelated_or_narrowed_evidence() {
    validate_scan_evidence(FINDINGS, REOPENINGS, DISCOVERIES).unwrap();
    let cases = [
        (
            2,
            "v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry",
            "v16_program_close_slab_rejects_until_market_has_zero_terminal_residue",
            "row 424 lost its scan-restart fingerprint, selector, or oracle",
        ),
        (
            2,
            "terminal-scan/backing-expiry-prefix",
            "terminal-scan/empty-market-close",
            "row 424 lost its scan-restart fingerprint, selector, or oracle",
        ),
        (
            2,
            "terminal-scan-must-rediscover-earlier-actionable-reserves-after-expiry",
            "terminal-close-must-reject-while-funded",
            "row 424 lost its scan-restart fingerprint, selector, or oracle",
        ),
        (
            1,
            "persisted-cursor+x-time-reclassification+x-earlier-slot",
            "persisted-cursor+x-empty-market+x-close-slab",
            "row 424 must retain the cursor/time/earlier-slot product",
        ),
        (
            1,
            "any-environmental-transition-that-changes-actionability-invalidates-scanned-prefixes",
            "backing-expiry-restarts-the-terminal-scan",
            "row 424 must retain general actionability invalidation",
        ),
        (
            1,
            "INV-071,INV-086,INV-088\tpersisted-cursor",
            "INV-071,INV-086\tpersisted-cursor",
            "row 424 lost an affected lifecycle or summary invariant",
        ),
    ];
    for (index, from, to, expected_error) in cases {
        let mut ledgers = [
            FINDINGS.to_owned(),
            REOPENINGS.to_owned(),
            DISCOVERIES.to_owned(),
        ];
        assert_eq!(
            ledgers[index].matches(from).count(),
            1,
            "unique mutation target"
        );
        ledgers[index] = ledgers[index].replacen(from, to, 1);
        assert_eq!(
            validate_scan_evidence(&ledgers[0], &ledgers[1], &ledgers[2]),
            Err(expected_error),
            "undetected metadata substitution: {from} -> {to}"
        );
    }
    eprintln!(
        "INV-070/088 row 424: {} metadata mutations rejected",
        cases.len()
    );
}

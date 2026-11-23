//! New nano (8 x 8) games are pinned: the outcome of `reset` (the whole exported state, or the
//! refusal) for a spread of seeds, map types and rosters hashes to the digest recorded here, as
//! `reset_digest.rs` pins the game's own sizes. A pin against change only.
use engine_fast::reset::{reset, Config, NANO};
use engine_fast::tables::Tables;
use sha2::{Digest, Sha256};

const CATALOG: &str = include_str!("../rules/rules-124.json");

/// 144 configurations: 6 seeds on each of the six map types and four rosters of two to four
/// seats. Re-pinned when Continents and Pangea came in: with their resets put
/// back to the old refusal the old digest 4d... is reproduced, so the other four map
/// types are unchanged.
fn configs() -> Vec<Config> {
    let rosters: [&[i32]; 4] = [&[7, 7], &[7, 7, 7, 7], &[2, 7], &[6, 8, 9, 10]];
    let mut out = Vec::new();
    for map in 1..=6 {
        for (r, tribes) in rosters.iter().enumerate() {
            for seed in 1..=6 {
                out.push(Config {
                    rules: 124,
                    seed: seed * 7919 + map * 101 + r as i32,
                    size: NANO,
                    map,
                    mode: 4,
                    tribes: tribes.to_vec(),
                    action_limit: 1000,
                });
            }
        }
    }
    out
}

#[test]
fn new_nano_games_are_unchanged() {
    let t = Tables::compile(CATALOG).unwrap();
    let mut hash = Sha256::new();
    let mut started = 0;
    for c in configs() {
        match reset(&c, CATALOG, &t) {
            Ok(g) => {
                assert_eq!(g.width, NANO);
                started += 1;
                hash.update(serde_json::to_vec(&g.export()).unwrap());
            }
            Err(refusal) => hash.update(format!("refused: {refusal}")),
        }
    }
    assert!(started >= 90, "{started} games started");
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        digest, "ae42929a2650632bb70e18e368a7be784b18d369d25b89edd67c9f5c2a1e3025",
        "{started} games started"
    );
}

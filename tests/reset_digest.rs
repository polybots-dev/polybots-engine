//! New games are pinned: the outcome of `reset` (the whole exported state, or the refusal)
//! for a spread of seeds, board sides, seat counts, tribes and map types hashes to the
//! digest recorded here. Most of a reset is the map generator, so a change to a generated
//! map in these configurations changes the digest; so does anything else in a new game's
//! state, including the rule data file it carries.
//!
//! This is a pin against change, not a comparison with the game. One digest covers every
//! game, so a failure does not say which configuration changed.

use engine_fast::reset::{reset, Config};
use engine_fast::tables::Tables;
use sha2::{Digest, Sha256};

const CATALOG: &str = include_str!("../rules/rules-124.json");

/// 108 configurations per map type: 6 seeds on each of three board sides and six rosters (two
/// to four seats; one tribe repeated, which draws replacement colours, and mixed tribes).
fn configs(maps: &[i32]) -> Vec<Config> {
    let rosters: [&[i32]; 6] = [
        &[7, 7],
        &[7, 7, 7],
        &[7, 7, 7, 7],
        &[2, 7],
        &[3, 4, 5],
        &[6, 8, 9, 10],
    ];
    let mut out = Vec::new();
    for size in [11, 14, 16] {
        for &map in maps {
            for (r, tribes) in rosters.iter().enumerate() {
                for seed in 1..=6 {
                    out.push(Config {
                        rules: 124,
                        seed: seed * 7919 + size * 31 + map * 101 + r as i32,
                        size,
                        map,
                        mode: 4,
                        tribes: tribes.to_vec(),
                        action_limit: 1000,
                    });
                }
            }
        }
    }
    out
}

/// The digest of every outcome, and how many games started.
fn digest(maps: &[i32]) -> (String, usize) {
    let t = Tables::compile(CATALOG).unwrap();
    let mut hash = Sha256::new();
    let mut started = 0;
    for c in configs(maps) {
        match reset(&c, CATALOG, &t) {
            Ok(g) => {
                started += 1;
                hash.update(serde_json::to_vec(&g.export()).unwrap());
            }
            Err(refusal) => hash.update(format!("refused: {refusal}")),
        }
    }
    let digest = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    (digest, started)
}

#[test]
fn new_games_are_unchanged() {
    // Dryland, Lakes, Archipelago, Water World
    let (digest, started) = digest(&[1, 2, 4, 5]);
    // most of the pinned outcomes are games, not refusals
    assert!(started >= 300, "{started} games started");
    assert_eq!(
        digest, "5e66cd2f0618de5111d61aedfd5ac9c44ad5bb56424f7a7a222974a4cea83a09",
        "{started} games started"
    );
}

/// The island generator (Continents, Pangea), pinned with the map generator
/// source from before it was made faster.
#[test]
fn new_island_games_are_unchanged() {
    let (digest, started) = digest(&[3, 6]);
    assert!(started >= 200, "{started} games started");
    assert_eq!(
        digest, "5d4e8d9fbf1ce4e16f07dba51c22a955acb28bf69a4a72c16092938d400c1907",
        "{started} games started"
    );
}

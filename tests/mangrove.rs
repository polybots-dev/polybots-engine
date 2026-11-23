//! A board with mangrove, which no rule makes and the move search does not model, is refused
//! when the game is imported.

use engine_fast::reset::{self, Config};
use engine_fast::state::Game;
use engine_fast::tables::Tables;

const CATALOG: &str = include_str!("../rules/rules-124.json");

#[test]
fn a_board_with_mangrove_is_refused_at_import() {
    let t = Tables::compile(CATALOG).unwrap();
    let config = Config {
        rules: 124,
        seed: 3,
        size: 11,
        map: 2,
        mode: 4,
        tribes: vec![7, 7],
        action_limit: 100,
    };
    let g = reset::reset(&config, CATALOG, &t).unwrap();
    let mut checkpoint = g.export();
    assert!(Game::import(&checkpoint).is_ok());
    checkpoint["game"]["state"]["board"]["tiles"][5]["terrain"] = serde_json::json!(8);
    assert_eq!(
        Game::import(&checkpoint).err().as_deref(),
        Some("mangrove terrain not implemented")
    );
}

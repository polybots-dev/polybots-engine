//! Every rule field of a game as one list of numbers, written from the Rust engine's `Game`
//! in the order `kernel/dump.h` writes it from a device block. Comparing the two checks the
//! block's layout field by field, by name on both sides, without going through this crate's
//! mirror structs.

use crate::state::EfGame;
use engine_fast::state::{Game, Player, Tile, Unit, Xy};
use std::collections::HashMap;

struct Dump(Vec<i64>);

impl Dump {
    fn put(&mut self, v: impl Into<i64>) {
        self.0.push(v.into());
    }
    fn flag(&mut self, v: bool) {
        self.0.push(i64::from(v));
    }
    fn xy(&mut self, c: Xy) {
        self.put(c.x);
        self.put(c.y);
    }
    fn list(&mut self, items: &[i32]) {
        self.put(items.len() as i64);
        self.0.extend(items.iter().map(|&v| i64::from(v)));
    }
    fn option(&mut self, v: Option<i64>) {
        self.flag(v.is_some());
        self.put(v.unwrap_or(0));
    }
}

fn player(d: &mut Dump, p: &Player) {
    d.put(p.action.id);
    d.put(p.action.currency);
    d.flag(p.action.opinions_dirty);
    d.put(p.action.tasks.len() as i64);
    for t in p.action.tasks.iter() {
        d.put(t.kind.0);
        d.flag(t.flags.started);
        d.flag(t.flags.completed);
        d.option(t.peaceful_turns);
        d.option(t.kills);
        d.option(t.conversions);
        d.option(t.explored_tiles);
    }
    d.put(p.tribe);
    d.list(&p.available_tech);
    d.list(&p.known_players);
    d.put(p.score);
    d.put(p.cities);
    d.put(p.kills);
    d.put(p.casualties);
    d.put(p.relations.len() as i64);
    for (id, r) in p.relations.iter() {
        d.put(*id);
        d.put(r.state);
        d.put(r.last_attack_turn);
        d.put(r.embassy_level);
        d.put(r.last_peace_broken_turn);
        d.put(r.first_meet);
        d.put(r.embassy_build_turn);
        d.put(r.previous_attack_turn);
    }
    d.flag(p.unlocked_tech_cache.is_some());
    d.list(p.unlocked_tech_cache.as_deref().unwrap_or(&[]));
    d.xy(p.start_tile);
    d.put(p.handicap);
    d.flag(p.auto_play);
    d.put(p.resigned_at_command);
    d.put(p.wiped_at_command);
    d.put(p.messages.len() as i64);
    for m in p.messages.iter() {
        d.put(m.kind);
        d.put(m.sender);
    }
    d.list(&p.built_unique_improvements);
    d.flag(p.block_train_units);
    d.put(p.skin);
    d.put(p.end_score);
    d.put(p.color);
    d.put(p.tribe_mix);
    d.put(p.climate);
    d.flag(p.has_chosen_tribe);
    d.put(p.resigned_turn);
    d.put(p.wipe_outs);
    d.put(p.killer_id);
    d.put(p.killed_turn);
    d.put(p.capital_count_cache);
}

fn unit(d: &mut Dump, g: &Game, u: &Unit, list_number: i64) {
    d.put(u.id);
    d.put(u.leader);
    d.put(u.follower);
    d.put(u.owner);
    d.put(u.kind);
    d.xy(u.previous_turn_end_coordinates);
    d.xy(u.coordinates);
    d.xy(u.home);
    d.put(u.health);
    d.put(u.promotion_level);
    d.put(u.xp);
    d.flag(u.moved);
    d.flag(u.attacked);
    d.put(u.direction);
    d.flag(u.flipped);
    d.put(u.created_turn);
    d.put(list_number);
    let effects = &g.effect_lists[u.effects as usize];
    d.put(effects.len() as i64);
    d.0.extend(effects.iter().copied());
    d.flag(u.is_temporary_explorer);
    d.put(u.birth_climate);
    d.put(u.birth_climate_skin);
}

fn tile(d: &mut Dump, g: &Game, t: &Tile, lists: &mut HashMap<u32, i64>) {
    d.xy(t.coordinates);
    d.put(t.terrain);
    d.put(t.climate);
    d.put(t.altitude);
    d.put(t.owner);
    d.put(t.capital_of);
    d.list(&t.explorers);
    d.xy(t.ruling_city);
    d.flag(t.improvement.is_some());
    if let Some(m) = &t.improvement {
        d.put(m.kind);
        d.put(m.owner);
        d.put(m.founder);
        d.put(m.level);
        d.put(m.founded);
        d.put(m.xp);
        d.put(m.population);
        d.put(m.production);
        d.put(m.base_score);
        d.put(m.border_size);
        d.put(m.upgrade);
        d.put(m.connected_to_capital_of_player);
        d.flag(m.named);
        for kind in 0..14 {
            d.put(m.rewards.count(kind) as i64);
        }
        d.list(&m.effects);
    }
    d.option(t.resource.map(i64::from));
    d.flag(t.has_road);
    d.flag(t.has_route);
    d.option(t.continent.map(i64::from));
    d.flag(t.had_route);
    d.put(t.last_population_check);
    d.put(t.available_population);
    d.put(t.skin);
    d.list(&t.effects);
    let mut node = t.unit;
    while let Some(n) = node {
        let u = &g.units[n as usize];
        // effect lists are numbered in the order the tiles reach them from 1; 0 is the device's
        // reserved empty list, which no unit owns
        let next = lists.len() as i64 + 1;
        let number = *lists.entry(u.effects).or_insert(next);
        d.put(1);
        unit(d, g, u, number);
        node = u.passenger;
    }
    d.put(0);
}

/// The game's rule fields in `kernel/dump.h`'s order.
pub fn of_game(g: &Game) -> Vec<i64> {
    let mut d = Dump(Vec::new());
    let f = &g.fixed;
    d.put(g.width);
    d.put(g.tiles.len() as i64);
    d.put(g.players.len() as i64);
    d.put(f.seats as i64);
    d.flag(f.win_by_capital);
    d.flag(f.win_by_extermination);
    d.flag(f.achievements_active);
    d.put(match f.naming_case.as_str() {
        "Common" => 0,
        "Turkic" => 1,
        _ => 2,
    });
    d.put(f.version);
    d.put(f.rules_mode);
    d.put(f.game_type);
    d.put(f.turn_limit);
    d.put(f.score_limit);
    d.put(f.death_condition);
    d.put(f.action_limit as i64);
    d.put(g.current_turn);
    d.put(g.current_player_index);
    d.put(g.current_unit_id);
    d.flag(g.needs_routes);
    d.put(g.current_state);
    d.put(g.current_command as i64);
    d.put(g.decisions as i64);
    d.put(g.commands as i64);
    d.put(g.seeds.gameplay_seed);
    d.put(g.seeds.village_name_seed);
    d.option(g.seeds.random_hash.map(|h| h.seed));
    d.option(g.seeds.village_name_hash.map(|h| h.seed));
    d.put(g.naming_seeds.len() as i64);
    d.0.extend(g.naming_seeds.iter().copied());
    d.put(g.triggers.len() as i64);
    for t in &g.triggers {
        d.put(t.player);
        d.put(t.opponent);
        d.put(t.kind);
        d.xy(t.coordinates);
    }
    d.put(g.actions.len() as i64);
    for a in &g.actions {
        // the record's fields, as the C side reads them; a variant the device cannot hold
        // shows as one -1
        match crate::action::to_device(a) {
            Ok(r) => crate::action::dump(&r, &mut d.0),
            Err(_) => d.put(-1),
        }
    }
    for p in &g.players {
        player(&mut d, p);
    }
    let mut lists = HashMap::new();
    for t in &g.tiles {
        tile(&mut d, g, t, &mut lists);
    }
    d.0
}

extern "C" {
    fn ef_twin_game_dump(g: *const EfGame, out: *mut i64, capacity: i32) -> i32;
}

/// The block's rule fields as the host build reads them.
pub fn of_block(g: &crate::state::Block) -> Vec<i64> {
    let mut out = vec![0i64; 1 << 17];
    // SAFETY: the C side writes at most `capacity` values and returns how many there are.
    let count =
        unsafe { ef_twin_game_dump(g.as_ptr(), out.as_mut_ptr(), out.len() as i32) } as usize;
    assert!(count <= out.len(), "the dump buffer is too small");
    out.truncate(count);
    out
}

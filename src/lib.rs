//! engine-fast: a fast, exact rules engine for The Battle of Polytopia, for training bots.
//!
//! One game is a [`state::Game`], moved forward one decision at a time:
//! - [`reset::reset`] starts a game from a configuration and a seed;
//! - [`menu::menu`] lists the legal moves of the player to move;
//! - [`step::advance`] applies one of them;
//! - [`view::write`] and [`queries`] give what the player to move may know.
//!
//! The rule data is compiled once into [`tables::Tables`]. The engine refuses what it does
//! not implement, with the rule's name, rather than approximating it. ARCHITECTURE.md at
//! the top of the repository maps every module.

pub mod action;
pub mod aquarion;
pub mod attack;
pub mod board;
pub mod capture;
pub mod combat;
pub mod contract;
pub mod cymanti;
pub mod economy;
pub mod elyrion;
pub mod explore;
mod game_rules;
pub mod infiltrate;
pub mod inline;
pub mod linked;
pub mod mapgen;
pub mod menu;
pub mod menu_cache;
pub mod movement;
pub mod polaris;
pub mod queries;
pub mod random;
pub mod reset;
pub mod reward;
pub mod routes;
pub mod ruins;
pub mod shadow;
pub mod state;
pub mod step;
pub mod tables;
pub mod turns;
pub mod units;
pub mod view;
pub mod viewer;
pub mod wire;

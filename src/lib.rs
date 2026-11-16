//! engine-fast: a fast, exact rules engine for The Battle of Polytopia, for training bots.
//!
//! [`reset`] starts a game, [`menu`] lists the legal moves, [`step`] applies one, and
//! [`view`] and [`queries`] give what the player to move may know. The engine refuses what
//! it does not implement rather than approximating it.

pub mod action;
pub mod aquarion;
pub mod attack;
pub mod capture;
pub mod combat;
pub mod contract;
pub mod cymanti;
pub mod economy;
pub mod elyrion;
pub mod explore;
pub mod infiltrate;
pub mod inline;
pub mod linked;
pub mod mapgen;
pub mod menu;
pub mod movement;
pub mod polaris;
pub mod queries;
pub mod random;
pub mod reset;
pub mod reward;
pub mod routes;
pub mod ruins;
pub mod state;
pub mod step;
pub mod tables;
pub mod turns;
pub mod units;
pub mod view;
pub mod viewer;

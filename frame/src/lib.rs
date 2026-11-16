//! Value types shared by engine-fast's crates: coordinates, actions, legal-list entries with
//! their details, remembered units, unit statistics, and section timers. Strict in both
//! directions: unknown keys are refused, and an `Option` field must be present (null
//! allowed) unless it is marked as one the game omits.
use serde::{Deserialize, Deserializer, Serialize};

/// An `Option` field that must be present (null allowed); serde reads a missing key as None.
fn req<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Xy {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RememberedUnit {
    pub index: i32,
    pub owner: i32,
    pub kind: i32,
    pub health_label: i32,
    pub turn: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub action: Action,
    pub details: Details,
}

/// A command; which fields are present depends on `kind`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub kind: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Xy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Xy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opponent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<bool>,
}

/// Candidate details; which fields are present depends on the action kind.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Details {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embark: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_preview: Option<AttackPreview>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttackPreview {
    #[serde(deserialize_with = "req")]
    pub damage_dealt: Option<i32>,
    #[serde(deserialize_with = "req")]
    pub damage_taken: Option<i32>,
    pub kills_target: bool,
    pub attacker_dies: bool,
    pub retaliation: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitStats {
    pub displayed_base_attack: f32,
    #[serde(deserialize_with = "req")]
    pub displayed_attack_modifier: Option<f32>,
    pub defence: Defence,
    pub displayed_base_movement: i32,
    #[serde(deserialize_with = "req")]
    pub displayed_movement_modifier: Option<i32>,
    pub displayed_range: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defence {
    pub displayed_base: f32,
    #[serde(deserialize_with = "req")]
    pub displayed_multiplier: Option<f32>,
}

/// Section timers for benchmarks. `let _t = timers::scope("frame.board");` charges the time
/// until `_t` is dropped to that name, per thread. Without the `timers` feature a scope is an
/// empty value and nothing is measured. Keep scopes out of per-tile loops: one costs about
/// two clock reads.
pub mod timers {
    #[cfg(feature = "timers")]
    mod imp {
        use std::cell::RefCell;
        use std::time::Instant;

        thread_local! {
            /// (name, nanoseconds, calls) in order of first use
            static TOTALS: RefCell<Vec<(&'static str, u64, u64)>> = const { RefCell::new(Vec::new()) };
        }
        pub struct Scope(&'static str, Instant);
        pub fn scope(name: &'static str) -> Scope {
            Scope(name, Instant::now())
        }
        impl Drop for Scope {
            fn drop(&mut self) {
                let ns = self.1.elapsed().as_nanos() as u64;
                TOTALS.with(|t| {
                    let mut t = t.borrow_mut();
                    match t.iter_mut().find(|e| e.0 == self.0) {
                        Some(e) => {
                            e.1 += ns;
                            e.2 += 1;
                        }
                        None => t.push((self.0, ns, 1)),
                    }
                });
            }
        }
        pub fn report() -> Vec<(&'static str, u64, u64)> {
            TOTALS.with(|t| t.borrow().clone())
        }
    }
    #[cfg(not(feature = "timers"))]
    mod imp {
        pub struct Scope;
        #[inline(always)]
        pub fn scope(_name: &'static str) -> Scope {
            Scope
        }
        pub fn report() -> Vec<(&'static str, u64, u64)> {
            Vec::new()
        }
    }
    pub use imp::{report, scope, Scope};
}

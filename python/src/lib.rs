//! The Python module `engine_fast`: the batch interface as numpy arrays, on the CPU or
//! the GPU backend. README.md here documents every call. Every array is a fresh copy
//! owned by the caller, except the GPU's `device_arrays`, which are device memory.

use ::engine_fast::reset::Config;
use ::engine_fast::view::{View, TILE_PLANES};
use batch::outputs::{Plane, OUTPUTS, PLAYER_FIELDS, PROGRESSION_LEN, SCALAR_FIELDS, VIEW_SEATS};
use engine_fast_batch as batch;
use numpy::ndarray::{Array1, Array2, Array3, Axis};
use numpy::{AllowTypeChange, IntoPyArray, PyArray1, PyArray2, PyArray3, PyArrayLike1};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use sha2::Digest;
use std::borrow::Cow;

/// The rules the package carries: rules version 124 in the engine's own format.
const RULES: &str = include_str!("../../rules/rules-124.json");
/// The SHA-256 of the game's own rule asset of rules version 124 (game build 2.18.0.16751),
/// which `rules=` or `ENGINE_FAST_RULES` may name instead.
const ASSET_SHA256: &str = "c6806c77963b5377bdbc2570bf0a3abece622e4e148883754dd190b02f890086";

#[cfg(feature = "cuda")]
mod device;

/// The rules version a loaded state and a written command carry in front.
const RULES_VERSION: u32 = 124;

fn refuse(e: String) -> PyErr {
    PyValueError::new_err(e)
}

/// The rule text: the file at `path`, else the file `ENGINE_FAST_RULES` names, else the rules
/// the package carries. A file in the engine's own format is taken as it is; the game's asset
/// only when it is rules version 124's.
fn rules(path: Option<String>) -> PyResult<Cow<'static, str>> {
    let named = || {
        std::env::var("ENGINE_FAST_RULES")
            .ok()
            .filter(|p| !p.is_empty())
    };
    let Some(path) = path.or_else(named) else {
        return Ok(Cow::Borrowed(RULES));
    };
    let text = std::fs::read_to_string(&path).map_err(|e| refuse(format!("{path}: {e}")))?;
    let own = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
        .map_err(|e| refuse(format!("{path}: {e}")))?
        .contains_key("format");
    if !own && sha256(&text) != ASSET_SHA256 {
        return Err(refuse(format!(
            "{path} is neither rules in the engine's format nor the game's rules version 124"
        )));
    }
    Ok(Cow::Owned(text))
}

fn sha256(text: &str) -> String {
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A row of a fresh array, which is contiguous.
fn slice<T>(row: Option<&mut [T]>) -> PyResult<&mut [T]> {
    row.ok_or_else(|| refuse("array layout".into()))
}

/// `(tiles, scalars, players, tech)` of `Batch.views`.
type Views<'py> = (
    Bound<'py, PyArray3<i16>>,
    Bound<'py, PyArray2<i32>>,
    Bound<'py, PyArray3<i64>>,
    Bound<'py, PyArray2<i16>>,
);

/// The engine behind a `Batch`: the CPU batch, or with the `cuda` feature the GPU's.
#[allow(clippy::large_enum_variant)] // one per Python batch, never moved in a loop
enum Inner {
    Cpu(batch::Batch),
    #[cfg(feature = "cuda")]
    Gpu(Box<engine_fast_gpu::batch::GpuBatch>),
}

/// Each call goes to whichever engine the batch was made with.
macro_rules! each {
    ($inner:expr, $b:ident => $call:expr) => {
        match $inner {
            Inner::Cpu($b) => $call,
            #[cfg(feature = "cuda")]
            Inner::Gpu($b) => $call,
        }
    };
}

impl Inner {
    /// The host calls see the device's state after device steps (`step_device`).
    fn sync(&mut self) -> Result<(), String> {
        match self {
            Inner::Cpu(_) => Ok(()),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => b.sync(),
        }
    }
    fn len(&self) -> usize {
        each!(self, b => b.len())
    }
    fn tech_ids(&self) -> usize {
        each!(self, b => b.tech_ids())
    }
    fn reset(&mut self, games: &[(usize, Config)]) -> Result<(), String> {
        each!(self, b => b.reset(games))
    }
    fn step(&mut self, moves: &[i32], applied: &mut [batch::Applied]) -> Result<(), String> {
        each!(self, b => b.step(moves, applied))
    }
    fn status(&self, slot: usize) -> batch::Status {
        each!(self, b => b.status(slot))
    }
    fn winner(&self, slot: usize) -> i32 {
        each!(self, b => b.winner(slot))
    }
    fn reason(&self, slot: usize) -> &str {
        each!(self, b => b.reason(slot))
    }
    fn moves(&self, slot: usize) -> &[batch::Move] {
        each!(self, b => b.moves(slot))
    }
    fn view(&mut self, slot: usize) -> Result<Option<&View>, String> {
        match self {
            Inner::Cpu(b) => Ok(b.view(slot)),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => b.view(slot),
        }
    }
    fn other_view(&mut self, slot: usize) -> Result<Option<Cow<'_, View>>, String> {
        match self {
            Inner::Cpu(b) => Ok(b.other_view(slot)?.map(Cow::Owned)),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => Ok(b.other_view(slot)?.map(Cow::Borrowed)),
        }
    }
    fn plane(&mut self, slot: usize, p: Plane) -> Result<Option<Vec<i16>>, String> {
        each!(self, b => b.plane(slot, p))
    }
    fn income(&mut self, slot: usize) -> Result<Option<i32>, String> {
        each!(self, b => b.income(slot))
    }
    fn city_score(&mut self, slot: usize) -> Result<Option<Vec<i32>>, String> {
        match self {
            Inner::Cpu(b) => Ok(b.city_score(slot)),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => b.city_score(slot),
        }
    }
    fn progression(&mut self, slot: usize) -> Result<Option<Vec<i32>>, String> {
        each!(self, b => b.progression(slot))
    }
}

/// `games` slots of one board size (side 11 to 30, or 8 for nano), played on `workers` threads (default:
/// one, the caller's own). `backend` "cpu" (the default) or, in a package built with the `cuda`
/// feature, "gpu": the same interface with the games kept on GPU 0. `rules` is the path of a rule
/// file, in the engine's format or the game's asset (default: the `ENGINE_FAST_RULES` environment
/// variable, else the rules the package carries).
#[pyclass]
struct Batch {
    inner: Inner,
    size: i32,
    /// the optional outputs asked for when the batch was made, from `OUTPUTS`
    outputs: Vec<&'static str>,
    /// of the rule text the batch was made with
    rules_sha256: String,
}

impl Batch {
    /// Refuses a call for an output the batch was made without.
    fn asked(&self, name: &str) -> PyResult<()> {
        if self.outputs.contains(&name) {
            Ok(())
        } else {
            Err(refuse(format!(
                "the batch was made without outputs=(\"{name}\",)"
            )))
        }
    }

    /// The cpu batch, for a call only it answers.
    #[cfg_attr(not(feature = "cuda"), allow(unused_variables))]
    fn cpu(&self, name: &str) -> PyResult<&batch::Batch> {
        match &self.inner {
            Inner::Cpu(b) => Ok(b),
            #[cfg(feature = "cuda")]
            Inner::Gpu(_) => Err(refuse(format!("{name}: the cpu backend only"))),
        }
    }

    /// The cpu batch, mutable.
    #[cfg_attr(not(feature = "cuda"), allow(unused_variables))]
    fn cpu_mut(&mut self, name: &str) -> PyResult<&mut batch::Batch> {
        match &mut self.inner {
            Inner::Cpu(b) => Ok(b),
            #[cfg(feature = "cuda")]
            Inner::Gpu(_) => Err(refuse(format!("{name}: the cpu backend only"))),
        }
    }

    /// The mover's views (`views`), or with `other` the other seat's (`other_views`).
    fn view_arrays<'py>(&mut self, py: Python<'py>, other: bool) -> PyResult<Views<'py>> {
        let n = self.inner.len();
        let cells = (self.size as usize).pow(2);
        let techs = self.inner.tech_ids();
        let mut tiles = Array3::from_elem((n, TILE_PLANES.len(), cells), -1i16);
        let mut scalars = Array2::from_elem((n, SCALAR_FIELDS.len()), -1i32);
        let mut players = Array3::from_elem((n, VIEW_SEATS, PLAYER_FIELDS.len()), -1i64);
        let mut tech = Array2::from_elem((n, techs), -1i16);
        for i in 0..n {
            let v = if other {
                self.inner.other_view(i)
            } else {
                self.inner.view(i).map(|v| v.map(Cow::Borrowed))
            };
            let Some(v) = v.map_err(refuse)? else {
                continue;
            };
            batch::outputs::view_rows(
                &v,
                slice(tiles.index_axis_mut(Axis(0), i).into_slice())?,
                slice(scalars.index_axis_mut(Axis(0), i).into_slice())?,
                slice(players.index_axis_mut(Axis(0), i).into_slice())?,
                slice(tech.index_axis_mut(Axis(0), i).into_slice())?,
            )
            .map_err(|e| refuse(format!("slot {i}: {e}")))?;
        }
        Ok((
            tiles.into_pyarray(py),
            scalars.into_pyarray(py),
            players.into_pyarray(py),
            tech.into_pyarray(py),
        ))
    }

    /// One row of `width` per slot, `[games, width]`, -1 for a slot without one.
    fn rows<'py, T: numpy::Element + Copy + From<i8>>(
        &self,
        py: Python<'py>,
        width: usize,
        rows: Vec<Option<Vec<T>>>,
    ) -> PyResult<Bound<'py, PyArray2<T>>> {
        let mut out = Array2::from_elem((rows.len(), width), T::from(-1i8));
        for (slot, row) in rows.into_iter().enumerate() {
            let Some(r) = row else { continue };
            if r.len() != width {
                return Err(refuse(format!(
                    "slot {slot}: a row larger than the batch's"
                )));
            }
            out.row_mut(slot).assign(&Array1::from_vec(r));
        }
        Ok(out.into_pyarray(py))
    }

    /// Plane `p` of every slot as int16 `[games, size * size]`, -1 in slots not playing.
    fn plane_rows<'py>(
        &mut self,
        py: Python<'py>,
        p: Plane,
    ) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.asked(p.name())?;
        self.inner.sync().map_err(refuse)?;
        let planes = (0..self.inner.len())
            .map(|slot| self.inner.plane(slot, p))
            .collect::<Result<Vec<_>, String>>()
            .map_err(refuse)?;
        self.rows(py, (self.size as usize).pow(2), planes)
    }
}

#[pymethods]
impl Batch {
    #[new]
    #[pyo3(signature = (games, size=11, workers=1, backend="cpu", outputs=Vec::new(), rules=None))]
    fn new(
        games: usize,
        size: i32,
        workers: usize,
        backend: &str,
        outputs: Vec<String>,
        rules: Option<String>,
    ) -> PyResult<Self> {
        if !::engine_fast::reset::side_admitted(size) {
            return Err(refuse("board side is neither 11 to 30 nor nano (8)".into()));
        }
        if let Some(o) = outputs.iter().find(|o| !OUTPUTS.contains(&o.as_str())) {
            return Err(refuse(format!("unknown output {o:?}: one of {OUTPUTS:?}")));
        }
        let wanted: Vec<&'static str> = OUTPUTS
            .into_iter()
            .filter(|n| outputs.iter().any(|o| o == n))
            .collect();
        let rules = self::rules(rules)?;
        let inner = match backend {
            "cpu" => Inner::Cpu(batch::Batch::new(&rules, games, workers).map_err(refuse)?),
            #[cfg(feature = "cuda")]
            "gpu" => Inner::Gpu(Box::new(
                engine_fast_gpu::batch::GpuBatch::with_outputs(
                    &rules,
                    games,
                    workers,
                    size as usize,
                    engine_fast_gpu::device::Outputs::named(&wanted).map_err(refuse)?,
                )
                .map_err(refuse)?,
            )),
            #[cfg(not(feature = "cuda"))]
            "gpu" => return Err(refuse("built without the cuda feature".into())),
            _ => return Err(refuse("backend \"cpu\" or \"gpu\"".into())),
        };
        Ok(Batch {
            inner,
            size,
            outputs: wanted,
            rules_sha256: sha256(&rules),
        })
    }

    /// The GPU batch's outputs in device memory, by name (see `DEVICE_ARRAYS`), as CUDA arrays
    /// that torch and others read in place (`torch.as_tensor(a, device="cuda")`); each keeps
    /// the batch alive and changes with every step. The gpu backend only.
    fn device_arrays<'py>(slf: Bound<'py, Self>) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
        let py = slf.py();
        let owner: PyObject = slf.clone().into_any().unbind();
        let mut this = slf.borrow_mut();
        match &mut this.inner {
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => {
                let games = b.len();
                let x = b.exports().map_err(refuse)?;
                device::arrays(py, owner, &x, games)
            }
            _ => {
                let _ = (py, owner);
                Err(refuse("device_arrays: the gpu backend only".into()))
            }
        }
    }

    /// One decision per slot from `actions`, a contiguous int32 CUDA array (a policy's output,
    /// for example a torch tensor) of one row index per slot, `SKIP` to skip; the step runs on
    /// the GPU and returns `(slots not playing, slots that stopped in this call)`, the only data
    /// it copies to the host. Per slot, `device_arrays()["applied"]` says what was done (see
    /// `APPLIED`). The host calls (`status`, `moves`, `views`, ...) still work, after copying
    /// the device's state back. The gpu backend only.
    fn step_device(&mut self, actions: &Bound<'_, PyAny>) -> PyResult<(u32, u32)> {
        match &mut self.inner {
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => {
                let (ptr, bytes) = device::actions(actions, b.len())?;
                let w = b.step_device(ptr, bytes).map_err(refuse)?;
                Ok((w[0], w[1]))
            }
            _ => {
                let _ = actions;
                Err(refuse("step_device: the gpu backend only".into()))
            }
        }
    }

    /// Bytes copied (to the device, to the host) by this batch's calls so far; zero for the
    /// cpu backend.
    fn traffic(&self) -> (u64, u64) {
        match &self.inner {
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => b.traffic(),
            _ => (0, 0),
        }
    }

    /// The attack preview of every legal row, as `moves()` lists them: int16 `[rows,
    /// len(PREVIEW_FIELDS)]`, zero for rows without one. Needs `outputs=("previews",)`.
    fn previews<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.asked("previews")?;
        self.inner.sync().map_err(refuse)?;
        let mut rows: Vec<i16> = Vec::new();
        for slot in 0..self.inner.len() {
            let p = match &mut self.inner {
                Inner::Cpu(b) => b.previews(slot),
                #[cfg(feature = "cuda")]
                Inner::Gpu(b) => b.previews(slot).map_err(refuse)?,
            };
            rows.extend(p.iter().flatten());
        }
        let shape = (rows.len() / 4, 4);
        Ok(Array2::from_shape_vec(shape, rows)
            .map_err(|e| refuse(e.to_string()))?
            .into_pyarray(py))
    }

    /// The defence bonus (tenths; 10 none) of each unit the player to move sees, int16
    /// `[games, size * size]`, -1 elsewhere and in slots not playing. Can show hidden state.
    /// Needs `outputs=("defence",)`.
    fn defence<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::Defence)
    }

    /// The maximum health the unit panel displays (a carrier: its passenger's unless it has
    /// Protect) of each unit the player to move sees, int16 `[games, size * size]`, -1
    /// elsewhere and in slots not playing. Shows a seen foreign carrier's passenger through
    /// it, as the game's screen does. Needs `outputs=("max_health",)`.
    fn max_health<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::MaxHealth)
    }

    /// The movement the unit panel displays (with boost, Swift, Bubble and poison) of each unit
    /// the player to move sees, int16 `[games, size * size]`, -1 elsewhere and in slots not
    /// playing. Shows a seen foreign unit's effects. Needs `outputs=("movement",)`.
    fn movement<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::Movement)
    }

    /// The attack modifier the unit panel displays (tenths of the attack: 50 when boosted, else
    /// 0) of each unit the player to move sees, int16 `[games, size * size]`, -1 elsewhere and
    /// in slots not playing. Needs `outputs=("attack_modifier",)`.
    fn attack_modifier<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::AttackModifier)
    }

    /// The stars per turn each city the player to move sees gives it, as the city displays
    /// them (units hidden from it absent), int16 `[games, size * size]`, -1 elsewhere and in
    /// slots not playing. Needs `outputs=("city_work",)`.
    fn city_work<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::CityWork)
    }

    /// The population slots taken by units homed in each own city (units hidden from the
    /// player to move absent; another player's unit homed there counts), int16 `[games, size *
    /// size]`, -1 elsewhere and in slots not playing. Needs `outputs=("city_units",)`.
    fn city_units<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::CityUnits)
    }

    /// 1 on an explored, owned city with another player's unit the player to move sees in it,
    /// 0 on other explored tiles, -1 elsewhere and in slots not playing; not who. Int16
    /// `[games, size * size]`. Needs `outputs=("city_siege",)`.
    fn city_siege<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::CitySiege)
    }

    /// The sides of each explored tile where its owner's territory ends, as bits (1 north,
    /// y + 1; 2 south; 4 east; 8 west), int16 `[games, size * size]`, -1 elsewhere and in slots
    /// not playing. Shows whether an unexplored neighbour has the same owner, as the game's
    /// border does. Needs `outputs=("border_edges",)`.
    fn border_edges<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.plane_rows(py, Plane::BorderEdges)
    }

    /// The income the player to move is shown (units hidden from it count as absent), int32
    /// `[games]`, -1 in slots not playing. Needs `outputs=("income",)`.
    fn income<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<i32>>> {
        self.asked("income")?;
        self.inner.sync().map_err(refuse)?;
        let out = (0..self.inner.len())
            .map(|slot| self.inner.income(slot).map(|v| v.unwrap_or(-1)))
            .collect::<Result<Vec<i32>, String>>()
            .map_err(refuse)?;
        Ok(Array1::from_vec(out).into_pyarray(py))
    }

    /// The score of each city the player to move sees (a foreign city's shows its population
    /// progress, as the building popup does), int32 `[games, size * size]`, -1 elsewhere and in
    /// slots not playing. Needs `outputs=("city_score",)`.
    fn city_score<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i32>>> {
        self.asked("city_score")?;
        self.inner.sync().map_err(refuse)?;
        let rows = (0..self.inner.len())
            .map(|slot| self.inner.city_score(slot))
            .collect::<Result<Vec<_>, String>>()
            .map_err(refuse)?;
        self.rows(py, (self.size as usize).pow(2), rows)
    }

    /// The player to move's progression row (see `PROGRESSION`: units lost, start tile,
    /// revealed resources, monuments, tasks, lighthouses, embassies), int32 `[games,
    /// PROGRESSION_LEN]`, -1 past each list's end and in slots not playing. Needs
    /// `outputs=("progression",)`.
    fn progression<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i32>>> {
        self.asked("progression")?;
        self.inner.sync().map_err(refuse)?;
        let rows = (0..self.inner.len())
            .map(|slot| self.inner.progression(slot))
            .collect::<Result<Vec<_>, String>>()
            .map_err(refuse)?;
        self.rows(py, PROGRESSION_LEN, rows)
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// The board side of every game in the batch.
    #[getter]
    fn size(&self) -> i32 {
        self.size
    }

    /// The SHA-256 of the rule text the batch plays on, however it was named.
    #[getter]
    fn rules_sha256(&self) -> &str {
        &self.rules_sha256
    }

    /// Start a new game in each of `slots` (each named once) with the matching entry of
    /// `seeds`. `tribes` has one tribe id per seat (two to four); `map` and `mode` are the
    /// game's preset and mode ids; a game stops as truncated after `action_limit` decisions.
    /// A configuration the engine does not take refuses the slot (status 4), not the call.
    #[pyo3(signature = (slots, seeds, tribes, map=2, mode=4, action_limit=1000))]
    #[allow(clippy::too_many_arguments)] // Python's keyword arguments
    fn reset(
        &mut self,
        py: Python<'_>,
        slots: PyArrayLike1<i64, AllowTypeChange>,
        seeds: PyArrayLike1<i64, AllowTypeChange>,
        tribes: Vec<i32>,
        map: i32,
        mode: i32,
        action_limit: u32,
    ) -> PyResult<()> {
        let (slots, seeds) = (slots.as_array(), seeds.as_array());
        if slots.len() != seeds.len() {
            return Err(refuse("one seed per slot".into()));
        }
        let mut games = Vec::with_capacity(slots.len());
        for (&slot, &seed) in slots.iter().zip(seeds) {
            let config = Config {
                rules: 124,
                seed: i32::try_from(seed).map_err(|_| refuse("seed out of range".into()))?,
                size: self.size,
                map,
                mode,
                tribes: tribes.clone(),
                action_limit,
            };
            let slot = usize::try_from(slot).map_err(|_| refuse("slot out of range".into()))?;
            games.push((slot, config));
        }
        let inner = &mut self.inner;
        py.allow_threads(|| inner.reset(&games)).map_err(refuse)
    }

    /// Start games for `restart_finished` on background threads (gpu backend), after any
    /// queued before: one per entry of `seeds`, with the rest as in `reset`.
    #[pyo3(signature = (seeds, tribes, map=2, mode=4, action_limit=1000))]
    fn queue(
        &mut self,
        seeds: PyArrayLike1<i64, AllowTypeChange>,
        tribes: Vec<i32>,
        map: i32,
        mode: i32,
        action_limit: u32,
    ) -> PyResult<()> {
        let seeds = seeds.as_array();
        let mut games = Vec::with_capacity(seeds.len());
        for &seed in seeds {
            games.push(Config {
                rules: 124,
                seed: i32::try_from(seed).map_err(|_| refuse("seed out of range".into()))?,
                size: self.size,
                map,
                mode,
                tribes: tribes.clone(),
                action_limit,
            });
        }
        match &mut self.inner {
            Inner::Cpu(_) => Err(refuse("queue needs the gpu backend".into())),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => b.queue(games).map_err(refuse),
        }
    }

    /// Games queued and not yet used, and of those the ones already built: (queued, ready).
    fn queue_depth(&self) -> PyResult<(usize, usize)> {
        match &self.inner {
            Inner::Cpu(_) => Err(refuse("queue_depth needs the gpu backend".into())),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => Ok((b.queued(), b.ready().map_err(refuse)?)),
        }
    }

    /// Give every slot not playing (in slot order) the next queued game (in queue order), as
    /// `reset` would, on the device alone; slots past the queue's end stay as they are. With
    /// `wait` false, only games already built are used. Returns the slots restarted, in queue
    /// order, a slot whose game was refused included (its status already refused). Read the
    /// finished slots' outcomes before: the device arrays show the new games afterwards; the
    /// other slots are untouched (gpu backend).
    #[pyo3(signature = (wait=true))]
    #[cfg_attr(not(feature = "cuda"), allow(unused_variables))] // `wait` is the gpu's
    fn restart_finished<'py>(
        &mut self,
        py: Python<'py>,
        wait: bool,
    ) -> PyResult<Bound<'py, PyArray1<i64>>> {
        let slots: Result<Vec<usize>, String> = match &mut self.inner {
            Inner::Cpu(_) => Err("restart_finished needs the gpu backend".into()),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => py.allow_threads(|| b.restart_finished(wait)),
        };
        let slots = slots.map_err(refuse)?;
        Ok(Array1::from_iter(slots.into_iter().map(|k| k as i64)).into_pyarray(py))
    }

    /// Apply one move per slot: an index into the slot's legal moves, or -1 to leave the slot
    /// as it is. Returns per slot 0 (no move given), 1 (taken: read the slot's new status) or
    /// 2 (rejected: the slot is not playing or the index is not in its list; nothing
    /// changed). See `APPLIED`.
    fn step<'py>(
        &mut self,
        py: Python<'py>,
        moves: PyArrayLike1<i32, AllowTypeChange>,
    ) -> PyResult<Bound<'py, PyArray1<u8>>> {
        // copied while the interpreter lock is held: the step runs without it
        let moves = moves.as_array().to_vec();
        self.inner.sync().map_err(refuse)?;
        let mut applied = vec![batch::Applied::Skipped; self.inner.len()];
        let inner = &mut self.inner;
        py.allow_threads(|| inner.step(&moves, &mut applied))
            .map_err(refuse)?;
        Ok(Array1::from_iter(applied.iter().map(|&a| a as u8)).into_pyarray(py))
    }

    /// The legality mask of every slot over the fixed action table (`table_slots(size)` bits,
    /// slot i in bit i % 32 of word i / 32): uint32 `[games, words]`, zero for slots not
    /// playing. The cpu backend.
    fn masks<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<u32>>> {
        let b = self.cpu("masks")?;
        let words = batch::table::slots((self.size * self.size) as usize).div_ceil(32);
        let mut out = Array2::<u32>::zeros((b.len(), words));
        let (mut rows, mut mask) = (Vec::new(), Vec::new());
        for slot in 0..b.len() {
            if b.table(slot, &mut rows, &mut mask).map_err(refuse)? {
                out.row_mut(slot).assign(&Array1::from_vec(mask.clone()));
            }
        }
        Ok(out.into_pyarray(py))
    }

    /// The action-table slot of every legal row, in `moves()` order (int64 `[rows]`; slot i's
    /// rows are `offsets[i]:offsets[i + 1]` of `moves()`). The cpu backend.
    fn table_rows<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<i64>>> {
        let b = self.cpu("table_rows")?;
        let (mut rows, mut mask, mut out) = (Vec::new(), Vec::new(), Vec::new());
        for slot in 0..b.len() {
            b.table(slot, &mut rows, &mut mask).map_err(refuse)?;
            out.extend(rows.iter().map(|&r| i64::from(r)));
        }
        Ok(Array1::from_vec(out).into_pyarray(py))
    }

    /// What `table_encode` and `table_decode` need per slot: int32 `[games, 3]`, the board
    /// side, the player to move and the number of seats; -1 for slots not playing.
    fn table_frames<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i32>>> {
        let b = self.cpu("table_frames")?;
        let mut out = Array2::from_elem((b.len(), 3), -1i32);
        for slot in 0..b.len() {
            if let (batch::Status::Playing, Some(g)) = (b.status(slot), b.game(slot)) {
                let f = batch::outputs::table_frame(g).map_err(refuse)?;
                out.row_mut(slot)
                    .assign(&Array1::from_vec(vec![f.side, f.mover, f.seats]));
            }
        }
        Ok(out.into_pyarray(py))
    }

    /// `step` with one action-table slot per game (int64, `SKIP` to skip) in place of a row
    /// index; a slot no legal row holds is rejected (2) and changes nothing. The cpu backend.
    fn step_table<'py>(
        &mut self,
        py: Python<'py>,
        slots: PyArrayLike1<i64, AllowTypeChange>,
    ) -> PyResult<Bound<'py, PyArray1<u8>>> {
        let slots = slots.as_array().to_vec();
        let b = self.cpu_mut("step_table")?;
        let mut applied = vec![batch::Applied::Skipped; b.len()];
        py.allow_threads(|| b.step_table(&slots, &mut applied))
            .map_err(refuse)?;
        Ok(Array1::from_iter(applied.iter().map(|&a| a as u8)).into_pyarray(py))
    }

    /// Put a multiplayer game's state in `slot`, for the player whose account id is `account`
    /// (its usual text, dashed or not). `state` is the backend's bytes: the rules version, then
    /// the serialized game. The slot plays (status 1) only while that player is to move, with
    /// that player's view and legal moves; while another player is to move it is empty. Returns
    /// the player's id and the index the next command is sent with. cpu backend only.
    fn load(&mut self, slot: usize, state: &[u8], account: &str) -> PyResult<(i32, usize)> {
        let size = self.size;
        let b = self.cpu_mut("load")?;
        // a real state is some tens of kilobytes
        if state.len() > 4 << 20 {
            return Err(refuse("the state is over 4 MiB".into()));
        }
        let wire = (state.get(4..))
            .filter(|_| state[..4] == RULES_VERSION.to_le_bytes())
            .ok_or_else(|| refuse(format!("the state is not of rules version {RULES_VERSION}")))?;
        let account = engine_fast::wire::account(account.as_bytes()).map_err(refuse)?;
        let loaded = b.load(slot, wire, account).map_err(refuse)?;
        if loaded.width != size {
            b.clear(slot);
            return Err(refuse(format!("the game's board is not {size} by {size}")));
        }
        Ok((loaded.seat, loaded.commands))
    }

    /// The legal move `index` of a loaded slot as the command to send: the rules version, then
    /// the serialized command. cpu backend only.
    fn command<'py>(
        &self,
        py: Python<'py>,
        slot: usize,
        index: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let command = self.cpu("command")?.command(slot, index).map_err(refuse)?;
        Ok(PyBytes::new(
            py,
            &[&RULES_VERSION.to_le_bytes()[..], &command].concat(),
        ))
    }

    /// Per slot: 0 empty, 1 playing, 2 ended, 3 truncated, 4 refused, 5 failed (`STATUS`).
    fn status<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<u8>>> {
        self.inner.sync().map_err(refuse)?;
        Ok(
            Array1::from_iter((0..self.inner.len()).map(|i| self.inner.status(i) as u8))
                .into_pyarray(py),
        )
    }

    /// Per slot: the winner's player id once the game has ended, else 0.
    fn winner<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<i32>>> {
        self.inner.sync().map_err(refuse)?;
        Ok(Array1::from_iter((0..self.inner.len()).map(|i| self.inner.winner(i))).into_pyarray(py))
    }

    /// Why a slot was refused or failed ("" otherwise).
    fn reason(&mut self, slot: usize) -> PyResult<String> {
        self.inner.sync().map_err(refuse)?;
        if slot >= self.inner.len() {
            return Err(refuse("slot out of range".into()));
        }
        Ok(self.inner.reason(slot).to_owned())
    }

    /// The legal moves of every playing slot: `(offsets, moves)`. Slot `i`'s moves are rows
    /// `offsets[i]:offsets[i + 1]` of `moves`, whose columns are `MOVE_FIELDS`; within a slot
    /// they are sorted. The index to pass to `step` is the row's position within its slot.
    #[allow(clippy::type_complexity)]
    fn moves<'py>(
        &mut self,
        py: Python<'py>,
    ) -> PyResult<(Bound<'py, PyArray1<i64>>, Bound<'py, PyArray2<i32>>)> {
        self.inner.sync().map_err(refuse)?;
        let n = self.inner.len();
        let mut offsets = Vec::with_capacity(n + 1);
        let mut rows = Vec::new();
        offsets.push(0i64);
        for i in 0..n {
            rows.extend(self.inner.moves(i).iter().flatten().copied());
            offsets.push((rows.len() / batch::MOVE_FIELDS.len()) as i64);
        }
        let shape = (
            rows.len() / batch::MOVE_FIELDS.len(),
            batch::MOVE_FIELDS.len(),
        );
        let moves = Array2::from_shape_vec(shape, rows).map_err(|e| refuse(e.to_string()))?;
        Ok((
            Array1::from(offsets).into_pyarray(py),
            moves.into_pyarray(py),
        ))
    }

    /// What the player to move may know, for every slot: `(tiles, scalars, players, tech)`.
    /// - `tiles`: int16 `[games, len(TILE_PLANES), size * size]`, tile index `y * size + x`;
    /// - `scalars`: int32 `[games, len(SCALAR_FIELDS)]`;
    /// - `players`: int64 `[games, 4, len(PLAYER_FIELDS)]`, the viewer first, then the
    ///   players it has met;
    /// - `tech`: int16 `[games, number of technology ids]`, the viewer's technologies in
    ///   ascending order.
    ///
    /// Rows of a slot that is not playing, and unused entries, are -1.
    fn views<'py>(&mut self, py: Python<'py>) -> PyResult<Views<'py>> {
        self.view_arrays(py, false)
    }

    /// The view of the next seat in turn order after the player to move (the other player in
    /// a 2-seat game; when every other player is dead, the next of them), laid out as `views`;
    /// -1 in slots not playing. For a privileged critic only: it shows what the mover's view
    /// hides, and must never reach an actor's input. Every other output is the same with or
    /// without it. Needs `outputs=("other_view",)`. Raises while any slot holds a loaded live
    /// game, whose other seat is a real opponent.
    fn other_views<'py>(&mut self, py: Python<'py>) -> PyResult<Views<'py>> {
        self.asked("other_view")?;
        self.view_arrays(py, true)
    }
}

fn frame(size: i32, mover: i32, seats: i32) -> PyResult<batch::table::Frame> {
    if !::engine_fast::reset::side_admitted(size)
        || !(2..=4).contains(&seats)
        || !(1..=seats).contains(&mover)
    {
        return Err(refuse(
            "a board side 8 or 11 to 30, 2 to 4 seats, a mover among them".into(),
        ));
    }
    Ok(batch::table::Frame {
        side: size,
        mover,
        seats,
    })
}

/// The number of slots of the action table on a board of side `size`.
#[pyfunction]
fn table_slots(size: i32) -> PyResult<usize> {
    frame(size, 1, 2)?;
    Ok(batch::table::slots((size * size) as usize))
}

/// Where the table's blocks start: (global, unit, attack, tile, move, end).
#[pyfunction]
fn table_blocks(size: i32) -> PyResult<[usize; 6]> {
    frame(size, 1, 2)?;
    Ok(batch::table::blocks((size * size) as usize))
}

/// The action-table slot of each row (int32 `[n, 5]` or `[n, 6]`: kind, source, target, option,
/// opponent, and price, which is not read), for the player `mover` of a game of `seats` seats
/// on side `size`: int64 `[n]`. Raises on a row that does not encode.
#[pyfunction]
fn table_encode<'py>(
    py: Python<'py>,
    rows: numpy::PyReadonlyArray2<'py, i32>,
    size: i32,
    mover: i32,
    seats: i32,
) -> PyResult<Bound<'py, PyArray1<i64>>> {
    let f = frame(size, mover, seats)?;
    let rows = rows.as_array();
    if !(5..=6).contains(&rows.ncols()) {
        return Err(refuse("rows of 5 or 6 fields".into()));
    }
    let mut out = Vec::with_capacity(rows.nrows());
    for r in rows.rows() {
        let m = [r[0], r[1], r[2], r[3], r[4], -1];
        out.push(batch::table::encode(&m, &f).map_err(refuse)? as i64);
    }
    Ok(Array1::from_vec(out).into_pyarray(py))
}

/// The five identifying fields (kind, source, target, option, opponent) of each slot: int32
/// `[n, 5]`, a row of -1 for a slot no move uses.
#[pyfunction]
fn table_decode<'py>(
    py: Python<'py>,
    slots: PyArrayLike1<i64, AllowTypeChange>,
    size: i32,
    mover: i32,
    seats: i32,
) -> PyResult<Bound<'py, PyArray2<i32>>> {
    let f = frame(size, mover, seats)?;
    let slots = slots.as_array();
    let mut out = Array2::from_elem((slots.len(), 5), -1i32);
    for (k, &s) in slots.iter().enumerate() {
        if let Some(fields) = usize::try_from(s)
            .ok()
            .and_then(|s| batch::table::decode(s, &f))
        {
            out.row_mut(k).assign(&Array1::from_vec(fields.to_vec()));
        }
    }
    Ok(out.into_pyarray(py))
}

#[pymodule(name = "engine_fast")]
fn python_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Batch>()?;
    m.add_function(wrap_pyfunction!(table_slots, m)?)?;
    m.add_function(wrap_pyfunction!(table_blocks, m)?)?;
    m.add_function(wrap_pyfunction!(table_encode, m)?)?;
    m.add_function(wrap_pyfunction!(table_decode, m)?)?;
    m.add("TABLE_SCHEMA", batch::table::SCHEMA)?;
    m.add("RULES", 124)?;
    // what a caller that pins rule data checks the carried rules against
    m.add("RULES_SHA256", sha256(RULES))?;
    m.add("TILE_PLANES", TILE_PLANES.to_vec())?;
    m.add("MOVE_FIELDS", batch::MOVE_FIELDS.to_vec())?;
    m.add("SCALAR_FIELDS", SCALAR_FIELDS.to_vec())?;
    m.add("PLAYER_FIELDS", PLAYER_FIELDS.to_vec())?;
    m.add(
        "STATUS",
        vec![
            "empty",
            "playing",
            "ended",
            "truncated",
            "refused",
            "failed",
        ],
    )?;
    m.add("APPLIED", vec!["skipped", "taken", "rejected"])?;
    m.add(
        "KINDS",
        batch::KINDS
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>(),
    )?;
    m.add("SKIP", batch::SKIP)?;
    m.add(
        "DEVICE_ARRAYS",
        vec![
            "status", "winner", "counts", "applied", "moves", "tiles", "scalars", "players", "tech",
        ],
    )?;
    m.add("OUTPUTS", OUTPUTS.to_vec())?;
    m.add(
        "PROGRESSION",
        batch::outputs::PROGRESSION
            .iter()
            .map(|&(name, rows, fields)| (name, rows, fields.to_vec()))
            .collect::<Vec<_>>(),
    )?;
    m.add("PROGRESSION_LEN", PROGRESSION_LEN)?;
    m.add("PREVIEW_FIELDS", batch::outputs::PREVIEW_FIELDS.to_vec())?;
    #[cfg(feature = "cuda")]
    m.add_class::<device::DeviceArray>()?;
    Ok(())
}

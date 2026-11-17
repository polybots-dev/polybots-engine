//! The Python module `engine_fast`: the batch interface as numpy arrays, on the CPU or
//! the GPU backend. README.md here documents every call. Every array is a fresh copy
//! owned by the caller, except the GPU's `device_arrays`, which are device memory.

use ::engine_fast::reset::Config;
use ::engine_fast::view::TILE_PLANES;
use engine_fast_batch as batch;
use numpy::ndarray::{Array1, Array2, Array3};
use numpy::{AllowTypeChange, IntoPyArray, PyArray1, PyArray2, PyArray3, PyArrayLike1};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use sha2::Digest;

/// The SHA-256 of the rule data of rules version 124 (game build 2.18.0.16751), the one
/// version this package implements.
const RULES_SHA256: &str = "c6806c77963b5377bdbc2570bf0a3abece622e4e148883754dd190b02f890086";

/// The most players a view lists: the viewer and up to three it has met.
const SEATS: usize = 4;
const PLAYER_FIELDS: [&str; 4] = ["seat", "tribe", "relation", "score"];
const SCALAR_FIELDS: [&str; 3] = ["viewer", "turn", "stars"];
/// The optional outputs a batch can be made with.
const OUTPUTS: [&str; 4] = ["previews", "defence", "income", "max_health"];
/// The outputs only the cpu backend computes.
const CPU_OUTPUTS: [&str; 2] = ["income", "max_health"];

#[cfg(feature = "cuda")]
mod device;

fn refuse(e: String) -> PyErr {
    PyValueError::new_err(e)
}

/// The game's rule data, from `path` or else the file `ENGINE_FAST_RULES` names
/// (tools/extract_rules.py copies it out of an installed game).
fn rules(path: Option<String>) -> PyResult<String> {
    let path = path
        .or_else(|| std::env::var("ENGINE_FAST_RULES").ok())
        .ok_or_else(|| {
            refuse(
            "no rule data: pass rules=PATH or set ENGINE_FAST_RULES (see tools/extract_rules.py)"
                .into(),
        )
        })?;
    let text = std::fs::read_to_string(&path).map_err(|e| refuse(format!("{path}: {e}")))?;
    let digest = sha2::Sha256::digest(text.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if hex != RULES_SHA256 {
        return Err(refuse(format!(
            "{path} is not the rule data of rules version 124"
        )));
    }
    Ok(text)
}

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
    fn view(&mut self, slot: usize) -> Result<Option<&::engine_fast::view::View>, String> {
        match self {
            Inner::Cpu(b) => Ok(b.view(slot)),
            #[cfg(feature = "cuda")]
            Inner::Gpu(b) => b.view(slot),
        }
    }
}

/// `games` slots of one board size (side 11 to 30, or 8 for nano), played on `workers` threads (default:
/// one, the caller's own). `backend` "cpu" (the default) or, in a package built with the `cuda`
/// feature, "gpu": the same interface with the games kept on GPU 0. `rules` is the path of the
/// game's rule data (default: the `ENGINE_FAST_RULES` environment variable).
#[pyclass]
struct Batch {
    inner: Inner,
    size: i32,
    /// the optional outputs asked for when the batch was made, from `OUTPUTS`
    outputs: Vec<&'static str>,
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

    /// One plane per slot as int16 `[games, size * size]`, -1 for a slot without one.
    fn planes<'py>(
        &self,
        py: Python<'py>,
        planes: Vec<Option<Vec<i16>>>,
    ) -> PyResult<Bound<'py, PyArray2<i16>>> {
        let cells = (self.size as usize).pow(2);
        let mut out = Array2::from_elem((planes.len(), cells), -1i16);
        for (slot, plane) in planes.into_iter().enumerate() {
            let Some(p) = plane else { continue };
            if p.len() != cells {
                return Err(refuse(format!(
                    "slot {slot}: a plane larger than the batch's"
                )));
            }
            out.row_mut(slot).assign(&Array1::from_vec(p));
        }
        Ok(out.into_pyarray(py))
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
        if backend == "gpu" {
            if let Some(o) = wanted.iter().find(|o| CPU_OUTPUTS.contains(o)) {
                return Err(refuse(format!("output {o:?}: the cpu backend only")));
            }
        }
        let catalog = self::rules(rules)?;
        let inner = match backend {
            "cpu" => Inner::Cpu(batch::Batch::new(&catalog, games, workers).map_err(refuse)?),
            #[cfg(feature = "cuda")]
            "gpu" => Inner::Gpu(Box::new(
                engine_fast_gpu::batch::GpuBatch::with_outputs(
                    &catalog,
                    games,
                    workers,
                    size as usize,
                    engine_fast_gpu::device::Outputs {
                        previews: wanted.contains(&"previews"),
                        defence: wanted.contains(&"defence"),
                    },
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
        self.asked("defence")?;
        self.inner.sync().map_err(refuse)?;
        let planes = (0..self.inner.len())
            .map(|slot| match &mut self.inner {
                Inner::Cpu(b) => b.defence(slot),
                #[cfg(feature = "cuda")]
                Inner::Gpu(b) => b.defence(slot),
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(refuse)?;
        self.planes(py, planes)
    }

    /// The maximum health the unit panel displays (a carrier: its passenger's unless it has
    /// Protect) of each unit the player to move sees, int16 `[games, size * size]`, -1
    /// elsewhere and in slots not playing. Shows a seen foreign carrier's passenger through
    /// it, as the game's screen does. Needs `outputs=("max_health",)`; the cpu backend only.
    fn max_health<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<i16>>> {
        self.asked("max_health")?;
        let b = self.cpu("max_health")?;
        let planes = (0..b.len())
            .map(|slot| b.max_health(slot))
            .collect::<Result<Vec<_>, String>>()
            .map_err(refuse)?;
        self.planes(py, planes)
    }

    /// The income the player to move is shown (units hidden from it count as absent), int32
    /// `[games]`, -1 in slots not playing. Needs `outputs=("income",)`; the cpu backend only.
    fn income<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyArray1<i32>>> {
        self.asked("income")?;
        let b = self.cpu("income")?;
        let out = (0..b.len())
            .map(|slot| b.income(slot).map(|v| v.unwrap_or(-1)))
            .collect::<Result<Vec<i32>, String>>()
            .map_err(refuse)?;
        Ok(Array1::from_vec(out).into_pyarray(py))
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// The board side of every game in the batch.
    #[getter]
    fn size(&self) -> i32 {
        self.size
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
    #[allow(clippy::type_complexity)]
    fn views<'py>(
        &mut self,
        py: Python<'py>,
    ) -> PyResult<(
        Bound<'py, PyArray3<i16>>,
        Bound<'py, PyArray2<i32>>,
        Bound<'py, PyArray3<i64>>,
        Bound<'py, PyArray2<i16>>,
    )> {
        let n = self.inner.len();
        let cells = (self.size as usize).pow(2);
        let techs = self.inner.tech_ids();
        let mut tiles = Array3::from_elem((n, TILE_PLANES.len(), cells), -1i16);
        let mut scalars = Array2::from_elem((n, SCALAR_FIELDS.len()), -1i32);
        let mut players = Array3::from_elem((n, SEATS, PLAYER_FIELDS.len()), -1i64);
        let mut tech = Array2::from_elem((n, techs), -1i16);
        for i in 0..n {
            let Some(v) = self.inner.view(i).map_err(refuse)? else {
                continue;
            };
            let row = tiles.index_axis_mut(numpy::ndarray::Axis(0), i);
            let row = row
                .into_slice()
                .ok_or_else(|| refuse("tile layout".into()))?;
            if v.tiles.len() != row.len() || v.players.len() > SEATS || v.tech.len() > techs {
                return Err(refuse(format!(
                    "slot {i}: a view larger than the batch's arrays"
                )));
            }
            row.copy_from_slice(&v.tiles);
            scalars[[i, 0]] = i32::from(v.viewer);
            scalars[[i, 1]] = v.turn;
            scalars[[i, 2]] = v.stars;
            for (k, p) in v.players.iter().enumerate() {
                let fields = [
                    i64::from(p.seat),
                    i64::from(p.tribe),
                    i64::from(p.relation),
                    p.score,
                ];
                for (f, value) in fields.into_iter().enumerate() {
                    players[[i, k, f]] = value;
                }
            }
            for (k, &id) in v.tech.iter().enumerate() {
                tech[[i, k]] = id;
            }
        }
        Ok((
            tiles.into_pyarray(py),
            scalars.into_pyarray(py),
            players.into_pyarray(py),
            tech.into_pyarray(py),
        ))
    }
}

#[pymodule(name = "engine_fast")]
fn python_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Batch>()?;
    m.add("RULES", 124)?;
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
    m.add("PREVIEW_FIELDS", batch::outputs::PREVIEW_FIELDS.to_vec())?;
    #[cfg(feature = "cuda")]
    m.add_class::<device::DeviceArray>()?;
    Ok(())
}

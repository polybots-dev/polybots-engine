//! The device loop from Python: a GPU batch's
//! outputs as CUDA arrays that torch and others read in place (`__cuda_array_interface__`,
//! version 3), and a policy's actions taken from a CUDA array. Nothing here copies per-game
//! data between the host and the device.

use engine_fast_gpu::device::{on_device, Exports};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

/// One exported array of a GPU batch, in device memory. It keeps the batch alive, and its
/// contents change in place with every step: copy it to keep a step's values.
/// Read it, never write it: the next step reads the batch's status and counts back.
#[pyclass(module = "engine_fast", frozen)]
pub struct DeviceArray {
    /// the batch the memory belongs to
    owner: PyObject,
    typestr: &'static str,
    shape: Vec<u64>,
    /// in bytes, or None where the array is contiguous
    strides: Option<Vec<u64>>,
    ptr: u64,
}

#[pymethods]
impl DeviceArray {
    #[getter]
    fn __cuda_array_interface__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("shape", PyTuple::new(py, &self.shape)?)?;
        d.set_item("typestr", self.typestr)?;
        // writable as the interface requires (torch refuses read-only), but a consumer must not
        // write: the step reads status and counts back
        d.set_item("data", (self.ptr, false))?;
        d.set_item("version", 3)?;
        match &self.strides {
            Some(s) => d.set_item("strides", PyTuple::new(py, s)?)?,
            None => d.set_item("strides", py.None())?,
        }
        Ok(d)
    }

    /// (kDLCUDA, device 0): where the memory is, for consumers that ask.
    fn __dlpack_device__(&self) -> (i32, i32) {
        (2, 0)
    }

    #[getter]
    fn shape<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.shape)
    }

    /// The batch this memory belongs to.
    #[getter]
    fn owner(&self, py: Python<'_>) -> PyObject {
        self.owner.clone_ref(py)
    }
}

/// The batch's exported arrays by name: `status`, `winner`, `counts`, `applied` (int32 per
/// slot), `moves` (int32 `[games, max_rows, len(MOVE_FIELDS)]`, rows past `counts` zero),
/// `tiles` (int16 `[games, len(TILE_PLANES), size * size]`), `scalars` (int32 `[games,
/// len(SCALAR_FIELDS)]`), `players` (int64 `[games, 4, len(PLAYER_FIELDS)]`) and `tech`
/// (int16 `[games, technology ids]`), laid out as the host calls hand them out (-1 rows for
/// slots not playing).
pub fn arrays<'py>(
    py: Python<'py>,
    owner: PyObject,
    x: &Exports,
    games: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let g = games as u64;
    let per_slot = |name: &str, ptr: u64| (name.to_owned(), "<i4", vec![g], None, ptr);
    let list = vec![
        per_slot("status", x.status),
        per_slot("winner", x.winners),
        per_slot("counts", x.counts),
        per_slot("applied", x.applied),
        (
            "moves".to_owned(),
            "<i4",
            vec![g, x.max_rows as u64, 6],
            Some(vec![x.menu_stride, 24, 4]),
            x.rows,
        ),
        (
            "tiles".to_owned(),
            "<i2",
            vec![g, x.planes as u64, x.cells as u64],
            Some(vec![x.view_stride, 2 * x.cells as u64, 2]),
            x.tiles,
        ),
        ("scalars".to_owned(), "<i4", vec![g, 3], None, x.scalars),
        ("players".to_owned(), "<i8", vec![g, 4, 4], None, x.players),
        (
            "tech".to_owned(),
            "<i2",
            vec![g, x.techs as u64],
            None,
            x.tech,
        ),
    ];
    let mut list = list;
    if x.previews != 0 {
        list.push((
            "previews".to_owned(),
            "<i2",
            vec![g, x.max_rows as u64, 4],
            None,
            x.previews,
        ));
    }
    if x.defence != 0 {
        list.push((
            "defence".to_owned(),
            "<i2",
            vec![g, x.cells as u64],
            None,
            x.defence,
        ));
    }
    let d = PyDict::new(py);
    for (name, typestr, shape, strides, ptr) in list {
        let a = DeviceArray {
            owner: owner.clone_ref(py),
            typestr,
            shape,
            strides,
            ptr,
        };
        d.set_item(name, Py::new(py, a)?)?;
    }
    Ok(d)
}

/// The device address and size of a policy's actions: a contiguous int32 CUDA array of one
/// entry per slot on device 0.
pub fn actions(obj: &Bound<'_, PyAny>, games: usize) -> PyResult<(u64, u64)> {
    let bad = |what: &str| PyValueError::new_err(format!("actions: {what}"));
    if let Ok(device) = obj.call_method0("__dlpack_device__") {
        let (kind, id): (i32, i32) = device.extract()?;
        if (kind, id) != (2, 0) {
            return Err(bad("not on CUDA device 0"));
        }
    }
    let cai = obj
        .getattr("__cuda_array_interface__")
        .map_err(|_| bad("not a CUDA array (no __cuda_array_interface__)"))?;
    let typestr: String = cai.get_item("typestr")?.extract()?;
    if typestr != "<i4" {
        return Err(bad("int32 needed"));
    }
    let shape: Vec<usize> = cai.get_item("shape")?.extract()?;
    if shape != [games] {
        return Err(bad("one entry per slot needed"));
    }
    if let Ok(strides) = cai.get_item("strides") {
        if !strides.is_none() {
            let s: Vec<usize> = strides.extract()?;
            if s != [4] {
                return Err(bad("contiguous needed"));
            }
        }
    }
    let (ptr, _readonly): (u64, bool) = cai.get_item("data")?.extract()?;
    if !on_device(ptr) {
        return Err(bad("not device memory of GPU 0"));
    }
    Ok((ptr, 4 * games as u64))
}

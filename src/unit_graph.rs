use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct UnitGraph {
    pub units: Vec<Unit>,
    pub roots: Vec<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Unit {
    pub pkg_id: String,
    pub target: UnitTarget,
    pub mode: String,
    pub features: Vec<String>,
    pub dependencies: Vec<UnitDep>,
}

#[derive(Debug, Deserialize)]
pub struct UnitTarget {
    pub kind: Vec<String>,
    pub crate_types: Vec<String>,
    pub name: String,
    pub src_path: String,
    pub edition: String,
}

#[derive(Debug, Deserialize)]
pub struct UnitDep {
    pub index: usize,
    pub extern_crate_name: String,
}

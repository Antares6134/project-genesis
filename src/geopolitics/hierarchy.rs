use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvinceNode {
    pub id: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub pixel_count: usize,
    pub neighbors: Vec<(u32, u32)>, // NEW: (Neighbor ID, Edge Cost)

    pub area_id: u32,
    pub region_id: u32,
    pub superregion_id: u32,
    pub continent_id: u32,
    pub anchor_id: u32, // ID of closest mainland coastal province (or self if mainland, 0 if remote ocean)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeopoliticalHierarchy {
    pub total_provinces: usize,
    pub total_areas: usize,
    pub total_regions: usize,
    pub total_superregions: usize,
    pub total_continents: usize,
    pub provinces: Vec<ProvinceNode>,
}

impl GeopoliticalHierarchy {
    /// A hierarchy of nothing: what the atlas holds before its act has run.
    ///
    /// The hierarchy tiers do not exist until a clustering pass has built them,
    /// and the atlas has to be able to paint a world that is younger than that.
    /// This is what such a world's hierarchy is — empty, and honestly so: a
    /// civilisations scope still waiting for act five has no provinces to look
    /// up, and every read of this one is guarded by a province count of zero.
    pub fn empty() -> Self {
        Self {
            total_provinces: 0,
            total_areas: 0,
            total_regions: 0,
            total_superregions: 0,
            total_continents: 0,
            provinces: Vec::new(),
        }
    }
}
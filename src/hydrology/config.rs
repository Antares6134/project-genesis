/// Seconds in a Julian year.
///
/// The bridge between the climate's *annual* water budget and the hydrology's
/// *instantaneous* discharge: a cell's yearly rainfall volume becomes a mean
/// flow in cubic metres per second by dividing by this.
pub const SECONDS_PER_YEAR: f32 = 31_557_600.0;

#[derive(Clone, Debug)]
pub struct HydrologyConfig {
    /// Depth, in heightmap units, a filled depression must reach to count as a
    /// genuine lake rather than a dimple of surface roughness.
    pub min_lake_depth: f32,
    /// Fewest pixels a filled depression must span to be kept as a lake.
    ///
    /// Priority flooding turns every closed pit into a lake, and the heightmap's
    /// own roughness leaves thousands of single-pixel pits that are not lakes in
    /// any real sense. They are erased after the flood so that only water bodies
    /// an atlas would actually draw survive: measured on an 8K world this removes
    /// ~95% of the lake *bodies* while leaving ~97% of the lake *area* — every
    /// real lake.
    pub min_lake_cells: usize,
    /// Smallest river worth drawing, in **m³/s**.
    ///
    /// This is a real discharge now, not an arbitrary accumulation score: the
    /// flow grid is annual rainfall volume routed downhill and converted with
    /// [`SECONDS_PER_YEAR`], so `20 m³/s` is a river draining a few thousand
    /// square kilometres — the smallest stream an atlas would bother to ink.
    pub min_river_flow: f32,
    /// Discharge, in **m³/s**, at which a river is drawn at full width and
    /// darkness. Amazon-class is ~200 000; the Nile is ~2 800.
    pub major_river_flow: f32,
    /// Annual rainfall, in millimetres, that a precipitation value of `1.0`
    /// stands for. The climate grid is normalized, so this is what turns its
    /// `0.0..=1.0` into metres of water per year (the same 1 200 mm the atlas
    /// prints when it reads rainfall back for a hover).
    pub reference_precip_mm_per_year: f32,
    /// Share of falling rain that reaches a channel as surface discharge rather
    /// than evaporating or soaking away, in a *temperate, well-watered* cell. A
    /// global mean of ~0.35 is Earth-like. It is scaled down by the two climate
    /// gates below before any water is routed.
    pub runoff_coefficient: f32,
    /// Air temperature (°C) at or below which the ground is frozen and yields no
    /// surface runoff at all.
    ///
    /// This is what keeps rivers off the ice caps and the tundra: a frozen cell
    /// contributes nothing, so no river can be *born* there, and one can only
    /// cross a frozen waste if it was first fed from wetter ground.
    pub runoff_freeze_temp: f32,
    /// Air temperature (°C) at or above which runoff runs at full strength. The
    /// ground between the two temperatures is a thawing margin, ramped linearly.
    pub runoff_thaw_temp: f32,
    /// Precipitation below which a cell is desert and contributes no runoff.
    ///
    /// Hyper-arid ground swallows its own rain, so a channel that *begins* in the
    /// desert is dry and is not drawn. A trunk that merely *crosses* the desert
    /// loses only its local trickle and keeps the water gathered upstream — the
    /// Nile, not a wadi.
    pub runoff_arid_precip: f32,
    /// Precipitation at or above which runoff runs at full strength; between the
    /// two the ground is semi-arid and yields proportionally less.
    pub runoff_wet_precip: f32,
}

impl Default for HydrologyConfig {
    fn default() -> Self {
        Self {
            min_lake_depth: 0.012,     // Only genuine deep tectonic rift depressions become lakes!
            min_lake_cells: 12,        // ...and only if the depression is wider than a speck
            min_river_flow: 100.0,     // only major rivers and their larger branches
            major_river_flow: 3_000.0, // ~ the Nile: a continental trunk at full width
            reference_precip_mm_per_year: 1_200.0,
            runoff_coefficient: 0.35,
            runoff_freeze_temp: 0.0,
            runoff_thaw_temp: 4.0,
            runoff_arid_precip: 0.04,
            runoff_wet_precip: 0.20,
        }
    }
}
/// Tuning for the political partition of the land.
///
/// The hierarchy itself is not tuned here — the tier targets live with the
/// clustering that reads them — but the one decision a reader can meaningfully
/// make about it does: how many great landmasses the world is allowed to call a
/// continent.
#[derive(Clone, Debug)]
pub struct GeopoliticsConfig {
    /// Hard ceiling on the major continental cores found by the topological
    /// clustering. `1` gives a single Pangaean hierarchy, `7` is the fractured
    /// Earth-like world the atlas was tuned at, and higher ceilings break the
    /// map into still more powers.
    ///
    /// Landmasses beyond the ceiling are not lost: they are anchored to the
    /// nearest continental core, exactly as an island is.
    pub max_continents: usize,
}

impl Default for GeopoliticsConfig {
    fn default() -> Self {
        Self { max_continents: 7 }
    }
}

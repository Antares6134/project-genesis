use crate::core::Grid;
use crate::geography::{OceanResult, ProvinceResult};
use crate::geopolitics::GeopoliticalHierarchy;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};

/// The four bytes that open a cache written before the acts header existed: `PG`
/// for Project Genesis, `Z1` for a zstd frame of format 1. Kept so those saves
/// still load.
const CACHE_MAGIC: &[u8; 4] = b"PGZ1";

/// The four bytes that open every cache this build writes: `PG` for Project
/// Genesis, `Z2` for a zstd frame of format 2, whose header carries one byte
/// beyond the magic — the deepest act the save holds.
const CACHE_MAGIC_ACTS: &[u8; 4] = b"PGZ2";

/// zstd level for cache writes. The payloads are spatially coherent grids and
/// boolean masks, so even this fast level folds a save to a fraction of its raw
/// size. Level 3 is also cheap to *read*: decompression runs at over a gigabyte
/// per second, so a load stays dominated by the bincode decode rather than by
/// inflating the frame.
const CACHE_ZSTD_LEVEL: i32 = 3;

/// Worker threads zstd compresses a cache on. The payloads are gigabyte-scale —
/// a full world at act six is roughly a gigabyte — and compressing it on one
/// thread leaves every other core idle while a save runs. Eight is deliberately
/// modest: it fills the cores of ordinary machines without starving the viewer,
/// and zstd simply queues work when fewer cores exist. This needs the crate's
/// `zstdmt` feature, which `Cargo.toml` turns on; without it the call below
/// would not compile, so a build that succeeds can always compress in parallel.
const CACHE_ZSTD_WORKERS: u32 = 8;

/// Bytes buffered in front of the codec on both sides of a cache write or read.
///
/// This is the single biggest lever on save and load speed. bincode encodes and
/// decodes *element by element* — a `Vec<f32>` grid becomes 33.5 million separate
/// `write_all` or `read_exact` calls — and handing each of those straight to a
/// zstd stream means 33.5 million calls into the C codec. Interposing one
/// megabyte of buffering turns them into a few hundred large calls instead:
/// measured on an 8192x4096 grid, a buffered write is 2.6x faster and a buffered
/// read 3.5x faster than the unbuffered path.
const CACHE_BUFFER_BYTES: usize = 1 << 20;

/// Write `value` as a cache: the magic, one byte naming the deepest act held,
/// then a zstd frame of its bincode payload.
///
/// bincode itself does no compression — it is a raw binary codec — so the bytes it
/// produces are wrapped here. The payload is byte-for-byte what an uncompressed
/// cache holds, so a compressed world and a plain one decode to exactly the same
/// grids, and the round-trip stays bit-exact.
fn write_cache<T: Serialize>(value: &T, acts: u8, path: &str) -> Result<(), String> {
    let file = File::create(path).map_err(|why| why.to_string())?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(CACHE_MAGIC_ACTS)
        .map_err(|why| why.to_string())?;
    // The deepest act the save holds, one byte, straight after the magic. The
    // atlas reads this to describe a cache without decoding its payload; a load
    // ignores it and reads the act off the fields, which stay the one truth.
    writer
        .write_all(&[acts])
        .map_err(|why| why.to_string())?;
    let mut encoder =
        zstd::stream::write::Encoder::new(writer, CACHE_ZSTD_LEVEL).map_err(|why| why.to_string())?;
    // Spread the compression over worker threads. This has to happen before any
    // byte is written: it is a parameter on the compression context, and a frame
    // cannot switch to multithreaded once it has started.
    encoder
        .multithread(CACHE_ZSTD_WORKERS)
        .map_err(|why| why.to_string())?;
    {
        // bincode calls `write_all` once per element, so without this buffer every
        // grid element would be a separate call into zstd. The buffer collapses
        // them into large writes — see `CACHE_BUFFER_BYTES`.
        let mut buffered = BufWriter::with_capacity(CACHE_BUFFER_BYTES, &mut encoder);
        bincode::serialize_into(&mut buffered, value).map_err(|why| why.to_string())?;
        // A buffered byte that never reaches the encoder would silently truncate
        // the save, so the flush error is reported rather than dropped.
        buffered.flush().map_err(|why| why.to_string())?;
    }
    // `finish` flushes the closing frame and reports a failed write; dropping the
    // encoder would swallow that error. The final `flush` matters for the same
    // reason: it is the last chance to hear about a full or failed disk.
    let mut writer = encoder.finish().map_err(|why| why.to_string())?;
    writer.flush().map_err(|why| why.to_string())?;
    Ok(())
}

/// Read a cache written by [`write_cache`], or by an older build — a `PGZ1` frame
/// without the acts header, or bare bincode.
///
/// The magic decides which: `PGZ2` means the acts byte and then a zstd frame,
/// `PGZ1` means the frame alone, and no magic means the whole file is the payload.
/// A file too short to hold the magic is simply handed to bincode, which reports
/// the error in its own words.
fn read_cache<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let mut file = File::open(path).map_err(|why| format!("{path}: {why}"))?;
    let mut magic = [0u8; CACHE_MAGIC.len()];
    let framed = file.read_exact(&mut magic).is_ok();
    if framed && (&magic == CACHE_MAGIC_ACTS || &magic == CACHE_MAGIC) {
        // A format-2 cache carries one byte past the magic: the deepest act the
        // save holds. It is a hint for the atlas, not the truth — the payload's
        // own fields decide what a load adopts — so it is read past and dropped.
        if &magic == CACHE_MAGIC_ACTS {
            let mut acts = [0u8; 1];
            file.read_exact(&mut acts)
                .map_err(|why| format!("{path}: {why}"))?;
        }
        let decoder = zstd::stream::read::Decoder::new(BufReader::new(file))
            .map_err(|why| format!("{path}: {why}"))?;
        // The mirror of the write buffer: bincode calls `read_exact` once per
        // element, so a buffered decoder is what keeps a load from making tens of
        // millions of small calls into zstd — see `CACHE_BUFFER_BYTES`.
        let buffered = BufReader::with_capacity(CACHE_BUFFER_BYTES, decoder);
        bincode::deserialize_from(buffered).map_err(|why| format!("{path}: {why}"))
    } else {
        // A cache written before the frame existed: bare bincode, no magic. It gets
        // the same read buffer, since the decode is element-wise either way.
        file.seek(SeekFrom::Start(0))
            .map_err(|why| format!("{path}: {why}"))?;
        let buffered = BufReader::with_capacity(CACHE_BUFFER_BYTES, file);
        bincode::deserialize_from(buffered).map_err(|why| format!("{path}: {why}"))
    }
}

/// The deepest act a cache holds, read straight off the file's header.
///
/// Only a format-2 cache (`PGZ2`) names the act in its header. For an older
/// `PGZ1` frame or a bare bincode save the act is knowable only by decoding the
/// payload, which the atlas's list must not pay for — so those answer `None`, and
/// the list says only that a world is there. A handful of bytes, so the panel may
/// describe a whole directory without loading a world.
pub fn cache_stage(path: &str) -> Option<u8> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; CACHE_MAGIC_ACTS.len() + 1];
    if file.read_exact(&mut head).is_err() {
        return None;
    }
    head.starts_with(CACHE_MAGIC_ACTS)
        .then(|| head[CACHE_MAGIC_ACTS.len()])
}

/// The deepest act a set of present-or-absent artefacts amounts to, `1..=6`.
///
/// Shared by a cache and its write-time view so the number stamped into a save's
/// header is the very number a load derives from the payload.
fn acts_held(
    ocean_hierarchy: bool,
    hierarchy: bool,
    provinces: bool,
    is_lake: bool,
    temp_grid: bool,
) -> u8 {
    if ocean_hierarchy {
        6
    } else if hierarchy {
        5
    } else if provinces {
        4
    } else if is_lake {
        3
    } else if temp_grid {
        2
    } else {
        1
    }
}

/// The one world cache: every act a world has grown, up to act six.
///
/// A save holds acts one through whichever act the world had reached when it was
/// written. The heightmap is always there; each later artefact is an `Option` and
/// is present exactly when its own act has run:
///
/// ```text
///   heightmap                 1   the bare land
///   temp_grid, precip_grid    2   the atmosphere
///   flow_grid, is_lake        3   the water
///   provinces                 4   the land cut into provinces
///   hierarchy                 5   provinces folded into areas, regions, superregions
///   ocean, ocean_hierarchy    6   the world ocean, cut and folded the same way
/// ```
///
/// The deepest act is therefore read straight off the fields, which stay the one
/// truth. A save also stamps that number into its header so the atlas can describe
/// a cache without decoding it; the stamp is a hint for the list only, and a load
/// still derives the act from the fields. Saves overwrite one another, so a single
/// file (`world_<seed>.bin`) always holds the deepest checkpoint a seed has.
///
/// Polar ice is not stored: it is a deterministic function of the heightmap, the
/// lakes and the seed, resolved again on load so that a loaded world and a grown
/// one agree to the last bit.
#[derive(Clone, Serialize, Deserialize)]
pub struct WorldCache {
    pub width: usize,
    pub height: usize,
    pub heightmap: Grid<f32>,
    /// Act two onward.
    pub temp_grid: Option<Grid<f32>>,
    /// Act two onward.
    pub precip_grid: Option<Grid<f32>>,
    /// Act three onward.
    pub flow_grid: Option<Grid<f32>>,
    /// Act three onward.
    pub is_lake: Option<Grid<bool>>,
    /// Act four onward.
    pub provinces: Option<ProvinceResult>,
    /// Act five onward.
    pub hierarchy: Option<GeopoliticalHierarchy>,
    /// Act six onward.
    pub ocean: Option<OceanResult>,
    /// Act six onward.
    pub ocean_hierarchy: Option<GeopoliticalHierarchy>,
}

/// The world held by reference, for writing without copying a pixel of it.
///
/// The pipeline keeps every grid behind its own `Arc` and never holds an assembled
/// cache, and bincode writes struct *fields* in order, so this view serialises to
/// exactly the bytes [`WorldCache`] does.
#[derive(Serialize)]
pub struct WorldCacheView<'a> {
    pub width: usize,
    pub height: usize,
    pub heightmap: &'a Grid<f32>,
    pub temp_grid: Option<&'a Grid<f32>>,
    pub precip_grid: Option<&'a Grid<f32>>,
    pub flow_grid: Option<&'a Grid<f32>>,
    pub is_lake: Option<&'a Grid<bool>>,
    pub provinces: Option<&'a ProvinceResult>,
    pub hierarchy: Option<&'a GeopoliticalHierarchy>,
    pub ocean: Option<&'a OceanResult>,
    pub ocean_hierarchy: Option<&'a GeopoliticalHierarchy>,
}

impl WorldCacheView<'_> {
    /// The deepest act this save holds, `1..=6`, read off the fields present.
    ///
    /// The mirror of [`WorldCache::stage`], so a save stamps the header with the
    /// very act a load would derive from the payload it writes.
    pub fn stage(&self) -> u8 {
        acts_held(
            self.ocean_hierarchy.is_some(),
            self.hierarchy.is_some(),
            self.provinces.is_some(),
            self.is_lake.is_some(),
            self.temp_grid.is_some(),
        )
    }
}

impl WorldCache {
    /// The deepest act this save holds, `1..=6`, read off the fields present.
    ///
    /// A loaded world resumes at the act after this one, so this single number is
    /// what tells the pipeline how much of the world it already has.
    pub fn stage(&self) -> u8 {
        acts_held(
            self.ocean_hierarchy.is_some(),
            self.hierarchy.is_some(),
            self.provinces.is_some(),
            self.is_lake.is_some(),
            self.temp_grid.is_some(),
        )
    }

    /// Write a world held as separate parts, without assembling a copy of it.
    ///
    /// The file is the magic, one byte naming the deepest act held, then a zstd
    /// frame of the bincode payload. The grids are spatially coherent and the
    /// province ids are sparse, so the frame is a good deal smaller than the raw
    /// bytes.
    pub fn save_view(view: &WorldCacheView<'_>, path: &str) -> Result<(), String> {
        write_cache(view, view.stage(), path)
    }

    /// Load a world, reporting a missing or unreadable file instead of panicking:
    /// a reader who asks to load a world that is not there deserves a sentence, not
    /// a crash. A cache written by an older build (bare bincode, no magic) loads
    /// exactly as a framed one does.
    pub fn try_load(path: &str) -> Result<Self, String> {
        read_cache(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full world: every act's artefact present, all the way to the hierarchies.
    fn little_world() -> WorldCache {
        WorldCache {
            width: 3,
            height: 2,
            heightmap: Grid::filled_with(3, 2, 0.42f32),
            temp_grid: Some(Grid::filled_with(3, 2, -7.5f32)),
            precip_grid: Some(Grid::filled_with(3, 2, 0.11f32)),
            flow_grid: Some(Grid::filled_with(3, 2, 1_800.0f32)),
            is_lake: Some(Grid::filled_with(3, 2, false)),
            provinces: Some(ProvinceResult {
                province_ids: Grid::filled_with(3, 2, 1u32),
                habitability: Grid::filled_with(3, 2, 0.5f32),
                is_wasteland: Grid::filled_with(3, 2, false),
                num_provinces: 1,
            }),
            hierarchy: Some(GeopoliticalHierarchy::empty()),
            ocean: Some(OceanResult {
                ocean_province_ids: Grid::filled_with(3, 2, 0u32),
                num_provinces: 0,
            }),
            ocean_hierarchy: Some(GeopoliticalHierarchy::empty()),
        }
    }

    /// A full world large enough that compression is visible: a 256×128 canvas
    /// whose grids are constant, which a zstd frame folds down to almost nothing.
    fn compressible_world() -> WorldCache {
        let mut cache = little_world();
        cache.width = 256;
        cache.height = 128;
        cache.heightmap = Grid::filled_with(256, 128, 0.42f32);
        cache.temp_grid = Some(Grid::filled_with(256, 128, -7.5f32));
        cache.precip_grid = Some(Grid::filled_with(256, 128, 0.11f32));
        cache.flow_grid = Some(Grid::filled_with(256, 128, 1_800.0f32));
        cache.is_lake = Some(Grid::filled_with(256, 128, false));
        cache.provinces = Some(ProvinceResult {
            province_ids: Grid::filled_with(256, 128, 1u32),
            habitability: Grid::filled_with(256, 128, 0.5f32),
            is_wasteland: Grid::filled_with(256, 128, false),
            num_provinces: 1,
        });
        cache
    }

    /// The cache as the pipeline hands it over: every grid by reference.
    fn as_view(cache: &WorldCache) -> WorldCacheView<'_> {
        WorldCacheView {
            width: cache.width,
            height: cache.height,
            heightmap: &cache.heightmap,
            temp_grid: cache.temp_grid.as_ref(),
            precip_grid: cache.precip_grid.as_ref(),
            flow_grid: cache.flow_grid.as_ref(),
            is_lake: cache.is_lake.as_ref(),
            provinces: cache.provinces.as_ref(),
            hierarchy: cache.hierarchy.as_ref(),
            ocean: cache.ocean.as_ref(),
            ocean_hierarchy: cache.ocean_hierarchy.as_ref(),
        }
    }

    /// A written cache and a written view must be the same bytes: bincode is not
    /// self-describing, so the writer and the reader have to agree field for field.
    #[test]
    fn a_cache_written_as_a_view_is_the_cache_written_whole() {
        let cache = little_world();

        let whole = bincode::serialize(&cache).expect("serialize the cache");
        let view = bincode::serialize(&as_view(&cache)).expect("serialize the view");

        assert_eq!(whole, view, "the view must write exactly the cache's bytes");
    }

    /// A world written as a view must come back as the same world, tiers and all.
    #[test]
    fn a_world_written_as_a_view_loads_back_unchanged() {
        let cache = little_world();
        let path = std::env::temp_dir().join("project_genesis_cache_view_test.bin");
        let path = path.to_string_lossy().to_string();

        WorldCache::save_view(&as_view(&cache), &path).expect("write the world");
        let loaded = WorldCache::try_load(&path).expect("read the world back");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.width, cache.width);
        assert_eq!(loaded.height, cache.height);
        assert_eq!(loaded.heightmap.raw_slice(), cache.heightmap.raw_slice());
        assert_eq!(loaded.stage(), 6);
        assert_eq!(
            loaded
                .provinces
                .expect("the provinces came back")
                .num_provinces,
            1
        );
        assert!(loaded.hierarchy.is_some());
        assert!(loaded.ocean.is_some());
        assert!(loaded.ocean_hierarchy.is_some());
    }

    /// A world that stopped short of act six must come back with its tiers intact
    /// and its stage read off the fields that are there.
    #[test]
    fn a_partial_world_keeps_its_stage_through_a_round_trip() {
        let mut cache = little_world();
        cache.ocean = None;
        cache.ocean_hierarchy = None;
        assert_eq!(cache.stage(), 5, "provinces and the land hierarchy, no ocean");

        let path = std::env::temp_dir().join("project_genesis_partial_cache_test.bin");
        let path = path.to_string_lossy().to_string();
        WorldCache::save_view(&as_view(&cache), &path).expect("write the world");
        let loaded = WorldCache::try_load(&path).expect("read the world back");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.stage(), 5);
        assert!(loaded.provinces.is_some());
        assert!(loaded.hierarchy.is_some());
        assert!(loaded.ocean.is_none());
        assert_eq!(loaded.heightmap.raw_slice(), cache.heightmap.raw_slice());
    }

    /// The stage is a fact of the fields, so dropping them one act at a time walks
    /// it back from six to one.
    #[test]
    fn a_world_knows_which_act_it_reached() {
        let mut cache = little_world();
        assert_eq!(cache.stage(), 6);
        cache.ocean_hierarchy = None;
        assert_eq!(cache.stage(), 5);
        cache.hierarchy = None;
        assert_eq!(cache.stage(), 4);
        cache.provinces = None;
        assert_eq!(cache.stage(), 3);
        cache.is_lake = None;
        assert_eq!(cache.stage(), 2);
        cache.temp_grid = None;
        assert_eq!(cache.stage(), 1, "a bare heightmap is act one");
    }

    /// A written cache opens with the magic and is smaller than the payload it
    /// carries — which is the whole point of the frame wrapped around it.
    #[test]
    fn a_written_cache_wears_its_magic_and_beats_its_payload() {
        let cache = compressible_world();
        let path = std::env::temp_dir().join("project_genesis_compressed_cache_test.bin");
        let path = path.to_string_lossy().to_string();

        WorldCache::save_view(&as_view(&cache), &path).expect("write the world");

        let bytes = std::fs::read(&path).expect("read the raw file");
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            &bytes[..CACHE_MAGIC_ACTS.len()],
            CACHE_MAGIC_ACTS,
            "a written cache must open with the format-2 magic"
        );
        assert_eq!(
            bytes[CACHE_MAGIC_ACTS.len()],
            cache.stage(),
            "the byte after the magic names the deepest act held"
        );
        let payload = bincode::serialize(&cache).expect("serialize the payload");
        assert!(
            bytes.len() < payload.len(),
            "the frame must shrink the file: {} bytes on disk vs a {} byte payload",
            bytes.len(),
            payload.len()
        );
    }

    /// A cache written by an older build — bare bincode, no magic — still loads.
    #[test]
    fn a_legacy_uncompressed_cache_still_loads() {
        let cache = little_world();
        let path = std::env::temp_dir().join("project_genesis_legacy_cache_test.bin");
        let path = path.to_string_lossy().to_string();

        let payload = bincode::serialize(&cache).expect("serialize the legacy payload");
        std::fs::write(&path, &payload).expect("write the legacy cache");

        let loaded = WorldCache::try_load(&path).expect("read the legacy world back");
        assert_eq!(
            cache_stage(&path),
            None,
            "a bare bincode save has no header to name its acts"
        );
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.width, cache.width);
        assert_eq!(loaded.height, cache.height);
        assert_eq!(loaded.heightmap.raw_slice(), cache.heightmap.raw_slice());
        assert_eq!(loaded.stage(), 6);
    }

    /// The act a save holds is stamped in the header, so the atlas's list can read
    /// it from a handful of bytes — without decoding a pixel of the payload.
    #[test]
    fn an_acts_header_is_read_without_decoding_the_world() {
        let mut cache = little_world();
        cache.ocean = None;
        cache.ocean_hierarchy = None;
        assert_eq!(cache.stage(), 5);

        let path = std::env::temp_dir().join("project_genesis_acts_header_test.bin");
        let path = path.to_string_lossy().to_string();
        WorldCache::save_view(&as_view(&cache), &path).expect("write the world");

        assert_eq!(
            cache_stage(&path),
            Some(5),
            "the header names acts one to five without a decode"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A cache from before the acts header — a `PGZ1` frame — still loads, and
    /// answers `None` when the list asks how far it was grown.
    #[test]
    fn a_legacy_framed_cache_still_loads_without_an_acts_header() {
        let cache = little_world();
        let path = std::env::temp_dir().join("project_genesis_legacy_frame_test.bin");
        let path = path.to_string_lossy().to_string();

        // Hand-build what an older build wrote: the old magic, then a zstd frame of
        // the bincode payload, with no acts byte between them.
        let payload = bincode::serialize(&cache).expect("serialize the legacy payload");
        let mut file = File::create(&path).expect("create the legacy cache");
        file.write_all(CACHE_MAGIC).expect("write the old magic");
        let mut encoder =
            zstd::stream::write::Encoder::new(file, CACHE_ZSTD_LEVEL).expect("a zstd encoder");
        encoder.write_all(&payload).expect("write the frame");
        encoder.finish().expect("close the frame");

        let loaded = WorldCache::try_load(&path).expect("read the legacy world back");
        assert_eq!(loaded.stage(), 6);
        assert_eq!(
            cache_stage(&path),
            None,
            "a format-1 save names no acts, so the list says only that it is there"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A missing world is an error the atlas can show, never a panic.
    #[test]
    fn loading_a_world_that_is_not_there_is_an_error() {
        let missing = std::env::temp_dir().join("project_genesis_no_such_world.bin");
        assert!(
            WorldCache::try_load(&missing.to_string_lossy()).is_err(),
            "a missing cache must be reported, not panic"
        );
    }
}


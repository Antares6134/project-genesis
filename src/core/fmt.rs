//! Number formatting shared by every stage that puts a figure in front of a human.
//!
//! The engine reports real units everywhere — metres, km², m³/s, people, years BZ.
//! A figure of any size is only readable once its thousands are separated, and a
//! column of figures is only readable if they are all grouped the *same* way. One
//! helper therefore serves the hypsometric legend, the atlas scales and the
//! civilization dossiers, so a population can never be printed two different ways.

/// A float rendered with space-grouped thousands: `-5000.0` becomes `-5 000`.
///
/// Rounded to the nearest integer first, so a legend column never breaks its
/// alignment over a trailing `.4`.
pub fn thousands(v: f32) -> String {
    let rounded = v.round();
    let body = thousands_u64(rounded.abs() as u64);
    if rounded < 0.0 { format!("-{body}") } else { body }
}

/// An integer count rendered with space-grouped thousands: `4918220` becomes
/// `4 918 220`.
///
/// Kept exact for the whole `u64` range — unlike [`thousands`], which has to go
/// through `f32` — because it carries the one figure on the map that is summed
/// over millions of pixels: population.
pub fn thousands_u64(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(-5000.0), "-5 000");
        assert_eq!(thousands(200.0), "200");
        assert_eq!(thousands(0.0), "0");
        assert_eq!(thousands(-11_000.0), "-11 000");
    }

    #[test]
    fn counts_survive_f32_precision() {
        // 24 610 940 is not representable in f32, so the integer path must be
        // used for every population the atlas ever prints.
        assert_eq!(thousands_u64(24_610_940), "24 610 940");
        assert_eq!(thousands_u64(0), "0");
        assert_eq!(thousands_u64(999), "999");
        assert_eq!(thousands_u64(1_000), "1 000");
    }
}

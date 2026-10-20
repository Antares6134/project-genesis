use serde::{Deserialize, Serialize};

/// A 2D continuous grid that wraps horizontally (cylindrically) along the X-axis
/// and clamps at the poles along the Y-axis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grid<T> {
    pub width: usize,
    pub height: usize,
    data: Vec<T>,
}

impl<T: Clone + Default> Grid<T> {
    /// Creates a new grid initialized with default values.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![T::default(); width * height],
        }
    }

    /// Creates a new grid filled with a specific value.
    pub fn filled_with(width: usize, height: usize, value: T) -> Self {
        Self {
            width,
            height,
            data: vec![value; width * height],
        }
    }
}

impl<T> Grid<T> {
    #[inline(always)]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline(always)]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Wraps X horizontally around the cylindrical boundary.
    #[inline(always)]
    pub fn wrap_x(&self, x: isize) -> usize {
        let w = self.width as isize;
        ((x % w + w) % w) as usize
    }

    /// Clamps Y to the north and south poles.
    #[inline(always)]
    pub fn clamp_y(&self, y: isize) -> usize {
        y.clamp(0, (self.height - 1) as isize) as usize
    }

    /// Maps 2D coordinate to 1D index with cylindrical X wrapping and polar Y clamping.
    #[inline(always)]
    pub fn coords_to_index(&self, x: isize, y: isize) -> usize {
        let wx = self.wrap_x(x);
        let cy = self.clamp_y(y);
        cy * self.width + wx
    }

    #[inline(always)]
    pub fn get(&self, x: isize, y: isize) -> &T {
        let idx = self.coords_to_index(x, y);
        &self.data[idx]
    }

    #[inline(always)]
    pub fn get_mut(&mut self, x: isize, y: isize) -> &mut T {
        let idx = self.coords_to_index(x, y);
        &mut self.data[idx]
    }

    #[inline(always)]
    pub fn set(&mut self, x: isize, y: isize, val: T) {
        let idx = self.coords_to_index(x, y);
        self.data[idx] = val;
    }

    pub fn raw_slice(&self) -> &[T] {
        &self.data
    }

    pub fn raw_slice_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}

// Float-specific mathematical operations (Heightmaps)
impl Grid<f32> {
    /// Samples the height at continuous coordinates (fx, fy) using bilinear interpolation.
    /// This is crucial for droplet erosion to calculate sub-pixel movements smoothly.
    pub fn sample_bilinear(&self, fx: f32, fy: f32) -> f32 {
        let x0 = fx.floor() as isize;
        let y0 = fy.floor() as isize;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let tx = fx - fx.floor();
        let ty = fy - fy.floor();

        let h00 = *self.get(x0, y0);
        let h10 = *self.get(x1, y0);
        let h01 = *self.get(x0, y1);
        let h11 = *self.get(x1, y1);

        // Linear interpolation along X, then Y
        let top = h00 * (1.0 - tx) + h10 * tx;
        let bottom = h01 * (1.0 - tx) + h11 * tx;

        top * (1.0 - ty) + bottom * ty
    }

    /// Computes the surface gradient / slope vector (-dh/dx, -dh/dy) at continuous coordinates.
    pub fn gradient(&self, fx: f32, fy: f32) -> (f32, f32) {
        let x0 = fx.floor() as isize;
        let y0 = fy.floor() as isize;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let tx = fx - fx.floor();
        let ty = fy - fy.floor();

        let h00 = *self.get(x0, y0);
        let h10 = *self.get(x1, y0);
        let h01 = *self.get(x0, y1);
        let h11 = *self.get(x1, y1);

        // Slope along X and Y
        let dx = (h10 - h00) * (1.0 - ty) + (h11 - h01) * ty;
        let dy = (h01 - h00) * (1.0 - tx) + (h11 - h10) * tx;

        (dx, dy)
    }

    /// Normalizes all values in the grid between 0.0 and 1.0.
    pub fn normalize(&mut self) {
        let mut min_val = f32::MAX;
        let mut max_val = f32::MIN;

        for &val in &self.data {
            if val < min_val { min_val = val; }
            if val > max_val { max_val = val; }
        }

        let range = max_val - min_val;
        if range > 1e-6 {
            for val in &mut self.data {
                *val = (*val - min_val) / range;
            }
        }
    }
}
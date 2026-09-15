//! Reproducible numerical sampling; the seed is part of each retained request.
pub struct Random(pub u64);
impl Random {
    pub fn unit(&mut self) -> f64 {
        // xorshift64: nonzero seed, full period on its unsigned integer state.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // Midpoints of the representable 52-bit bins keep log inputs open.
        ((self.0 >> 12) as f64 + 0.5) / (1_u64 << 52) as f64
    }
    pub fn normal(&mut self) -> f64 {
        (-2. * self.unit().ln()).sqrt() * (std::f64::consts::TAU * self.unit()).cos()
    }
    pub fn index(&mut self, n: usize) -> usize {
        (self.unit() * n as f64) as usize
    }
}

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// 2D vector for physics calculations
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };
    pub const UP: Vec2 = Vec2 { x: 0.0, y: -1.0 };
    pub const DOWN: Vec2 = Vec2 { x: 0.0, y: 1.0 };
    pub const LEFT: Vec2 = Vec2 { x: -1.0, y: 0.0 };
    pub const RIGHT: Vec2 = Vec2 { x: 1.0, y: 0.0 };

    #[inline]
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn from_angle(angle: f32) -> Self {
        Self {
            x: angle.cos(),
            y: angle.sin(),
        }
    }

    #[inline]
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    #[inline]
    pub fn length_sq(&self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    #[inline]
    pub fn magnitude(&self) -> f32 {
        self.length()
    }

    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 0.0 {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        } else {
            Self::ZERO
        }
    }

    /// Returns normalized vector and original length
    pub fn normalize_with_length(&self) -> (Self, f32) {
        let len = self.length();
        if len > 0.0 {
            (
                Self {
                    x: self.x / len,
                    y: self.y / len,
                },
                len,
            )
        } else {
            (Self::ZERO, 0.0)
        }
    }

    #[inline]
    pub fn dot(&self, other: Vec2) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// 2D cross product (returns scalar z-component)
    #[inline]
    pub fn cross(&self, other: Vec2) -> f32 {
        self.x * other.y - self.y * other.x
    }

    #[inline]
    pub fn distance_to(&self, other: Vec2) -> f32 {
        (*self - other).length()
    }

    #[inline]
    pub fn distance_sq_to(&self, other: Vec2) -> f32 {
        (*self - other).length_sq()
    }

    pub fn clamp_length(&self, max: f32) -> Self {
        let len = self.length();
        if len > max && len > 0.0 {
            *self * (max / len)
        } else {
            *self
        }
    }

    pub fn clamp_length_min_max(&self, min: f32, max: f32) -> Self {
        let len = self.length();
        if len < min && len > 0.0 {
            *self * (min / len)
        } else if len > max && len > 0.0 {
            *self * (max / len)
        } else {
            *self
        }
    }

    pub fn lerp(&self, other: Vec2, t: f32) -> Self {
        *self + (other - *self) * t
    }

    pub fn rotate(&self, angle: f32) -> Self {
        let (sin, cos) = (angle.sin(), angle.cos());
        Self {
            x: self.x * cos - self.y * sin,
            y: self.x * sin + self.y * cos,
        }
    }

    /// Returns perpendicular vector (rotated 90 degrees counter-clockwise)
    pub fn perpendicular(&self) -> Self {
        Self {
            x: -self.y,
            y: self.x,
        }
    }

    /// Reflects vector off surface with given normal
    pub fn reflect(&self, normal: Vec2) -> Self {
        *self - normal * (2.0 * self.dot(normal))
    }

    /// Returns angle in radians
    pub fn angle(&self) -> f32 {
        self.y.atan2(self.x)
    }

    /// Returns angle between this vector and another
    pub fn angle_to(&self, other: Vec2) -> f32 {
        (self.cross(other)).atan2(self.dot(other))
    }

    /// Component-wise min
    pub fn min(&self, other: Vec2) -> Self {
        Self {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
        }
    }

    /// Component-wise max
    pub fn max(&self, other: Vec2) -> Self {
        Self {
            x: self.x.max(other.x),
            y: self.y.max(other.y),
        }
    }

    /// Component-wise abs
    pub fn abs(&self) -> Self {
        Self {
            x: self.x.abs(),
            y: self.y.abs(),
        }
    }

    /// Check if vector is approximately zero
    pub fn is_zero(&self, epsilon: f32) -> bool {
        self.x.abs() < epsilon && self.y.abs() < epsilon
    }

    /// Check if vector is approximately equal to another
    pub fn approx_eq(&self, other: Vec2, epsilon: f32) -> bool {
        (self.x - other.x).abs() < epsilon && (self.y - other.y).abs() < epsilon
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, rhs: Vec2) -> Vec2 {
        Vec2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_new() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.x, 3.0);
        assert_eq!(v.y, 4.0);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Vec2::ZERO, Vec2::new(0.0, 0.0));
        assert_eq!(Vec2::ONE, Vec2::new(1.0, 1.0));
        assert_eq!(Vec2::UP, Vec2::new(0.0, -1.0));
        assert_eq!(Vec2::RIGHT, Vec2::new(1.0, 0.0));
    }

    #[test]
    fn test_length() {
        let v = Vec2::new(3.0, 4.0);
        assert!(approx_eq(v.length(), 5.0));
        assert!(approx_eq(v.length_sq(), 25.0));
        assert!(approx_eq(v.magnitude(), 5.0));
    }

    #[test]
    fn test_length_zero() {
        assert!(approx_eq(Vec2::ZERO.length(), 0.0));
    }

    #[test]
    fn test_normalize() {
        let v = Vec2::new(3.0, 4.0);
        let n = v.normalize();
        assert!(approx_eq(n.length(), 1.0));
        assert!(approx_eq(n.x, 0.6));
        assert!(approx_eq(n.y, 0.8));
    }

    #[test]
    fn test_normalize_zero() {
        let v = Vec2::ZERO.normalize();
        assert_eq!(v, Vec2::ZERO);
    }

    #[test]
    fn test_normalize_with_length() {
        let v = Vec2::new(3.0, 4.0);
        let (n, len) = v.normalize_with_length();
        assert!(approx_eq(len, 5.0));
        assert!(approx_eq(n.length(), 1.0));
    }

    #[test]
    fn test_dot() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 4.0);
        assert!(approx_eq(a.dot(b), 11.0));
    }

    #[test]
    fn test_dot_perpendicular() {
        let a = Vec2::new(1.0, 0.0);
        let b = Vec2::new(0.0, 1.0);
        assert!(approx_eq(a.dot(b), 0.0));
    }

    #[test]
    fn test_cross() {
        let a = Vec2::new(1.0, 0.0);
        let b = Vec2::new(0.0, 1.0);
        assert!(approx_eq(a.cross(b), 1.0));
        assert!(approx_eq(b.cross(a), -1.0));
    }

    #[test]
    fn test_distance() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(3.0, 4.0);
        assert!(approx_eq(a.distance_to(b), 5.0));
        assert!(approx_eq(a.distance_sq_to(b), 25.0));
    }

    #[test]
    fn test_clamp_length() {
        let v = Vec2::new(6.0, 8.0); // length = 10
        let clamped = v.clamp_length(5.0);
        assert!(approx_eq(clamped.length(), 5.0));
        assert!(approx_eq(clamped.x, 3.0));
        assert!(approx_eq(clamped.y, 4.0));
    }

    #[test]
    fn test_clamp_length_no_change() {
        let v = Vec2::new(3.0, 4.0); // length = 5
        let clamped = v.clamp_length(10.0);
        assert!(approx_eq(clamped.x, 3.0));
        assert!(approx_eq(clamped.y, 4.0));
    }

    #[test]
    fn test_lerp() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(10.0, 10.0);
        let mid = a.lerp(b, 0.5);
        assert!(approx_eq(mid.x, 5.0));
        assert!(approx_eq(mid.y, 5.0));
    }

    #[test]
    fn test_lerp_edges() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(10.0, 10.0);
        assert!(a.lerp(b, 0.0).approx_eq(a, EPSILON));
        assert!(a.lerp(b, 1.0).approx_eq(b, EPSILON));
    }

    #[test]
    fn test_rotate() {
        let v = Vec2::new(1.0, 0.0);
        let rotated = v.rotate(PI / 2.0);
        assert!(approx_eq(rotated.x, 0.0));
        assert!(approx_eq(rotated.y, 1.0));
    }

    #[test]
    fn test_rotate_180() {
        let v = Vec2::new(1.0, 0.0);
        let rotated = v.rotate(PI);
        assert!(approx_eq(rotated.x, -1.0));
        assert!(approx_eq(rotated.y, 0.0));
    }

    #[test]
    fn test_from_angle() {
        let v = Vec2::from_angle(0.0);
        assert!(approx_eq(v.x, 1.0));
        assert!(approx_eq(v.y, 0.0));

        let v = Vec2::from_angle(PI / 2.0);
        assert!(approx_eq(v.x, 0.0));
        assert!(approx_eq(v.y, 1.0));
    }

    #[test]
    fn test_perpendicular() {
        let v = Vec2::new(1.0, 0.0);
        let p = v.perpendicular();
        assert!(approx_eq(p.x, 0.0));
        assert!(approx_eq(p.y, 1.0));
        assert!(approx_eq(v.dot(p), 0.0));
    }

    #[test]
    fn test_reflect() {
        let v = Vec2::new(1.0, -1.0).normalize();
        let normal = Vec2::new(0.0, 1.0);
        let reflected = v.reflect(normal);
        assert!(approx_eq(reflected.x, v.x));
        assert!(approx_eq(reflected.y, -v.y));
    }

    #[test]
    fn test_angle() {
        assert!(approx_eq(Vec2::new(1.0, 0.0).angle(), 0.0));
        assert!(approx_eq(Vec2::new(0.0, 1.0).angle(), PI / 2.0));
        assert!(approx_eq(Vec2::new(-1.0, 0.0).angle(), PI));
    }

    #[test]
    fn test_add() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 4.0);
        let c = a + b;
        assert_eq!(c, Vec2::new(4.0, 6.0));
    }

    #[test]
    fn test_sub() {
        let a = Vec2::new(5.0, 7.0);
        let b = Vec2::new(2.0, 3.0);
        let c = a - b;
        assert_eq!(c, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn test_mul_scalar() {
        let a = Vec2::new(2.0, 3.0);
        let b = a * 2.0;
        assert_eq!(b, Vec2::new(4.0, 6.0));
    }

    #[test]
    fn test_scalar_mul() {
        let a = Vec2::new(2.0, 3.0);
        let b = 2.0 * a;
        assert_eq!(b, Vec2::new(4.0, 6.0));
    }

    #[test]
    fn test_neg() {
        let a = Vec2::new(2.0, -3.0);
        let b = -a;
        assert_eq!(b, Vec2::new(-2.0, 3.0));
    }

    #[test]
    fn test_add_assign() {
        let mut a = Vec2::new(1.0, 2.0);
        a += Vec2::new(3.0, 4.0);
        assert_eq!(a, Vec2::new(4.0, 6.0));
    }

    #[test]
    fn test_sub_assign() {
        let mut a = Vec2::new(5.0, 7.0);
        a -= Vec2::new(2.0, 3.0);
        assert_eq!(a, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn test_mul_assign() {
        let mut a = Vec2::new(2.0, 3.0);
        a *= 2.0;
        assert_eq!(a, Vec2::new(4.0, 6.0));
    }

    #[test]
    fn test_min_max() {
        let a = Vec2::new(1.0, 5.0);
        let b = Vec2::new(3.0, 2.0);
        assert_eq!(a.min(b), Vec2::new(1.0, 2.0));
        assert_eq!(a.max(b), Vec2::new(3.0, 5.0));
    }

    #[test]
    fn test_abs() {
        let v = Vec2::new(-3.0, -4.0);
        assert_eq!(v.abs(), Vec2::new(3.0, 4.0));
    }

    #[test]
    fn test_is_zero() {
        assert!(Vec2::ZERO.is_zero(EPSILON));
        assert!(Vec2::new(1e-6, 1e-6).is_zero(1e-5));
        assert!(!Vec2::new(1.0, 0.0).is_zero(EPSILON));
    }

    #[test]
    fn test_serde() {
        let v = Vec2::new(1.5, 2.5);
        let encoded =
            bincode::serde::encode_to_vec(&v, bincode::config::standard()).unwrap();
        let (decoded, _): (Vec2, usize) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(v, decoded);
    }
}

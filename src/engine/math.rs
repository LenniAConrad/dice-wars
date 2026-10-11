use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}
impl Vec2 {
    pub const ZERO: Self = vec2(0.0, 0.0);
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }
    pub fn normalize_or_zero(self) -> Self {
        let length = self.length();
        if length > 0.0 {
            self / length
        } else {
            Self::ZERO
        }
    }
}
impl Add for Vec2 {
    type Output = Self;
    fn add(self, v: Self) -> Self {
        vec2(self.x + v.x, self.y + v.y)
    }
}
impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, v: Self) -> Self {
        vec2(self.x - v.x, self.y - v.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, k: f32) -> Self {
        vec2(self.x * k, self.y * k)
    }
}
impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        v * self
    }
}
impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, k: f32) -> Self {
        vec2(self.x / k, self.y / k)
    }
}
impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        self * -1.0
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, v: Self) {
        *self = *self + v;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, p: Vec2) -> bool {
        p.x >= self.x && p.x <= self.x + self.w && p.y >= self.y && p.y <= self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
impl Color {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }
    pub fn bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a].map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
    }
}
pub const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);
pub const BLACK: Color = Color::new(0.0, 0.0, 0.0, 1.0);

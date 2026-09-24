//! CPU particle effects powering footsteps, landings, jetpacks, thrusters,
//! mining sparks, machine exhaust, splashes, healing sparkles and more.
//!
//! The effects are deliberately voxel-flavoured: most styles are tiny cubes,
//! while additive/glow styles use billboarded quads with a soft radial texture.
//! Materials are cached by (quantized color, alpha bucket, blend mode) so a
//! burst of 40 particles shares a handful of GPU materials. Every emitter is a
//! plain method on [`ParticleSystem`], which keeps call sites short inside the
//! already parameter-heavy gameplay systems.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::collections::HashMap;

use crate::rng::Rng;
use crate::tween::{clamp01, lerp};

/// Hard cap on live particles. Emitters silently drop additions past the cap so
/// a pathological frame (huge explosion in a storm) cannot tank the frame rate.
pub const PARTICLE_CAP: usize = 1_100;

/// Material alpha is quantized into this many buckets; swapping the shared
/// material handle when the bucket changes gives smooth-looking fades without a
/// material per particle.
const ALPHA_BUCKETS: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Shape {
    Cube,
    Slab,
    Quad,
}

/// Per-style tuning used by every emitter. Call sites override individual
/// fields through [`EmitOptions`] when they need to.
#[derive(Clone, Copy, Debug)]
pub struct StyleSpec {
    pub shape: Shape,
    pub color: (f32, f32, f32),
    pub color2: (f32, f32, f32),
    pub additive: bool,
    pub gravity: f32,
    pub drag: f32,
    pub life: (f32, f32),
    pub size: (f32, f32),
    pub speed: (f32, f32),
    /// 0 = tight cone around `dir`, 1 = hemisphere, 2 = full sphere.
    pub spread: f32,
    pub turbulence: f32,
    pub spin: (f32, f32),
    pub ground: bool,
    pub bounce: f32,
    pub billboard: bool,
    pub alpha: f32,
    /// Fraction of the lifetime spent fading in (rest fades out).
    pub fade_in: f32,
    /// Linear multiplier used for additive styles so bloom picks them up.
    pub brightness: f32,
}

impl StyleSpec {
    const fn base(shape: Shape, color: (f32, f32, f32)) -> Self {
        Self {
            shape,
            color,
            color2: color,
            additive: false,
            gravity: 9.0,
            drag: 1.2,
            life: (0.4, 0.8),
            size: (0.06, 0.03),
            speed: (0.5, 1.6),
            spread: 1.0,
            turbulence: 0.0,
            spin: (0.0, 2.0),
            ground: false,
            bounce: 0.3,
            billboard: false,
            alpha: 1.0,
            fade_in: 0.08,
            brightness: 1.0,
        }
    }

    fn blurred(mut self, amount: f32) -> Self {
        self.turbulence = amount;
        self
    }

    fn rising(mut self, gravity: f32, life: (f32, f32), size: (f32, f32)) -> Self {
        self.gravity = gravity;
        self.life = life;
        self.size = size;
        self.drag = 1.4;
        self
    }
}

/// Named particle presets. Keeping this as an enum (instead of free-form specs)
/// lets the material cache key a style by a single byte.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ParticleStyle {
    Dust,
    Sand,
    Snow,
    Leaf,
    Spore,
    Smoke,
    Steam,
    Toxic,
    Bubble,
    Splash,
    Spark,
    Ember,
    Flame,
    Thruster,
    Exhaust,
    Glow,
    Heal,
    Shield,
    Electric,
    Blood,
    Debris,
    Frost,
    Rainbow,
}

impl ParticleStyle {
    pub fn spec(self) -> StyleSpec {
        match self {
            Self::Dust => StyleSpec::base(Shape::Cube, (0.58, 0.51, 0.42))
                .blurred(0.5)
                .rising(5.5, (0.5, 1.0), (0.07, 0.14)),
            Self::Sand => StyleSpec {
                color: (0.86, 0.75, 0.5),
                color2: (0.72, 0.6, 0.38),
                ..StyleSpec::base(Shape::Cube, (0.86, 0.75, 0.5))
                    .blurred(0.35)
                    .rising(7.0, (0.4, 0.9), (0.06, 0.12))
            },
            Self::Snow => StyleSpec {
                speed: (0.4, 1.4),
                spread: 2.0,
                ..StyleSpec::base(Shape::Cube, (0.96, 0.98, 1.0))
                    .blurred(0.8)
                    .rising(1.6, (1.6, 3.2), (0.07, 0.11))
            },
            Self::Leaf => StyleSpec {
                shape: Shape::Slab,
                color2: (0.62, 0.78, 0.3),
                spin: (2.0, 7.0),
                speed: (0.3, 1.2),
                spread: 2.0,
                ..StyleSpec::base(Shape::Slab, (0.36, 0.68, 0.26)).blurred(1.1)
            },
            Self::Spore => StyleSpec {
                additive: true,
                brightness: 1.35,
                billboard: true,
                shape: Shape::Quad,
                color: (0.66, 0.42, 0.9),
                speed: (0.1, 0.5),
                spread: 2.0,
                spin: (0.0, 0.5),
                ..StyleSpec::base(Shape::Quad, (0.66, 0.42, 0.9))
                    .blurred(0.9)
                    .rising(-0.22, (2.2, 4.6), (0.09, 0.16))
            },
            Self::Smoke => StyleSpec {
                alpha: 0.42,
                color: (0.32, 0.33, 0.36),
                color2: (0.18, 0.19, 0.22),
                turbulence: 0.9,
                ..StyleSpec::base(Shape::Cube, (0.32, 0.33, 0.36)).rising(
                    -0.5,
                    (1.2, 2.4),
                    (0.16, 0.5),
                )
            },
            Self::Steam => StyleSpec {
                alpha: 0.5,
                color: (0.9, 0.95, 0.98),
                turbulence: 0.7,
                ..StyleSpec::base(Shape::Cube, (0.9, 0.95, 0.98)).rising(
                    -0.9,
                    (1.0, 2.0),
                    (0.1, 0.34),
                )
            },
            Self::Toxic => StyleSpec {
                additive: true,
                brightness: 1.1,
                billboard: true,
                shape: Shape::Quad,
                color: (0.42, 0.95, 0.4),
                turbulence: 1.2,
                alpha: 0.5,
                ..StyleSpec::base(Shape::Quad, (0.42, 0.95, 0.4)).rising(
                    -0.35,
                    (1.4, 2.6),
                    (0.14, 0.36),
                )
            },
            Self::Bubble => StyleSpec {
                shape: Shape::Quad,
                billboard: true,
                color: (0.6, 0.85, 1.0),
                alpha: 0.55,
                speed: (0.2, 0.8),
                spread: 2.0,
                ..StyleSpec::base(Shape::Quad, (0.6, 0.85, 1.0))
                    .blurred(0.5)
                    .rising(-1.1, (1.2, 2.4), (0.06, 0.13))
            },
            Self::Splash => StyleSpec {
                color: (0.68, 0.86, 1.0),
                color2: (0.5, 0.72, 0.95),
                gravity: 13.0,
                speed: (1.4, 4.2),
                spread: 0.7,
                ground: true,
                alpha: 0.85,
                ..StyleSpec::base(Shape::Cube, (0.68, 0.86, 1.0)).rising(
                    13.0,
                    (0.35, 0.7),
                    (0.05, 0.02),
                )
            },
            Self::Spark => StyleSpec {
                additive: true,
                brightness: 2.4,
                billboard: true,
                shape: Shape::Quad,
                color: (1.0, 0.82, 0.32),
                color2: (1.0, 0.5, 0.16),
                gravity: 12.0,
                drag: 2.4,
                life: (0.25, 0.6),
                size: (0.16, 0.05),
                speed: (2.6, 7.5),
                spread: 2.0,
                ground: true,
                bounce: 0.4,
                fade_in: 0.0,
                ..StyleSpec::base(Shape::Quad, (1.0, 0.82, 0.32))
            },
            Self::Ember => StyleSpec {
                additive: true,
                brightness: 1.8,
                billboard: true,
                shape: Shape::Quad,
                color: (1.0, 0.55, 0.2),
                turbulence: 1.6,
                speed: (0.3, 1.4),
                spread: 2.0,
                fade_in: 0.05,
                ..StyleSpec::base(Shape::Quad, (1.0, 0.55, 0.2)).rising(
                    -0.6,
                    (2.0, 4.0),
                    (0.09, 0.01),
                )
            },
            Self::Flame => StyleSpec {
                additive: true,
                brightness: 1.9,
                billboard: true,
                shape: Shape::Quad,
                color: (1.0, 0.72, 0.26),
                color2: (1.0, 0.32, 0.08),
                turbulence: 1.1,
                speed: (0.4, 1.8),
                spread: 0.9,
                fade_in: 0.0,
                ..StyleSpec::base(Shape::Quad, (1.0, 0.72, 0.26)).rising(
                    -1.6,
                    (0.35, 0.8),
                    (0.2, 0.02),
                )
            },
            Self::Thruster => StyleSpec {
                additive: true,
                brightness: 2.1,
                billboard: true,
                shape: Shape::Quad,
                color: (0.62, 0.92, 1.0),
                color2: (0.35, 0.62, 1.0),
                turbulence: 0.6,
                drag: 0.9,
                speed: (3.0, 9.0),
                spread: 0.18,
                fade_in: 0.0,
                ..StyleSpec::base(Shape::Quad, (0.62, 0.92, 1.0)).rising(
                    -0.6,
                    (0.3, 0.7),
                    (0.24, 0.05),
                )
            },
            Self::Exhaust => StyleSpec {
                alpha: 0.5,
                color: (0.55, 0.57, 0.62),
                color2: (0.34, 0.36, 0.4),
                turbulence: 0.7,
                speed: (1.2, 3.4),
                spread: 0.45,
                ..StyleSpec::base(Shape::Cube, (0.55, 0.57, 0.62)).rising(
                    -1.2,
                    (0.7, 1.5),
                    (0.12, 0.4),
                )
            },
            Self::Glow => StyleSpec {
                additive: true,
                brightness: 1.6,
                billboard: true,
                shape: Shape::Quad,
                color: (0.75, 0.95, 1.0),
                speed: (0.05, 0.5),
                spread: 2.0,
                fade_in: 0.1,
                ..StyleSpec::base(Shape::Quad, (0.75, 0.95, 1.0)).rising(
                    -0.25,
                    (1.0, 2.2),
                    (0.12, 0.02),
                )
            },
            Self::Heal => StyleSpec {
                additive: true,
                brightness: 1.7,
                billboard: true,
                shape: Shape::Quad,
                color: (0.45, 1.0, 0.55),
                speed: (0.4, 2.0),
                spread: 0.4,
                fade_in: 0.1,
                ..StyleSpec::base(Shape::Quad, (0.45, 1.0, 0.55)).rising(
                    -1.0,
                    (0.8, 1.6),
                    (0.1, 0.02),
                )
            },
            Self::Shield => StyleSpec {
                additive: true,
                brightness: 1.8,
                billboard: true,
                shape: Shape::Quad,
                color: (0.4, 0.9, 1.0),
                speed: (1.2, 4.0),
                spread: 1.6,
                fade_in: 0.0,
                ground: false,
                ..StyleSpec::base(Shape::Quad, (0.4, 0.9, 1.0)).rising(0.0, (0.5, 1.0), (0.3, 0.05))
            },
            Self::Electric => StyleSpec {
                additive: true,
                brightness: 2.6,
                billboard: true,
                shape: Shape::Quad,
                color: (0.55, 0.95, 1.0),
                turbulence: 2.4,
                speed: (2.0, 6.0),
                spread: 2.0,
                life: (0.15, 0.4),
                size: (0.14, 0.02),
                fade_in: 0.0,
                ..StyleSpec::base(Shape::Quad, (0.55, 0.95, 1.0))
            },
            Self::Blood => StyleSpec {
                color: (0.62, 0.16, 0.18),
                color2: (0.4, 0.1, 0.12),
                gravity: 11.0,
                speed: (0.8, 3.0),
                spread: 0.8,
                ground: true,
                ..StyleSpec::base(Shape::Cube, (0.62, 0.16, 0.18)).rising(
                    11.0,
                    (0.4, 0.8),
                    (0.07, 0.03),
                )
            },
            Self::Debris => StyleSpec {
                color: (0.6, 0.55, 0.48),
                speed: (1.5, 4.5),
                spread: 2.0,
                ground: true,
                bounce: 0.35,
                ..StyleSpec::base(Shape::Cube, (0.6, 0.55, 0.48)).rising(
                    12.0,
                    (0.5, 1.1),
                    (0.09, 0.05),
                )
            },
            Self::Frost => StyleSpec {
                color: (0.78, 0.92, 1.0),
                alpha: 0.8,
                turbulence: 1.4,
                speed: (0.4, 1.6),
                spread: 2.0,
                ..StyleSpec::base(Shape::Cube, (0.78, 0.92, 1.0)).rising(
                    1.2,
                    (0.8, 1.8),
                    (0.07, 0.13),
                )
            },
            Self::Rainbow => StyleSpec {
                additive: true,
                brightness: 1.9,
                billboard: true,
                shape: Shape::Quad,
                color: (1.0, 0.8, 0.4),
                color2: (0.5, 0.8, 1.0),
                speed: (0.6, 2.6),
                spread: 2.0,
                fade_in: 0.05,
                ..StyleSpec::base(Shape::Quad, (1.0, 0.8, 0.4)).rising(
                    -0.4,
                    (0.9, 1.8),
                    (0.12, 0.02),
                )
            },
        }
    }
}

/// Emitter overrides on top of a [`ParticleStyle`] preset.
#[derive(Clone, Copy, Debug)]
pub struct EmitOptions {
    /// Main emission direction; used together with `spread`.
    pub dir: Vec3,
    /// Explicit velocity injected after the random speed; doubles as a size
    /// multiplier for moving emitters (walking speed dust, ship thrust).
    pub velocity: Vec3,
    pub count: u32,
    pub speed: (f32, f32),
    pub spread: f32,
    pub color: Option<(f32, f32, f32)>,
    pub size_scale: f32,
    pub life_scale: f32,
    pub alpha: Option<f32>,
    pub ground: Option<bool>,
}

impl Default for EmitOptions {
    fn default() -> Self {
        Self {
            dir: Vec3::Y,
            velocity: Vec3::ZERO,
            count: 1,
            speed: (-1.0, -1.0),
            spread: -1.0,
            color: None,
            size_scale: 1.0,
            life_scale: 1.0,
            alpha: None,
            ground: None,
        }
    }
}

impl EmitOptions {
    pub fn count(mut self, count: u32) -> Self {
        self.count = count;
        self
    }

    pub fn dir(mut self, dir: Vec3) -> Self {
        self.dir = dir;
        self
    }

    pub fn velocity(mut self, velocity: Vec3) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn speed(mut self, min: f32, max: f32) -> Self {
        self.speed = (min, max);
        self
    }

    pub fn spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }

    pub fn color(mut self, r: f32, g: f32, b: f32) -> Self {
        self.color = Some((r, g, b));
        self
    }

    pub fn size_scale(mut self, scale: f32) -> Self {
        self.size_scale = scale;
        self
    }

    pub fn life_scale(mut self, scale: f32) -> Self {
        self.life_scale = scale;
        self
    }

    pub fn alpha(mut self, alpha: f32) -> Self {
        self.alpha = Some(alpha);
        self
    }
}

/// A live particle. Simulation only touches this plus [`Transform`].
#[derive(Component)]
pub struct Particle {
    pub style: ParticleStyle,
    pub velocity: Vec3,
    pub life: f32,
    pub max_life: f32,
    pub start_scale: f32,
    pub end_scale: f32,
    pub gravity: f32,
    pub drag: f32,
    pub turbulence: f32,
    pub spin: Vec3,
    pub color: (f32, f32, f32),
    pub alpha: f32,
    pub additive: bool,
    pub billboard: bool,
    pub ground: bool,
    pub bounce: f32,
    pub fade_in: f32,
    pub bucket: u8,
    pub seed: u32,
}

/// Cached meshes/materials used by every emitter.
#[derive(Resource, Default)]
pub struct ParticleCache {
    pub meshes: HashMap<Shape, Handle<Mesh>>,
    pub soft_texture: Option<Handle<Image>>,
    pub materials: HashMap<u32, Handle<StandardMaterial>>,
    pub active: usize,
}

/// 15-bit quantized color (5 bits per channel) packed into bits 10..25.
fn color_key(r: f32, g: f32, b: f32) -> u32 {
    let q = |v: f32| (clamp01(v) * 31.0).round() as u32;
    (q(r) << 10) | (q(g) << 5) | q(b)
}

/// Material cache key: style (5 bits) | additive (1) | alpha bucket (4) | color (15).
fn material_key(style: ParticleStyle, color: (f32, f32, f32), bucket: u8, additive: bool) -> u32 {
    ((style as u32) & 0x1F)
        | ((additive as u32) << 5)
        | ((bucket as u32 & 0xF) << 6)
        | (color_key(color.0, color.1, color.2) << 10)
}

/// Soft radial falloff used by additive billboards. White RGB, alpha falls off
/// from the centre so overlapping sparks accumulate into a warm core.
fn soft_texture() -> Image {
    const SIZE: usize = 32;
    let mut bytes = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let fy = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let d = (fx * fx + fy * fy).sqrt();
            let alpha = clamp01(1.0 - d);
            let alpha = alpha * alpha * (3.0 - 2.0 * alpha);
            let offset = (y * SIZE + x) * 4;
            bytes[offset] = 255;
            bytes[offset + 1] = 255;
            bytes[offset + 2] = 255;
            bytes[offset + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        bytes,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
}

/// Converts an authored particle colour into the form stored by the material.
///
/// Blend styles are authored in sRGB (so a "brown dust" value actually looks
/// brown); treating those numbers as linear rendered every chip and dust mote
/// washed-out white. Additive styles are deliberately linear HDR with a
/// brightness boost so bloom can pick them up.
pub fn particle_base_color(
    color: (f32, f32, f32),
    brightness: f32,
    additive: bool,
) -> bevy::color::Srgba {
    if additive {
        Color::linear_rgb(
            color.0 * brightness,
            color.1 * brightness,
            color.2 * brightness,
        )
        .to_srgba()
    } else {
        Color::srgb(color.0, color.1, color.2).to_srgba()
    }
}

impl ParticleCache {
    fn ensure_assets(
        &mut self,
        meshes: &mut Assets<Mesh>,
        _materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) {
        self.meshes
            .entry(Shape::Cube)
            .or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0)));
        self.meshes
            .entry(Shape::Slab)
            .or_insert_with(|| meshes.add(Cuboid::new(1.0, 0.16, 0.62)));
        self.meshes
            .entry(Shape::Quad)
            .or_insert_with(|| meshes.add(Plane3d::default().mesh().size(1.0, 1.0)));
        if self.soft_texture.is_none() {
            self.soft_texture = Some(images.add(soft_texture()));
        }
    }

    pub fn material(
        &mut self,
        style: ParticleStyle,
        color: (f32, f32, f32),
        alpha: f32,
        additive: bool,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Handle<StandardMaterial> {
        self.ensure_assets(meshes, materials, images);
        let bucket =
            ((clamp01(alpha) * (ALPHA_BUCKETS - 1) as f32).round() as u8).min(ALPHA_BUCKETS - 1);
        let key = material_key(style, color, bucket, additive);
        self.materials
            .entry(key)
            .or_insert_with(|| {
                let a = bucket as f32 / (ALPHA_BUCKETS - 1) as f32;
                let spec = style.spec();
                let brightness = if additive {
                    spec.brightness.max(1.0)
                } else {
                    1.0
                };
                let mut base = particle_base_color(color, brightness, additive);
                base.alpha = a;
                let mut material = StandardMaterial {
                    base_color: Color::Srgba(base),
                    unlit: true,
                    alpha_mode: if additive {
                        AlphaMode::Add
                    } else {
                        AlphaMode::Blend
                    },
                    double_sided: true,
                    cull_mode: None,
                    fog_enabled: false,
                    ..default()
                };
                if spec.shape == Shape::Quad
                    && let Some(texture) = &self.soft_texture
                {
                    material.base_color_texture = Some(texture.clone());
                }
                materials.add(material)
            })
            .clone()
    }
}

/// Bundled access to everything an emitter needs. Dropped directly into systems
/// as a single `mut fx: ParticleSystem` parameter. Asset resources are optional
/// so unit-test apps that do not register the render asset stores simply skip
/// particle emission instead of panicking.
#[derive(SystemParam)]
pub struct ParticleSystem<'w, 's> {
    pub commands: Commands<'w, 's>,
    meshes: Option<ResMut<'w, Assets<Mesh>>>,
    materials: Option<ResMut<'w, Assets<StandardMaterial>>>,
    images: Option<ResMut<'w, Assets<Image>>>,
    cache: Option<ResMut<'w, ParticleCache>>,
}

impl ParticleSystem<'_, '_> {
    pub fn live(&self) -> usize {
        self.cache.as_ref().map(|cache| cache.active).unwrap_or(0)
    }

    /// Creates an ad-hoc mesh (used by effect systems that need a custom
    /// shape, e.g. meteors) without taking a second `Assets<Mesh>` parameter
    /// that would conflict with the bundled system param.
    pub fn add_mesh(&mut self, mesh: Mesh) -> Handle<Mesh> {
        match self.meshes.as_mut() {
            Some(meshes) => meshes.add(mesh),
            None => Handle::default(),
        }
    }

    /// Ad-hoc material counterpart to [`Self::add_mesh`].
    pub fn add_material(&mut self, material: StandardMaterial) -> Handle<StandardMaterial> {
        match self.materials.as_mut() {
            Some(materials) => materials.add(material),
            None => Handle::default(),
        }
    }

    /// True when the render asset stores are available (always true in-game).
    pub fn enabled(&self) -> bool {
        self.cache.is_some() && self.meshes.is_some() && self.materials.is_some()
    }

    /// Core emitter: samples the style spec, merges the overrides and spawns.
    pub fn emit(&mut self, style: ParticleStyle, pos: Vec3, options: EmitOptions) -> u32 {
        let (Some(meshes), Some(materials), Some(images), Some(cache)) = (
            self.meshes.as_deref_mut(),
            self.materials.as_deref_mut(),
            self.images.as_deref_mut(),
            self.cache.as_deref_mut(),
        ) else {
            return 0;
        };
        if cache.active >= PARTICLE_CAP {
            return 0;
        }
        let spec = style.spec();
        let mut rng = Rng::new(hash_position(pos) ^ (style as u32).wrapping_mul(0x9E37_79B9));
        let speed = if options.speed.0 >= 0.0 {
            options.speed
        } else {
            spec.speed
        };
        let spread = if options.spread >= 0.0 {
            options.spread
        } else {
            spec.spread
        };
        let budget = PARTICLE_CAP.saturating_sub(cache.active);
        let count = options.count.min(budget as u32);
        cache.ensure_assets(meshes, materials, images);
        let mesh = cache
            .meshes
            .get(&spec.shape)
            .cloned()
            .expect("particle shapes are ensured above");
        let dir = if options.dir.length_squared() > 1e-6 {
            options.dir.normalize()
        } else {
            Vec3::Y
        };
        for _ in 0..count {
            let mut velocity = match spread as i32 {
                0 => {
                    // Tight cone: random already-biased direction.
                    let tangent = Vec3::new(rng.next() - 0.5, rng.next() - 0.5, rng.next() - 0.5);
                    (dir + tangent * 0.25).normalize_or_zero() * rng.range_f(speed.0, speed.1)
                }
                1 => {
                    let tangent = Vec3::new(rng.next() - 0.5, rng.next() - 0.5, rng.next() - 0.5);
                    (dir + tangent * 0.9).normalize_or_zero() * rng.range_f(speed.0, speed.1)
                }
                _ => {
                    let theta = rng.range_f(0.0, std::f32::consts::TAU);
                    let z = rng.range_f(-1.0, 1.0);
                    let r = (1.0 - z * z).max(0.0).sqrt();
                    Vec3::new(theta.cos() * r, z, theta.sin() * r) * rng.range_f(speed.0, speed.1)
                }
            };
            velocity += options.velocity;
            let life = rng.range_f(spec.life.0, spec.life.1) * options.life_scale;
            let color = options.color.unwrap_or_else(|| {
                if rng.next() < 0.5 {
                    spec.color
                } else {
                    spec.color2
                }
            });
            let alpha = options.alpha.unwrap_or(spec.alpha);
            let additive = spec.additive;
            let material = cache.material(style, color, alpha, additive, meshes, materials, images);
            self.commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(pos),
                Particle {
                    style,
                    velocity,
                    life,
                    max_life: life.max(0.001),
                    start_scale: spec.size.0 * options.size_scale,
                    end_scale: spec.size.1 * options.size_scale,
                    gravity: spec.gravity,
                    drag: spec.drag,
                    turbulence: spec.turbulence,
                    spin: Vec3::new(
                        rng.range_f(-1.0, 1.0),
                        rng.range_f(-1.0, 1.0),
                        rng.range_f(-1.0, 1.0),
                    )
                    .normalize_or_zero()
                        * rng.range_f(spec.spin.0, spec.spin.1),
                    color,
                    alpha,
                    additive,
                    billboard: spec.billboard,
                    ground: options.ground.unwrap_or(spec.ground),
                    bounce: spec.bounce,
                    fade_in: spec.fade_in,
                    bucket: u8::MAX,
                    seed: rng.range_f(0.0, 65535.0) as u32,
                },
                crate::InGame,
            ));
            cache.active += 1;
        }
        count
    }

    /// Simple directional burst (explosion, block break, impact).
    pub fn burst(&mut self, style: ParticleStyle, pos: Vec3, count: u32, size_scale: f32) -> u32 {
        self.emit(
            style,
            pos,
            EmitOptions::default().count(count).size_scale(size_scale),
        )
    }

    pub fn footstep(&mut self, pos: Vec3, velocity: Vec3, surface: FootSurface, sprint: bool) {
        let style = match surface {
            FootSurface::Grass => ParticleStyle::Dust,
            FootSurface::Sand => ParticleStyle::Sand,
            FootSurface::Snow => ParticleStyle::Snow,
            FootSurface::Stone => ParticleStyle::Dust,
            // Metal floors produce small chips rather than bright sparks: the
            // spark glow at the player's feet was visually noisy.
            FootSurface::Metal => ParticleStyle::Debris,
            FootSurface::Water => ParticleStyle::Splash,
            FootSurface::Lava => ParticleStyle::Ember,
            FootSurface::Alien => ParticleStyle::Spore,
            FootSurface::Wood => ParticleStyle::Leaf,
        };
        let (count, size) = match surface {
            FootSurface::Snow | FootSurface::Sand => (3, 1.0),
            FootSurface::Water => (4, 1.1),
            FootSurface::Lava => (3, 0.9),
            FootSurface::Metal => (2, 0.55),
            _ => (3, 0.9),
        };
        let count = if sprint { count + 1 } else { count };
        let horizontal = Vec3::new(velocity.x, 0.0, velocity.z);
        let dir = if horizontal.length_squared() > 0.01 {
            -horizontal.normalize()
        } else {
            Vec3::Y
        };
        let mut options = EmitOptions::default()
            .count(count)
            .dir(dir)
            .velocity(horizontal * 0.18)
            .size_scale(size)
            .life_scale(0.9);
        if surface == FootSurface::Metal {
            options = options.color(0.72, 0.75, 0.8);
        }
        self.emit(style, pos + Vec3::Y * 0.06, options);
    }

    pub fn landing(&mut self, pos: Vec3, impact: f32) {
        let impact = impact.clamp(0.0, 1.0);
        let count = (6.0 + impact * 18.0) as u32;
        self.emit(
            ParticleStyle::Dust,
            pos + Vec3::Y * 0.08,
            EmitOptions::default()
                .count(count)
                .spread(1.6)
                .speed(1.4 + impact * 3.4, 2.6 + impact * 5.2)
                .size_scale(1.0 + impact * 1.3)
                .life_scale(1.0 + impact * 0.6),
        );
        if impact > 0.35 {
            self.emit(
                ParticleStyle::Debris,
                pos + Vec3::Y * 0.1,
                EmitOptions::default()
                    .count((impact * 8.0) as u32)
                    .spread(1.4)
                    .speed(2.0, 5.0)
                    .size_scale(0.9),
            );
        }
    }

    pub fn jetpack(&mut self, pos: Vec3, velocity: Vec3, power: f32) {
        let count = if power > 0.7 { 3 } else { 2 };
        self.emit(
            ParticleStyle::Thruster,
            pos,
            EmitOptions::default()
                .count(count)
                .dir(-Vec3::Y)
                .velocity(velocity * 0.1)
                .speed(2.0, 4.5)
                .spread(0.35)
                .size_scale(0.7 + power * 0.5)
                .life_scale(0.7),
        );
        self.emit(
            ParticleStyle::Smoke,
            pos,
            EmitOptions::default()
                .count(1)
                .dir(-Vec3::Y)
                .speed(0.4, 1.2)
                .spread(0.8)
                .alpha(0.25)
                .size_scale(0.7),
        );
    }

    pub fn thruster(&mut self, pos: Vec3, back: Vec3, power: f32, warm: bool) {
        let style = if warm {
            ParticleStyle::Flame
        } else {
            ParticleStyle::Thruster
        };
        self.emit(
            style,
            pos,
            EmitOptions::default()
                .count(2 + (power * 3.0) as u32)
                .dir(back)
                .speed(6.0 + power * 10.0, 10.0 + power * 16.0)
                .spread(0.3)
                .size_scale(1.0 + power * 0.8),
        );
    }

    pub fn mining(&mut self, pos: Vec3, normal: Vec3, tint: (f32, f32, f32)) {
        // Physical chips only. The previous additive spark burst read as bright
        // white flecks flying around the screen while mining, which was tiring
        // to look at; block-coloured chips stay informative but calm.
        self.emit(
            ParticleStyle::Debris,
            pos,
            EmitOptions::default()
                .count(2)
                .dir(normal + Vec3::Y * 0.2)
                .speed(1.0, 2.6)
                .spread(1.1)
                .color(tint.0, tint.1, tint.2)
                .size_scale(0.6),
        );
    }

    pub fn machine_smoke(&mut self, pos: Vec3, intensity: f32) {
        self.emit(
            ParticleStyle::Smoke,
            pos,
            EmitOptions::default()
                .count(1)
                .dir(Vec3::Y)
                .speed(0.5, 1.4)
                .spread(0.6)
                .alpha(0.18 + intensity * 0.16)
                .size_scale(0.8 + intensity * 0.4),
        );
    }

    pub fn steam(&mut self, pos: Vec3, intensity: f32) {
        self.emit(
            ParticleStyle::Steam,
            pos,
            EmitOptions::default()
                .count(1 + (intensity * 2.0) as u32)
                .dir(Vec3::Y)
                .speed(0.8, 2.0)
                .spread(0.5)
                .size_scale(0.8 + intensity * 0.6),
        );
    }

    pub fn heal(&mut self, pos: Vec3, amount: f32) {
        self.emit(
            ParticleStyle::Heal,
            pos,
            EmitOptions::default()
                .count((3.0 + amount * 2.0) as u32)
                .speed(0.5, 1.8)
                .spread(1.4)
                .size_scale(0.8),
        );
    }

    pub fn shield_recharge(&mut self, pos: Vec3) {
        self.emit(
            ParticleStyle::Shield,
            pos,
            EmitOptions::default()
                .count(3)
                .speed(1.4, 3.2)
                .spread(1.6)
                .size_scale(0.7),
        );
    }

    pub fn hurt(&mut self, pos: Vec3, color: (f32, f32, f32)) {
        self.emit(
            ParticleStyle::Blood,
            pos,
            EmitOptions::default()
                .count(6)
                .speed(1.0, 3.2)
                .spread(1.2)
                .color(color.0, color.1, color.2)
                .size_scale(0.9),
        );
    }

    pub fn explosion(&mut self, pos: Vec3, scale: f32) {
        self.emit(
            ParticleStyle::Flame,
            pos,
            EmitOptions::default()
                .count((10.0 * scale).clamp(4.0, 40.0) as u32)
                .speed(2.0, 9.0 * scale)
                .spread(2.0)
                .size_scale(1.0 * scale)
                .life_scale(0.9 + scale * 0.4),
        );
        self.emit(
            ParticleStyle::Smoke,
            pos,
            EmitOptions::default()
                .count((6.0 * scale).clamp(3.0, 24.0) as u32)
                .speed(0.6, 3.0 * scale)
                .spread(2.0)
                .size_scale(1.0 * scale)
                .life_scale(1.0 + scale * 0.5),
        );
        self.emit(
            ParticleStyle::Spark,
            pos,
            EmitOptions::default()
                .count((10.0 * scale).clamp(4.0, 36.0) as u32)
                .speed(4.0, 12.0 * scale)
                .spread(2.0)
                .size_scale(0.9 * scale),
        );
    }

    pub fn splash(&mut self, pos: Vec3, scale: f32) {
        self.emit(
            ParticleStyle::Splash,
            pos,
            EmitOptions::default()
                .count((7.0 * scale).clamp(4.0, 26.0) as u32)
                .speed(1.4, 4.0 * scale)
                .spread(0.9)
                .size_scale(scale)
                .life_scale(1.0 + scale * 0.4),
        );
    }
}

/// Coarse surface classification used to pick footstep particles/audio.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FootSurface {
    Grass,
    Sand,
    Snow,
    Stone,
    Metal,
    Water,
    Lava,
    Alien,
    Wood,
}

impl FootSurface {
    /// Classify a voxel block key into a footstep family.
    pub fn from_block_key(key: &str) -> Self {
        match key {
            "sand" | "sandstone" | "salt" => Self::Sand,
            "snow" | "ice" => Self::Snow,
            "water" => Self::Water,
            "lava" => Self::Lava,
            "grass" | "red_moss" => Self::Grass,
            "log" | "planks" | "mush_stem" | "mush_cap" => Self::Wood,
            "metal" | "machine" | "belt" | "furnace" | "assembler" | "refinery" | "reactor"
            | "solar" | "wind" | "chest" | "beacon" => Self::Metal,
            "alien" | "hive" | "amber" | "crystal" | "glow_shroom" | "coral" => Self::Alien,
            _ => Self::Stone,
        }
    }

    pub fn pitch(self) -> f32 {
        match self {
            Self::Metal => 1.35,
            Self::Water => 0.85,
            Self::Sand => 0.78,
            Self::Snow => 0.7,
            Self::Wood => 1.12,
            Self::Alien => 1.25,
            Self::Lava => 0.6,
            _ => 1.0,
        }
    }

    pub fn step_volume(self) -> f32 {
        match self {
            Self::Metal => 0.22,
            Self::Water => 0.3,
            Self::Snow | Self::Sand => 0.2,
            _ => 0.26,
        }
    }
}

fn hash_position(pos: Vec3) -> u32 {
    let x = (pos.x * 8.0) as i32 as u32;
    let y = (pos.y * 8.0) as i32 as u32;
    let z = (pos.z * 8.0) as i32 as u32;
    x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ z.wrapping_mul(83_492_791)
}

/// Per-frame simulation: gravity, drag, turbulence, ground collision,
/// billboarding, spin, size/alpha over lifetime, despawn.
pub fn particle_system(
    time: Res<Time>,
    world: Option<Res<crate::world::World>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut cache: ResMut<ParticleCache>,
    mut commands: Commands,
    mut particles: Query<(
        Entity,
        &mut Particle,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let camera_rotation = camera.iter().next().map(|t| t.rotation());
    let elapsed = time.elapsed_secs();
    let mut active = 0usize;
    for (entity, mut particle, mut transform, mut material) in &mut particles {
        particle.life -= dt;
        if particle.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        active += 1;
        let t = 1.0 - clamp01(particle.life / particle.max_life);
        // Forces
        particle.velocity.y -= particle.gravity * dt;
        let drag = 1.0 - (particle.drag * dt).min(1.0);
        particle.velocity *= drag;
        if particle.turbulence > 0.0 {
            let seed = particle.seed as f32 * 0.001;
            let turbulence = Vec3::new(
                (elapsed * 2.3 + seed).sin(),
                (elapsed * 2.1 + seed * 1.7).sin(),
                (elapsed * 2.6 + seed * 2.3).sin(),
            );
            let strength = particle.turbulence;
            particle.velocity += turbulence * strength * dt;
        }
        let mut next = transform.translation + particle.velocity * dt;
        if particle.ground
            && let Some(world) = world.as_deref()
        {
            let floor = world.g.height_at(next.x.floor(), next.z.floor()) as f32 + 1.0;
            let scale = lerp(particle.start_scale, particle.end_scale, t).max(0.01);
            if next.y - scale * 0.5 < floor {
                next.y = floor + scale * 0.5;
                if particle.velocity.y < 0.0 {
                    particle.velocity.y = -particle.velocity.y * particle.bounce;
                }
                particle.velocity.x *= 0.72;
                particle.velocity.z *= 0.72;
            }
        }
        transform.translation = next;
        // Orientation
        if particle.billboard {
            if let Some(rotation) = camera_rotation {
                transform.rotation = rotation;
            }
        } else if particle.spin.length_squared() > 0.0 {
            let spin = particle.spin * dt;
            transform.rotate(Quat::from_rotation_x(spin.x));
            transform.rotate(Quat::from_rotation_y(spin.y));
            transform.rotate(Quat::from_rotation_z(spin.z));
        }
        // Size over lifetime
        let size = lerp(particle.start_scale, particle.end_scale, t).max(0.005);
        transform.scale = Vec3::splat(size);
        // Alpha over lifetime with fade-in support
        let fade_in = if particle.fade_in > 0.0 {
            clamp01(t / particle.fade_in)
        } else {
            1.0
        };
        let fade_out = 1.0 - clamp01((t - 0.55) / 0.45);
        let alpha = particle.alpha * ease_alpha(fade_in) * ease_alpha(fade_out);
        let bucket = ((alpha * (ALPHA_BUCKETS - 1) as f32).round() as u8).min(ALPHA_BUCKETS - 1);
        if bucket != particle.bucket {
            particle.bucket = bucket;
            let key = material_key(particle.style, particle.color, bucket, particle.additive);
            if let Some(handle) = cache.materials.get(&key) {
                material.0 = handle.clone();
            }
        }
    }
    cache.active = active;
}

#[inline]
fn ease_alpha(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}

/// Slow smooth drift used by particle wind so spawns look connected to weather.
pub fn wind_at(time: f32, seed: u32) -> Vec3 {
    let s = seed as f32 * 0.013;
    Vec3::new(
        (time * 0.23 + s).sin() * 0.6,
        (time * 0.11 + s * 1.7).sin() * 0.12,
        (time * 0.19 + s * 2.3).cos() * 0.6,
    )
}

/// Applies atmospheric wind drift to particles carrying the `turbulence` flag;
/// exposed for weather systems that need the same vector.
pub fn wind_vector(time: f32, weather_strength: f32) -> Vec3 {
    wind_at(time, 0x51A7) * (0.4 + weather_strength)
}

/// Particles plugin: asset cache + one simulation system.
pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleCache>().add_systems(
            Update,
            particle_system.run_if(in_state(crate::schedule::GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_has_sane_parameters() {
        let styles = [
            ParticleStyle::Dust,
            ParticleStyle::Sand,
            ParticleStyle::Snow,
            ParticleStyle::Leaf,
            ParticleStyle::Spore,
            ParticleStyle::Smoke,
            ParticleStyle::Steam,
            ParticleStyle::Toxic,
            ParticleStyle::Bubble,
            ParticleStyle::Splash,
            ParticleStyle::Spark,
            ParticleStyle::Ember,
            ParticleStyle::Flame,
            ParticleStyle::Thruster,
            ParticleStyle::Exhaust,
            ParticleStyle::Glow,
            ParticleStyle::Heal,
            ParticleStyle::Shield,
            ParticleStyle::Electric,
            ParticleStyle::Blood,
            ParticleStyle::Debris,
            ParticleStyle::Frost,
            ParticleStyle::Rainbow,
        ];
        for style in styles {
            let spec = style.spec();
            assert!(spec.life.0 > 0.0 && spec.life.1 >= spec.life.0, "{style:?}");
            assert!(spec.size.0 > 0.0 && spec.size.1 > 0.0, "{style:?}");
            assert!(
                spec.speed.0 >= 0.0 && spec.speed.1 >= spec.speed.0,
                "{style:?}"
            );
            assert!(spec.alpha > 0.0 && spec.alpha <= 1.0, "{style:?}");
            assert!((0.0..=2.0).contains(&spec.spread), "{style:?}");
            assert!(
                spec.brightness >= 1.0 && spec.brightness <= 4.0,
                "{style:?}"
            );
        }
    }

    #[test]
    fn material_keys_differ_by_alpha_and_color() {
        let style = ParticleStyle::Dust;
        let a = material_key(style, (0.5, 0.5, 0.5), 0, false);
        let b = material_key(style, (0.5, 0.5, 0.5), 4, false);
        let c = material_key(style, (0.9, 0.5, 0.5), 0, false);
        let d = material_key(style, (0.5, 0.5, 0.5), 0, true);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn color_quantization_is_stable() {
        assert_eq!(color_key(0.0, 0.0, 0.0), 0);
        assert_eq!(color_key(1.0, 1.0, 1.0), 0x7FFF);
        assert_eq!(color_key(0.52, 0.52, 0.52), color_key(0.53, 0.53, 0.53));
    }

    #[test]
    fn foot_surfaces_cover_common_blocks() {
        assert_eq!(FootSurface::from_block_key("grass"), FootSurface::Grass);
        assert_eq!(FootSurface::from_block_key("sand"), FootSurface::Sand);
        assert_eq!(FootSurface::from_block_key("snow"), FootSurface::Snow);
        assert_eq!(FootSurface::from_block_key("water"), FootSurface::Water);
        assert_eq!(FootSurface::from_block_key("metal"), FootSurface::Metal);
        assert_eq!(FootSurface::from_block_key("stone"), FootSurface::Stone);
        assert!(FootSurface::from_block_key("stone").step_volume() > 0.0);
    }

    #[test]
    fn soft_texture_center_is_opaque_and_corner_transparent() {
        let image = soft_texture();
        let data = image.data.as_ref().expect("texture data");
        let center = (16 * 32 + 16) * 4 + 3;
        assert!(data[center] > 240, "center alpha was {}", data[center]);
        assert_eq!(data[3], 0, "top-left corner must be transparent");
        assert!(data[data.len() - 1] < 8);
    }

    #[test]
    fn emit_options_builder_overrides() {
        let options = EmitOptions::default()
            .count(9)
            .speed(2.0, 3.0)
            .spread(0.5)
            .size_scale(2.0)
            .life_scale(0.5)
            .alpha(0.25)
            .color(0.1, 0.2, 0.3);
        assert_eq!(options.count, 9);
        assert_eq!(options.speed, (2.0, 3.0));
        assert_eq!(options.spread, 0.5);
        assert_eq!(options.size_scale, 2.0);
        assert_eq!(options.life_scale, 0.5);
        assert_eq!(options.alpha, Some(0.25));
        assert_eq!(options.color, Some((0.1, 0.2, 0.3)));
    }

    #[test]
    fn blend_particles_keep_their_authored_srgb_tone() {
        // Dust is authored as a mid-brown sRGB value. Read as linear it turned
        // near-white, which is why every chip and dust mote looked white.
        let dust = particle_base_color((0.58, 0.51, 0.42), 1.0, false);
        let linear = Color::Srgba(dust).to_linear();
        assert!(
            (linear.red - 0.296).abs() < 0.03,
            "dust red linear was {}",
            linear.red
        );
        assert!(linear.green < linear.red);
        // Additive styles stay linear and bright for bloom.
        let spark = particle_base_color((1.0, 0.8, 0.3), 2.4, true);
        let linear = Color::Srgba(spark).to_linear();
        assert!(linear.red > 2.0);
    }

    #[test]
    fn wind_is_bounded() {
        for i in 0..100 {
            let w = wind_vector(i as f32 * 0.1, 1.0);
            assert!(w.length() < 3.0);
        }
    }
}

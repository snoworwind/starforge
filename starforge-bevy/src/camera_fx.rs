//! Camera feel: trauma-based screen shake, first-person head bob, landing dip,
//! sprint FOV, strafe roll and damage kicks.
//!
//! The module never writes the camera on its own during the movement phase:
//! [`ground_feel_system`] runs just before the player camera and updates the
//! shared [`CameraFeel`] resource, then [`camera_shake_system`] runs after every
//! other camera writer (ground, seated, flight, warp) and layers the shake on
//! top. That ordering is what keeps the effect identical in every scene.

use bevy::prelude::*;

use crate::player::{Player, PlayerCameraMode};
use crate::schedule::{GameSet, in_planet_mode};
use crate::space::FlightMode;
use crate::tween::{ShakeNoise, Spring, exp_approach};

/// How strongly each source feeds the shake bus. Tuned so the sum of a normal
/// firefight stays under 1.0.
pub mod shake {
    pub const LANDING: f32 = 0.35;
    pub const HURT: f32 = 0.42;
    pub const EXPLOSION: f32 = 0.55;
    pub const WEAPON_FIRE: f32 = 0.12;
    pub const SHIP_HIT: f32 = 0.5;
    pub const WARP: f32 = 0.7;
    pub const THUNDER: f32 = 0.45;
    pub const METEOR: f32 = 0.6;
    pub const DRILL: f32 = 0.06;
}

#[derive(Resource)]
pub struct CameraFeel {
    /// Distance travelled since the last step, drives the bob phase.
    pub travel: f32,
    pub bob_phase: f32,
    pub bob_amp: f32,
    pub bob_speed: f32,
    /// Vertical landing dip (negative compresses, positive rebounds).
    pub dip: Spring,
    /// Third-person shoulder offset driven by strafing.
    pub lateral: Spring,
    /// Extra FOV in degrees applied on top of the camera's base FOV.
    pub fov: f32,
    pub fov_target: f32,
    /// One-shot FOV impulse from abilities (dash, burst); decays quickly.
    pub ability_fov: f32,
    /// Slight roll from lateral velocity (ground) — adds life to strafing.
    pub roll: Spring,
    pub trauma: f32,
    pub shake_time: f32,
    pub shake_noise: ShakeNoise,
    pub was_grounded: bool,
    pub last_vel_y: f32,
    pub last_hp: f32,
    pub last_shield: f32,
    /// Time spent underwater, used by the screen effects.
    pub submerged_t: f32,
    pub hurt_flash: f32,
    /// Exposed for tests/debug: last impulse magnitude applied to the dip.
    pub last_landing: f32,
}

impl Default for CameraFeel {
    fn default() -> Self {
        Self {
            travel: 0.0,
            bob_phase: 0.0,
            bob_amp: 0.0,
            bob_speed: 0.0,
            dip: Spring::new(0.0, 110.0, 0.72),
            lateral: Spring::new(0.0, 70.0, 0.9),
            fov: 0.0,
            fov_target: 0.0,
            ability_fov: 0.0,
            roll: Spring::new(0.0, 60.0, 0.85),
            trauma: 0.0,
            shake_time: 0.0,
            shake_noise: ShakeNoise::new(0x5EED_5AFE),
            was_grounded: false,
            last_vel_y: 0.0,
            last_hp: 8.0,
            last_shield: 6.0,
            submerged_t: 0.0,
            hurt_flash: 0.0,
            last_landing: 0.0,
        }
    }
}

impl CameraFeel {
    /// Queue a shake. `amount` is in trauma units; values stack but cap at 1.
    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount.max(0.0)).min(1.0);
    }

    /// Current shake amplitude (squared falloff feels punchier than linear).
    pub fn amplitude(&self) -> f32 {
        (self.trauma * self.trauma).clamp(0.0, 1.0)
    }

    /// Noise-sampled offset for the current frame. Uses wall-clock time so the
    /// shake does not slow down when the game pauses input.
    pub fn shake_offset(&self) -> Vec3 {
        let t = self.shake_time;
        let magnitude = self.amplitude();
        if magnitude < 1e-4 {
            return Vec3::ZERO;
        }
        let base = self.shake_noise.sample(t * 1.7);
        // A second, faster layer gives the shake a sharper attack.
        let fast = self.shake_noise.sample(t * 4.3 + 13.0) * 0.35;
        (base + fast) * magnitude
    }

    /// Rotational shake in radians.
    pub fn shake_rotation(&self) -> Vec3 {
        let magnitude = self.amplitude();
        if magnitude < 1e-4 {
            return Vec3::ZERO;
        }
        self.shake_noise.sample(self.shake_time * 2.1 + 79.0) * 0.03 * magnitude
    }

    pub fn reset(&mut self) {
        let noise = self.shake_noise;
        *self = Self {
            shake_noise: noise,
            ..default()
        };
    }
}

/// Damage/landing bookkeeping for the ground player, run right before the
/// camera is assembled. Also emits landing dust and footsteps via particles.
#[allow(clippy::too_many_arguments)]
pub fn ground_feel_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut feel: ResMut<CameraFeel>,
    player: Query<&Player>,
    mut fx: crate::particles::ParticleSystem,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let Ok(p) = player.single() else {
        return;
    };
    feel.shake_time += dt;
    feel.trauma = (feel.trauma - 0.9 * dt).max(0.0);
    feel.hurt_flash = (feel.hurt_flash - 2.2 * dt).max(0.0);

    let horizontal = Vec2::new(p.vel.x, p.vel.z);
    let speed = horizontal.length();
    let sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Distance-driven head bob: the phase advances with actual travel so the
    // bob never runs while standing still, even on a laggy frame.
    let moved = speed * dt;
    feel.travel += moved;
    if p.on_ground && !p.in_liquid {
        let stride_rate = if sprint { 0.115 } else { 0.16 };
        feel.bob_phase = (feel.bob_phase + moved * std::f32::consts::TAU * stride_rate)
            .rem_euclid(std::f32::consts::TAU);
    } else {
        // Ease the phase back to neutral in the air so landings start clean.
        feel.bob_phase = exp_approach(feel.bob_phase % std::f32::consts::TAU, 0.0, 3.0, dt);
    }
    let speed_factor = (speed / 7.2).clamp(0.0, 1.2);
    let target_amp = if p.on_ground && !p.dead && speed > 0.35 {
        (0.028 + speed_factor * 0.035) * (if p.in_liquid { 0.35 } else { 1.0 })
    } else {
        0.0
    };
    feel.bob_amp = exp_approach(feel.bob_amp, target_amp, 7.0, dt);
    feel.bob_speed = exp_approach(feel.bob_speed, speed, 6.0, dt);

    // Landing: impulse the dip spring and kick up dust.
    if p.on_ground && !feel.was_grounded {
        let fall_speed = (-feel.last_vel_y).max(0.0);
        if fall_speed > 3.0 {
            let impact = ((fall_speed - 3.0) / 22.0).clamp(0.0, 1.0);
            feel.dip.impulse(-(0.35 + impact * 1.5));
            feel.add_trauma(shake::LANDING * (0.35 + impact));
            feel.last_landing = impact;
            if impact > 0.12 && !p.in_liquid {
                fx.landing(p.pos, impact);
            }
        }
    }
    feel.was_grounded = p.on_ground;
    feel.last_vel_y = p.vel.y;

    // Spin strafing into a subtle roll: project velocity on the camera right.
    let right = Vec3::new(p.forward().z, 0.0, -p.forward().x);
    let lateral_speed = p.vel.dot(right);
    let target_roll = (-lateral_speed * 0.0075).clamp(-0.035, 0.035);
    feel.roll.update(target_roll, dt);
    feel.lateral.update(lateral_speed * 0.012, dt);

    // Sprint FOV kick, comfortably subtle at 75° base.
    let moving_forward = keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::KeyS);
    feel.ability_fov = exp_approach(feel.ability_fov, 0.0, 6.0, dt);
    feel.fov_target = if sprint && moving_forward && speed > 3.0 {
        4.5
    } else if p.in_liquid {
        -2.0
    } else {
        0.0
    } + feel.ability_fov;
    feel.fov = exp_approach(feel.fov, feel.fov_target, 5.0, dt);

    // Damage kick: watch for hp/shield drops even if damage came from a
    // source that does not know about the camera.
    let max_shield = p.stat_max("shield");
    if p.stats.hp < feel.last_hp - 0.01 || p.stats.shield < feel.last_shield - 0.01 {
        feel.add_trauma(shake::HURT);
        feel.hurt_flash = 1.0;
    }
    feel.last_hp = p.stats.hp;
    feel.last_shield = p.stats.shield.min(max_shield);

    // Jetpack rumble while thrusting.
    if keys.pressed(KeyCode::Space) && !p.on_ground && p.stats.jet > 0.0 {
        feel.add_trauma(0.018);
    }
    feel.submerged_t = if p.in_liquid {
        feel.submerged_t + dt
    } else {
        0.0
    };
}

/// Applies the feel offsets and shake to whichever camera is active.
/// Runs after every camera writer in the frame.
pub fn camera_shake_system(
    feel: Res<CameraFeel>,
    flight: Res<FlightMode>,
    mode: Res<PlayerCameraMode>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut tf) = cam.single_mut() else {
        return;
    };
    let rotation = tf.rotation;
    match *flight {
        FlightMode::Planet | FlightMode::Seated | FlightMode::Station => {
            let up = rotation * Vec3::Y;
            let right = rotation * Vec3::X;
            let bob = if mode.third_person { 0.45 } else { 1.0 };
            let lateral = (feel.bob_phase).sin() * feel.bob_amp * 0.65 * bob;
            let vertical = (feel.bob_phase * 2.0).sin() * feel.bob_amp * 0.42 * bob;
            tf.translation += right * lateral + up * (vertical + feel.dip.value);
            let roll = feel.roll.value * if mode.third_person { 1.4 } else { 1.0 };
            tf.rotation = rotation * Quat::from_rotation_z(roll);
        }
        _ => {}
    }
    if feel.trauma > 0.0 {
        let offset = feel.shake_offset();
        let rotational = feel.shake_rotation();
        tf.translation += rotation * (offset * 0.09);
        tf.rotation *=
            Quat::from_euler(EulerRot::YXZ, rotational.y, rotational.x, rotational.z).normalize();
    }
}

/// Resets the feel state whenever a new world becomes playable.
pub fn reset_camera_feel(mut feel: ResMut<CameraFeel>) {
    feel.reset();
}

pub struct CameraFxPlugin;

impl Plugin for CameraFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFeel>()
            .add_systems(
                Update,
                ground_feel_system
                    .in_set(GameSet::LateLook)
                    .before(crate::player::camera_system)
                    .run_if(in_state(crate::schedule::GameState::Playing))
                    .run_if(in_planet_mode),
            )
            .add_systems(
                Update,
                camera_shake_system
                    .in_set(GameSet::CameraFx)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                OnEnter(crate::schedule::GameState::Playing),
                reset_camera_feel,
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trauma_accumulates_and_caps() {
        let mut feel = CameraFeel::default();
        feel.add_trauma(0.4);
        feel.add_trauma(0.4);
        feel.add_trauma(0.9);
        assert_eq!(feel.trauma, 1.0);
        assert!(feel.amplitude() <= 1.0);
    }

    #[test]
    fn shake_offset_is_zero_without_trauma() {
        let feel = CameraFeel::default();
        assert_eq!(feel.shake_offset(), Vec3::ZERO);
        assert_eq!(feel.shake_rotation(), Vec3::ZERO);
    }

    #[test]
    fn shake_offset_is_bounded() {
        let mut feel = CameraFeel::default();
        feel.add_trauma(1.0);
        feel.shake_time = 12.34;
        let offset = feel.shake_offset();
        assert!(offset.x.abs() <= 1.5 && offset.y.abs() <= 1.5 && offset.z.abs() <= 1.5);
    }

    #[test]
    fn dip_spring_returns_to_rest() {
        let mut feel = CameraFeel::default();
        feel.dip.impulse(-1.2);
        for _ in 0..600 {
            feel.dip.update(0.0, 1.0 / 60.0);
        }
        assert!(feel.dip.value.abs() < 1e-3);
    }

    #[test]
    fn reset_clears_transient_state() {
        let mut feel = CameraFeel::default();
        feel.add_trauma(0.8);
        feel.fov = 4.0;
        feel.dip.impulse(-2.0);
        feel.reset();
        assert_eq!(feel.trauma, 0.0);
        assert_eq!(feel.fov, 0.0);
        assert_eq!(feel.dip.velocity, 0.0);
    }
}

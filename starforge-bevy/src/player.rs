//! Player controller — port of js/player.js (movement, jetpack, liquid, mining laser,
//! placement, hotbar, drops, survival stats).

use crate::audio;
use crate::creatures::{Creature, spawn_drop};
use crate::data::{self, CHARGE_DEFS, Difficulty, DropEntry, ids};
use crate::inventory::{Inventory, Slot};
use crate::schedule::{GameSet, GameState, ground_mode, in_planet_mode, walk_look_mode};
use crate::ui::{Ghost, UiState};
use crate::world::World;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

pub const W: f32 = 0.3;
pub const H: f32 = 1.8;
pub const EYE: f32 = 1.62;
pub const JETPACK_CEILING: f32 = 118.0;

#[derive(Clone, Debug)]
pub struct Stats {
    pub hp: f32,
    pub shield: f32,
    pub o2: f32,
    pub haz: f32,
    pub jet: f32,
    pub laser: f32,
}

impl Stats {
    pub fn full() -> Self {
        Self {
            hp: 8.0,
            shield: 6.0,
            o2: 100.0,
            haz: 100.0,
            jet: 100.0,
            laser: 100.0,
        }
    }
    pub fn get(&self, name: &str) -> f32 {
        match name {
            "hp" => self.hp,
            "shield" => self.shield,
            "o2" => self.o2,
            "haz" => self.haz,
            "jet" => self.jet,
            _ => self.laser,
        }
    }
    pub fn get_mut(&mut self, name: &str) -> &mut f32 {
        match name {
            "hp" => &mut self.hp,
            "shield" => &mut self.shield,
            "o2" => &mut self.o2,
            "haz" => &mut self.haz,
            "jet" => &mut self.jet,
            _ => &mut self.laser,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mining {
    pub target: [i32; 3],
    pub prog: f32,
    pub dig_sound_t: f32,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Equipment {
    pub suit: Option<String>,
    pub life_support: Option<String>,
    pub tool: Option<String>,
    pub defense: Option<String>,
}

impl Equipment {
    fn slot_mut(&mut self, slot: &str) -> Option<&mut Option<String>> {
        match slot {
            "suit" => Some(&mut self.suit),
            "life_support" => Some(&mut self.life_support),
            "tool" => Some(&mut self.tool),
            "defense" => Some(&mut self.defense),
            _ => None,
        }
    }

    pub fn equipped(&self) -> impl Iterator<Item = &String> {
        [
            self.suit.as_ref(),
            self.life_support.as_ref(),
            self.tool.as_ref(),
            self.defense.as_ref(),
        ]
        .into_iter()
        .flatten()
    }

    pub fn bonus(&self, effect: &str) -> f32 {
        self.equipped()
            .filter_map(|key| data::item_by_key(key))
            .filter_map(|item| item.equipment)
            .filter(|bonus| bonus.effect == effect)
            .map(|bonus| bonus.amount)
            .sum()
    }

    pub fn equip(&mut self, item: &str) -> Result<Option<String>, &'static str> {
        let bonus = data::item_by_key(item)
            .and_then(|item| item.equipment)
            .ok_or("该物品不是装备")?;
        let slot = self.slot_mut(bonus.slot).ok_or("未知装备槽")?;
        Ok(slot.replace(item.to_string()))
    }

    pub fn take_slot(&mut self, slot_name: &str) -> Option<String> {
        self.slot_mut(slot_name).and_then(Option::take)
    }

    pub fn sanitize(&mut self) {
        for (slot_name, equipped) in [
            ("suit", &mut self.suit),
            ("life_support", &mut self.life_support),
            ("tool", &mut self.tool),
            ("defense", &mut self.defense),
        ] {
            let valid = equipped
                .as_deref()
                .and_then(data::item_by_key)
                .and_then(|item| item.equipment)
                .is_some_and(|bonus| bonus.slot == slot_name);
            if !valid {
                *equipped = None;
            }
        }
    }
}

#[derive(Component)]
pub struct Player {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub was_ground: bool,
    pub in_liquid: bool,
    pub stats: Stats,
    pub inv: Inventory,
    pub equipment: Equipment,
    pub hot_idx: i32, // -1 = mining laser
    pub mining: Option<Mining>,
    pub dmg_acc: f32,
    pub dead: bool,
    pub respawn_timer: f32,
    pub difficulty: Difficulty,
    pub place_dir: Option<u8>,
    pub credits: i32,
    pub jet_entity: Option<Entity>,
    pub toasts: Vec<(String, f32)>,
    pub play_time: f32,
    pub appearance: crate::save::Appearance,
    /// 危险低值警报计时（JS hazBeepT）
    pub haz_beep_t: f32,
    /// 「需要采矿激光」提示计时（JS noLaserHintT）
    pub no_laser_t: f32,
    pub step_t: f32,
}

impl Player {
    pub fn new(difficulty: Difficulty) -> Self {
        Self {
            pos: Vec3::new(96.5, 42.0, 96.5),
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            was_ground: false,
            in_liquid: false,
            stats: Stats::full(),
            inv: Inventory::default(),
            equipment: Equipment::default(),
            hot_idx: -1,
            mining: None,
            dmg_acc: 0.0,
            dead: false,
            respawn_timer: 0.0,
            difficulty,
            place_dir: None,
            credits: 0,
            jet_entity: None,
            toasts: Vec::new(),
            play_time: 0.0,
            appearance: crate::save::Appearance::default(),
            haz_beep_t: 0.0,
            no_laser_t: 0.0,
            step_t: 0.0,
        }
    }

    pub fn creative(&self) -> bool {
        self.difficulty == Difficulty::Creative
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    pub fn look_dir(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::Y * EYE
    }

    /// Auto placement direction from yaw: 0=E(+X),1=S(+Z),2=W(-X),3=N(-Z).
    pub fn auto_dir(&self) -> u8 {
        let fx = -self.yaw.sin();
        let fz = -self.yaw.cos();
        if fx.abs() > fz.abs() {
            if fx > 0.0 { 0 } else { 2 }
        } else if fz > 0.0 {
            1
        } else {
            3
        }
    }

    pub fn effective_dir(&self) -> u8 {
        self.place_dir.unwrap_or_else(|| self.auto_dir())
    }

    pub fn hot_slot(&self) -> Option<usize> {
        if self.hot_idx < 0 {
            None
        } else {
            let idx = self.hot_idx as usize;
            (idx < self.inv.slots.len()).then_some(idx)
        }
    }

    pub fn selected_item(&self) -> Option<&Slot> {
        self.hot_slot().and_then(|i| self.inv.slots[i].as_ref())
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push((text.into(), 3.0));
    }

    /// Apply damage: shield first, then hp. Returns true if the player died.
    pub fn damage(&mut self, n: f32) -> bool {
        if self.dead || !n.is_finite() || n <= 0.0 || self.creative() {
            return false;
        }
        let mut rem = n;
        let shield_take = rem.min(self.stats.shield);
        self.stats.shield -= shield_take;
        rem -= shield_take;
        self.stats.hp -= rem;
        if self.stats.hp <= 0.0 {
            self.stats.hp = 0.0;
            self.dead = true;
            self.respawn_timer = 1.8;
            return true;
        }
        false
    }

    /// Charge a system from inventory items (CHARGE_DEFS).
    pub fn can_charge(&self, system: &str) -> bool {
        let Some(def) = CHARGE_DEFS.iter().find(|d| d.0 == system) else {
            return false;
        };
        let max = self.stat_max(system);
        self.stats.get(system) < max - 0.01 && self.inv.count_item(def.1) >= def.2
    }

    pub fn charge(&mut self, system: &str) -> bool {
        let Some(def) = CHARGE_DEFS.iter().find(|d| d.0 == system) else {
            return false;
        };
        if !self.can_charge(system) {
            return false;
        }
        self.inv.remove_item(def.1, def.2);
        let max = self.stat_max(system);
        *self.stats.get_mut(system) = (self.stats.get(system) + def.3).min(max);
        true
    }

    pub fn stat_max(&self, system: &str) -> f32 {
        match system {
            "hp" => 8.0,
            "shield" => 6.0 + self.equipment.bonus("shield_capacity"),
            "o2" => 100.0 + self.equipment.bonus("o2_capacity"),
            _ => 100.0,
        }
    }

    pub fn hazard_resistance(&self, hazard: &str) -> f32 {
        self.equipment
            .bonus(&format!("{hazard}_resist"))
            .clamp(0.0, 0.9)
    }
}

// ---------- Input / look ----------

pub fn look_system(
    mouse: Res<AccumulatedMouseMotion>,
    mut q: Query<&mut Player>,
    ui: Res<UiState>,
) {
    if ui.locked() {
        return;
    }
    let delta = mouse.delta;
    if delta.length_squared() < 1e-6 {
        return;
    }
    if delta.x.abs() > 200.0 || delta.y.abs() > 200.0 {
        return; // spike discard
    }
    for mut p in &mut q {
        let s = 0.0024;
        p.yaw -= delta.x * s;
        p.pitch -= delta.y * s;
        // 允许看到正下方：旧限制 1.35 软 / 1.55 硬 ≈ 88.8°，永远到不了
        // 垂直（π/2 ≈ 1.5708）。保持软区手感，硬限放到 90°。
        let soft = 1.40f32;
        let hard = std::f32::consts::FRAC_PI_2;
        p.pitch = if p.pitch > soft {
            soft + (hard - soft) * (1.0 - (-(p.pitch - soft) * 3.0).exp())
        } else if p.pitch < -soft {
            -soft - (hard - soft) * (1.0 - (-(-p.pitch - soft) * 3.0).exp())
        } else {
            p.pitch
        };
        p.pitch = p.pitch.clamp(-hard, hard);
    }
}

// ---------- Movement / physics ----------

pub fn movement_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut q: Query<&mut Player>,
    ui: Res<UiState>,
    world: Res<World>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    let dt = time.delta_secs();
    for mut p in &mut q {
        if p.dead || ui.locked() {
            // 面板/死亡时停喷气音
            if let Some(e) = p.jet_entity.take() {
                commands.entity(e).despawn();
            }
            continue;
        }
        let sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let speed = if sprint { 7.2 } else { 4.5 };
        let f = p.forward();
        let r = Vec3::new(-f.z, 0.0, f.x);
        let mut wish = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) {
            wish += f;
        }
        if keys.pressed(KeyCode::KeyS) {
            wish -= f;
        }
        if keys.pressed(KeyCode::KeyD) {
            wish += r;
        }
        if keys.pressed(KeyCode::KeyA) {
            wish -= r;
        }
        if wish.length_squared() > 0.0 {
            wish = wish.normalize() * speed;
        }
        let accel = if p.on_ground { 12.0 } else { 5.0 };
        let k = (accel * dt).min(1.0);
        p.vel.x += (wish.x - p.vel.x) * k;
        p.vel.z += (wish.z - p.vel.z) * k;

        // liquid test (previous frame pos)
        let feet = data::block_by_id(world.get(
            p.pos.x.floor() as i32,
            (p.pos.y + 0.1).floor() as i32,
            p.pos.z.floor() as i32,
        ));
        let eye = data::block_by_id(world.get(
            p.pos.x.floor() as i32,
            (p.pos.y + EYE).floor() as i32,
            p.pos.z.floor() as i32,
        ));
        p.in_liquid = feet.liquid || eye.liquid;

        if p.in_liquid {
            p.vel.x *= (1.0 - 5.0 * dt).max(0.0);
            p.vel.z *= (1.0 - 5.0 * dt).max(0.0);
            p.vel.y += (0.0 - p.vel.y) * (3.2 * dt).min(1.0);
            if keys.pressed(KeyCode::Space) {
                p.vel.y = (p.vel.y + 18.0 * dt).min(5.5);
            } else if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
                p.vel.y = (p.vel.y - 18.0 * dt).max(-5.5);
            }
            p.on_ground = false;
        } else {
            p.vel.y -= 22.0 * dt;
            p.vel.y = p.vel.y.max(-40.0);
            let space = keys.pressed(KeyCode::Space);
            if space && p.on_ground {
                p.vel.y = 7.4;
                p.on_ground = false;
                audio::play(&mut commands, sfx.jump.clone(), 0.5, None);
            } else if space && !p.on_ground && p.stats.jet > 0.0 && p.pos.y < JETPACK_CEILING {
                p.vel.y = (p.vel.y + 33.0 * dt).min(8.5);
                p.stats.jet = (p.stats.jet - 28.0 * dt).max(0.0);
            } else if p.pos.y >= JETPACK_CEILING {
                p.vel.y = p.vel.y.min(0.0);
            }
        }
        // 喷气背包循环音（JS Sound.loops.jet 启停）
        let jetting = keys.pressed(KeyCode::Space) && !p.on_ground && p.stats.jet > 0.0;
        if jetting && p.jet_entity.is_none() {
            let e = audio::play_jet(&mut commands, sfx.jet.clone(), 0.35);
            p.jet_entity = Some(e);
        } else if !jetting && let Some(e) = p.jet_entity.take() {
            commands.entity(e).despawn();
        }
        if p.on_ground {
            p.stats.jet = (p.stats.jet + 40.0 * dt).min(100.0);
        }
        if p.creative() {
            p.stats.jet = 100.0;
        }
        let horizontal_speed = Vec2::new(p.vel.x, p.vel.z).length();
        if p.on_ground && horizontal_speed > 1.0 {
            p.step_t += dt;
            if p.step_t >= if sprint { 0.28 } else { 0.38 } {
                p.step_t = 0.0;
                audio::play(
                    &mut commands,
                    sfx.step.clone(),
                    0.28,
                    Some(0.92 + (p.pos.x.abs() % 0.12)),
                );
            }
        } else {
            p.step_t = 0.0;
        }
        // toasts decay
        for t in p.toasts.iter_mut() {
            t.1 -= dt;
        }
        p.toasts.retain(|t| t.1 > 0.0);
    }
}

pub fn collision_system(time: Res<Time>, mut q: Query<&mut Player>, world: Res<World>) {
    // A long frame must not turn one collision step into a several-hundred
    // metre teleport. This also bounds the recovery loop below.
    let dt = time.delta_secs().clamp(0.0, 0.1);
    for mut p in &mut q {
        if p.dead {
            continue;
        }
        let collides = |px: f32, py: f32, pz: f32| -> bool {
            let x0 = (px - W).floor() as i32;
            let x1 = (px + W).floor() as i32;
            let y0 = py.floor() as i32;
            let y1 = (py + H).floor() as i32;
            let z0 = (pz - W).floor() as i32;
            let z1 = (pz + W).floor() as i32;
            for x in x0..=x1 {
                for y in y0..=y1 {
                    for z in z0..=z1 {
                        let def = data::block_by_id(world.get(x, y, z));
                        if !def.solid {
                            continue;
                        }
                        if let Some(lb) = def.lowbox
                            && py > y as f32 + lb
                        {
                            continue; // step over
                        }
                        return true;
                    }
                }
            }
            false
        };
        let mut np = p.pos;
        np.x += p.vel.x * dt;
        if collides(np.x, p.pos.y, p.pos.z) {
            np.x = p.pos.x;
            p.vel.x = 0.0;
        }
        np.z += p.vel.z * dt;
        if collides(np.x, p.pos.y, np.z) {
            np.z = p.pos.z;
            p.vel.z = 0.0;
        }
        np.y = p.pos.y + p.vel.y * dt;
        p.was_ground = p.on_ground;
        p.on_ground = false;
        if collides(np.x, np.y, np.z) {
            if p.vel.y < 0.0 {
                p.on_ground = true;
                if p.vel.y < -12.0 {
                    let dmg = ((-p.vel.y - 12.0) / 4.0).floor();
                    p.damage(dmg);
                }
            }
            np.y = p.pos.y;
            p.vel.y = 0.0;
            for _ in 0..2048 {
                if !collides(np.x, np.y, np.z) {
                    break;
                }
                np.y += 0.05;
            }
        }
        p.pos = np;
        if p.pos.y > JETPACK_CEILING {
            p.pos.y = JETPACK_CEILING;
            p.vel.y = p.vel.y.min(0.0);
        }
        if p.pos.y < -10.0 {
            p.pos.y = 80.0;
            p.damage(2.0);
        }
    }
}

/// Survival stats: oxygen/hazard drain, regen, lava burn, death respawn.
pub fn survival_system(
    time: Res<Time>,
    mut q: Query<&mut Player>,
    world: Res<World>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
    ui: Res<crate::ui::UiState>,
) {
    let dt = time.delta_secs();
    for mut p in &mut q {
        if p.dead {
            p.respawn_timer -= dt;
            if p.respawn_timer <= 0.0 {
                let spawn = world.find_spawn(96, 96);
                p.pos = spawn;
                p.vel = Vec3::ZERO;
                p.stats = Stats::full();
                let max_o2 = p.stat_max("o2");
                let max_shield = p.stat_max("shield");
                p.stats.o2 = max_o2;
                p.stats.shield = max_shield;
                p.dead = false;
                p.toast("外骨骼已在重生点重建");
            }
            continue;
        }
        // 面板打开时冻结生存消耗（JS: Player.update(dt*0)）
        if ui.locked() {
            continue;
        }
        let biome = world.biome();
        // O₂ 无条件消耗（JS player.js:700）——与是否在水中无关
        if !p.creative() {
            let life_support_mul = if p.equipment.bonus("o2_capacity") > 0.0 {
                0.75
            } else {
                1.0
            };
            p.stats.o2 = (p.stats.o2 - 0.35 * life_support_mul * dt).max(0.0);
            if p.stats.o2 <= 0.0 {
                p.dmg_acc += dt * 0.5;
            }
            // 危险低值警报（JS: haz<25 每 3s 提示）
            if p.stats.haz < 25.0 {
                p.haz_beep_t += dt;
                if p.haz_beep_t > 3.0 {
                    p.haz_beep_t = 0.0;
                    audio::play(&mut commands, sfx.error.clone(), 0.35, None);
                    p.toast("⚠ 危险防护不足");
                }
            } else {
                p.haz_beep_t = 0.0;
            }
        }
        if p.creative() {
            let max_o2 = p.stat_max("o2");
            let max_shield = p.stat_max("shield");
            p.stats.haz = 100.0;
            p.stats.o2 = max_o2;
            p.stats.shield = max_shield;
            p.stats.hp = 8.0;
            p.stats.laser = 100.0;
        } else if let Some(hazard) = biome.haz {
            let exposure = 1.0 - p.hazard_resistance(hazard);
            p.stats.haz = (p.stats.haz - biome.haz_rate * exposure * dt).max(0.0);
            // Hazards change how the player plans a trip instead of merely
            // presenting five labels for the same meter drain.
            match hazard {
                "heat" if p.stats.haz < 35.0 => {
                    p.stats.jet = (p.stats.jet - 0.35 * exposure * dt).max(0.0);
                }
                "cold" => {
                    p.stats.jet = (p.stats.jet - 0.18 * exposure * dt).max(0.0);
                }
                "toxic" => {
                    p.stats.o2 = (p.stats.o2 - 0.16 * exposure * dt).max(0.0);
                }
                "rad" if p.stats.haz < 20.0 => {
                    p.dmg_acc += 0.35 * exposure * dt;
                }
                "storm" => {
                    p.stats.shield = (p.stats.shield - 0.12 * exposure * dt).max(0.0);
                }
                _ => {}
            }
        } else {
            p.stats.haz = (p.stats.haz + 2.0 * dt).min(100.0);
        }
        if p.stats.o2 > 20.0 && p.stats.haz > 10.0 {
            let storm_blocks_regen = biome.haz == Some("storm");
            if !storm_blocks_regen {
                let max_shield = p.stat_max("shield");
                p.stats.shield = (p.stats.shield + 0.15 * dt).min(max_shield);
            }
        }
        if p.stats.haz <= 0.0 && biome.haz.is_some() {
            p.dmg_acc += dt * 0.4;
        }
        if p.in_liquid && biome.lava {
            p.dmg_acc += dt * 3.0;
        }
        if p.dmg_acc >= 1.0 {
            let whole = p.dmg_acc.floor();
            p.dmg_acc -= whole;
            let died = p.damage(whole);
            if died {
                audio::play(&mut commands, sfx.alarm.clone(), 0.8, None);
            } else {
                audio::play(&mut commands, sfx.hurt.clone(), 0.7, None);
            }
        }
    }
}

// ---------- Camera ----------

pub fn camera_system(
    player: Query<&Player>,
    mode: Res<PlayerCameraMode>,
    mut cam: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<Player>)>,
) {
    let Ok(p) = player.single() else { return };
    for (mut tf, mut proj) in &mut cam {
        let yaw = Quat::from_rotation_y(p.yaw);
        let pitch = Quat::from_rotation_x(p.pitch);
        if mode.third_person {
            let look = p.eye();
            tf.translation = look - p.forward() * 6.0 + Vec3::Y * 2.2;
            tf.look_at(look, Vec3::Y);
        } else {
            tf.translation = p.eye();
            tf.rotation = yaw * pitch;
        }
        *proj = Projection::Perspective(PerspectiveProjection {
            fov: 75f32.to_radians(),
            far: crate::space::CAM_FAR,
            ..default()
        });
    }
}

#[derive(Resource, Default)]
pub struct PlayerCameraMode {
    pub third_person: bool,
}

pub fn camera_toggle_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<PlayerCameraMode>,
    mut player: Query<&mut Player>,
) {
    if !keys.just_pressed(KeyCode::KeyV) {
        return;
    }
    mode.third_person = !mode.third_person;
    if let Ok(mut p) = player.single_mut() {
        p.toast(if mode.third_person {
            "第三人称镜头"
        } else {
            "第一人称镜头"
        });
    }
}

// ---------- Mining laser ----------

/// Block breaks queued from the mining system (world mutation must happen in a dedicated system).
#[derive(Resource, Default)]
pub struct BreakQueue(pub Vec<(Entity, [i32; 3], Vec<DropEntry>, f32)>);

#[allow(clippy::too_many_arguments)]
pub fn mining_system(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Player)>,
    mut creatures: ParamSet<(Query<(Entity, &Creature, &Transform)>, Query<&mut Creature>)>,
    world: Res<World>,
    sfx: Res<audio::Sfx>,
    ui: Res<UiState>,
    mut queue: ResMut<BreakQueue>,
) {
    let dt = time.delta_secs();
    for (pe, mut p) in &mut q {
        if p.dead || ui.locked() {
            p.mining = None;
            continue;
        }
        let laser_selected = p.hot_idx == -1;
        let firing = laser_selected && mouse.pressed(MouseButton::Left);
        if !firing {
            // JS：非激光选中 + 左键对准可挖掘方块 → 「需要采矿激光」提示（1s 间隔）
            if mouse.pressed(MouseButton::Left) && !laser_selected {
                let origin = p.eye();
                let dir = p.look_dir();
                let mineable = world
                    .raycast(origin, dir, 6.0)
                    .map(|(cell, _, _)| {
                        let def = data::block_by_id(world.get(cell[0], cell[1], cell[2]));
                        def.hard.is_finite() && def.hard > 0.0
                    })
                    .unwrap_or(false);
                if mineable {
                    p.no_laser_t += dt;
                    if p.no_laser_t > 1.0 {
                        p.no_laser_t = -1.5;
                        audio::play(&mut commands, sfx.error.clone(), 0.5, None);
                        p.toast("需要采矿激光：按 0 或滚轮切换到激光枪");
                    }
                } else {
                    p.no_laser_t = 0.0;
                }
            } else {
                p.no_laser_t = 0.0;
            }
            p.mining = None;
            continue;
        }
        let origin = p.eye();
        let dir = p.look_dir();
        let tool_bonus = p.equipment.bonus("laser_efficiency").clamp(0.0, 0.8);
        let mut laser_mul = if p.stats.laser <= 0.0 {
            0.25
        } else {
            1.0 + tool_bonus
        };
        let laser_drain = 1.8 * (1.0 - tool_bonus * 0.7);
        if p.creative() {
            laser_mul = 1.0;
        }
        // Creature fire has a longer reach than block mining, but solid
        // terrain must still occlude it. Without this cap the creature pass
        // happened before the block raycast and allowed shots through walls.
        let obstruction = world
            .raycast(origin, dir, 22.0)
            .map(|(_, _, distance)| distance)
            .unwrap_or(22.0);
        let mut best = obstruction.min(22.0);
        let mut hit_ent = None;
        for (ent, c, tf) in creatures.p0().iter() {
            let center = tf.translation + Vec3::Y * c.height * 0.4;
            let oc = center - origin;
            let tca = oc.dot(dir);
            if tca < 0.0 || tca > best {
                continue;
            }
            let d2 = oc.length_squared() - tca * tca;
            if d2 <= c.radius * c.radius {
                best = tca;
                hit_ent = Some(ent);
            }
        }
        if let Some(ent) = hit_ent {
            p.mining = None;
            if let Ok(mut c) = creatures.p1().get_mut(ent) {
                c.shoot_t += dt;
                if c.shoot_t >= 0.28 {
                    c.shoot_t = 0.0;
                    let dmg = if p.creative() {
                        4.0
                    } else if laser_mul < 1.0 {
                        0.5
                    } else {
                        1.0
                    };
                    c.hp -= dmg;
                    c.hit_t = 0.25; // 受击反馈（缩放脉冲）
                    c.aggro_t = 8.0;
                    audio::play(&mut commands, sfx.laser_hit.clone(), 0.5, None);
                }
            }
            if !p.creative() {
                p.stats.laser = (p.stats.laser - laser_drain * dt).max(0.0);
            }
        } else if let Some((cell, _normal, dist)) = world.raycast(origin, dir, 6.0) {
            let def = data::block_by_id(world.get(cell[0], cell[1], cell[2]));
            let hard = def.hard;
            // machines are mineable too (they drop their machine item)
            if hard.is_finite() && hard > 0.0 {
                let same = p.mining.as_ref().map(|m| m.target == cell).unwrap_or(false);
                if !same {
                    p.mining = Some(Mining {
                        target: cell,
                        prog: 0.0,
                        dig_sound_t: 0.0,
                    });
                }
                let creative = p.creative();
                let mult = p.difficulty.drop_mult();
                if let Some(m) = p.mining.as_mut() {
                    m.prog += dt / hard * if creative { 6.0 } else { laser_mul };
                    m.dig_sound_t += dt;
                    if m.dig_sound_t >= 0.22 {
                        m.dig_sound_t = 0.0;
                        audio::play(&mut commands, sfx.dig.clone(), 0.5, None);
                    }
                    if m.prog >= 1.0 {
                        p.mining = None;
                        let drops = def.drops.to_vec();
                        queue.0.push((pe, cell, drops, mult));
                    }
                }
                if !p.creative() {
                    p.stats.laser = (p.stats.laser - laser_drain * dt).max(0.0);
                }
            } else {
                p.mining = None;
                let drain = if dist < 6.0 {
                    laser_drain
                } else {
                    laser_drain * 0.5
                };
                if !p.creative() {
                    p.stats.laser = (p.stats.laser - drain * dt).max(0.0);
                }
            }
        } else {
            p.mining = None;
            if !p.creative() {
                p.stats.laser = (p.stats.laser - laser_drain * 0.5 * dt).max(0.0);
            }
        }
    }
}

// ---------- Placement ----------

pub fn placement_system(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut q: Query<&mut Player>,
    mut world: ResMut<World>,
    sfx: Res<audio::Sfx>,
    ui: Res<UiState>,
    ghost: Query<Entity, With<Ghost>>,
    machines: Query<(&crate::factory::Machine, Entity)>,
    mut placed_ev: MessageWriter<crate::quests::PlacedEvent>,
    mut net_ev: MessageWriter<crate::network::BlockChanged>,
) {
    for mut p in &mut q {
        if p.dead || ui.locked() {
            continue;
        }
        // R: cycle placement direction
        if keys.just_pressed(KeyCode::KeyR) {
            let next = (p.effective_dir() + 1) % 4;
            p.place_dir = Some(next);
            p.toast(format!("朝向：{}", ["东", "南", "西", "北"][next as usize]));
        }
        // snapshot selected slot (owned clone so we can mutate the player later)
        let selected_index = p.hot_slot();
        let sel_slot = selected_index.and_then(|index| p.inv.slots[index].clone());
        let Some((selected_index, slot)) = selected_index.zip(sel_slot) else {
            continue;
        };
        let Some(item_def) = data::item_by_key(&slot.item) else {
            continue;
        };
        let Some(block_key) = item_def.block else {
            continue;
        };
        let b_def = data::block_by_key(block_key);
        let origin = p.eye();
        let dir = p.look_dir();
        let hit = world.raycast(origin, dir, 6.0);
        let target: Option<[i32; 3]> = hit.and_then(|(cell, normal, _)| {
            let t = [
                cell[0] + normal[0],
                cell[1] + normal[1],
                cell[2] + normal[2],
            ];
            if !(0..data::WORLD_H).contains(&t[1]) {
                return None;
            }
            if world.get(t[0], t[1], t[2]) != ids::AIR {
                return None;
            }
            let bx0 = (p.pos.x - W).floor() as i32;
            let bx1 = (p.pos.x + W).floor() as i32;
            let by0 = p.pos.y.floor() as i32;
            let by1 = (p.pos.y + H).floor() as i32;
            let bz0 = (p.pos.z - W).floor() as i32;
            let bz1 = (p.pos.z + W).floor() as i32;
            if t[0] >= bx0
                && t[0] <= bx1
                && t[1] >= by0
                && t[1] <= by1
                && t[2] >= bz0
                && t[2] <= bz1
            {
                return None;
            }
            Some(t)
        });
        if mouse.just_pressed(MouseButton::Right)
            && let Some(t) = target
        {
            let ok = p.inv.count_item(&slot.item) > 0 || p.creative();
            if ok {
                if !p.creative() && p.inv.take_from_slot(selected_index, 1).is_none() {
                    audio::play(&mut commands, sfx.error.clone(), 0.5, None);
                    p.toast("物品状态已变化，请重试");
                    continue;
                }
                world.set(t[0], t[1], t[2], b_def.id);
                net_ev.write(crate::network::BlockChanged {
                    x: t[0],
                    y: t[1],
                    z: t[2],
                    id: b_def.id,
                    dir: p.effective_dir(),
                });
                if b_def.machine.is_some() {
                    // avoid duplicates
                    let exists = machines.iter().any(|(m, _)| m.pos == t);
                    if !exists {
                        crate::factory::spawn_machine(
                            &mut commands,
                            t,
                            block_key,
                            p.effective_dir(),
                        );
                    }
                }
                placed_ev.write(crate::quests::PlacedEvent {
                    block: block_key.to_string(),
                });
                audio::play(&mut commands, sfx.place.clone(), 0.7, None);
            } else {
                audio::play(&mut commands, sfx.error.clone(), 0.5, None);
                p.toast("物品不足");
            }
        }
        if let Ok(e) = ghost.single() {
            if let Some(t) = target {
                let ok = p.inv.count_item(&slot.item) > 0 || p.creative();
                let lb = b_def.lowbox.unwrap_or(1.0);
                commands.entity(e).insert((
                    Ghost {
                        pos: Vec3::new(
                            t[0] as f32 + 0.5,
                            t[1] as f32 + lb * 0.5,
                            t[2] as f32 + 0.5,
                        ),
                        scale: Vec3::new(1.0, lb, 1.0),
                        ok,
                        active: true,
                    },
                    Visibility::Visible,
                ));
            } else {
                commands.entity(e).insert((
                    Ghost {
                        pos: Vec3::ZERO,
                        scale: Vec3::ONE,
                        ok: false,
                        active: false,
                    },
                    Visibility::Hidden,
                ));
            }
        }
    }
}

// ---------- Hotbar / drop ----------

pub fn hotbar_system(
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<AccumulatedMouseScroll>,
    mut q: Query<&mut Player>,
    mut commands: Commands,
    world: Res<World>,
    sfx: Res<audio::Sfx>,
    ui: Res<UiState>,
    icons: Res<crate::ui::IconMaterials>,
) {
    for mut p in &mut q {
        if p.dead || ui.locked() {
            continue;
        }
        let digits = [
            (KeyCode::Digit0, -1),
            (KeyCode::Digit1, 0),
            (KeyCode::Digit2, 1),
            (KeyCode::Digit3, 2),
            (KeyCode::Digit4, 3),
            (KeyCode::Digit5, 4),
            (KeyCode::Digit6, 5),
            (KeyCode::Digit7, 6),
            (KeyCode::Digit8, 7),
            (KeyCode::Digit9, 8),
        ];
        for (k, n) in digits {
            if keys.just_pressed(k) {
                p.hot_idx = n;
                audio::play(&mut commands, sfx.click.clone(), 0.35, None);
            }
        }
        let mut scroll = 0i32;
        let wy = wheel.delta.y;
        scroll += if wy > 0.0 {
            1
        } else if wy < 0.0 {
            -1
        } else {
            0
        };
        if scroll != 0 {
            let cur = if p.hot_idx == -1 { 9 } else { p.hot_idx };
            let next = (cur + scroll + 10) % 10;
            p.hot_idx = if next == 9 { -1 } else { next };
            audio::play(&mut commands, sfx.click.clone(), 0.3, None);
        }
        // G: drop item
        if keys.just_pressed(KeyCode::KeyG) {
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            if let Some(i) = p.hot_slot()
                && let Some(s) = p.inv.slots[i].clone()
            {
                let n = if shift { s.n } else { 1 };
                let dir = p.look_dir();
                let drop_pos = p.pos + Vec3::new(dir.x * 0.7, -0.15 + dir.y * 0.5, dir.z * 0.7);
                let vel = Vec3::new(dir.x * 6.0, dir.y * 6.0 + 2.2, dir.z * 6.0);
                let Some(taken) = p.inv.take_from_slot(i, n) else {
                    continue;
                };
                spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    drop_pos,
                    vel,
                    taken.item,
                    taken.n,
                    1.2,
                );
                audio::play(&mut commands, sfx.click.clone(), 0.4, None);
            }
        }
    }
}

/// Apply queued block breaks (world mutation must not happen in a query loop over World).
pub fn break_system(
    mut queue: ResMut<BreakQueue>,
    mut world: ResMut<World>,
    mut q: Query<&mut Player>,
    machines: Query<(
        Entity,
        &crate::factory::Machine,
        &crate::factory::MachineState,
    )>,
    mut commands: Commands,
    icons: Res<crate::ui::IconMaterials>,
    mut net_ev: MessageWriter<crate::network::BlockChanged>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut feedback: ResMut<crate::feedback::FeedbackAssets>,
    sfx: Res<audio::Sfx>,
) {
    for (player_e, cell, drops, mult) in queue.0.drain(..) {
        let mut rng = crate::rng::Rng::new(
            (cell[0] as u32).wrapping_mul(73856093)
                ^ (cell[2] as u32).wrapping_mul(19349663)
                ^ (cell[1] as u32),
        );
        // 拆机内容返还（JS Factory.remove 退款），实体由 machine_sync_system 兜底清理
        if let Some((e, _m, st)) = machines.iter().find(|(_, m, _)| m.pos == cell) {
            for (item, n) in crate::factory::machine_refund(st) {
                spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    Vec3::new(
                        cell[0] as f32 + 0.5,
                        cell[1] as f32 + 0.5,
                        cell[2] as f32 + 0.5,
                    ),
                    Vec3::new((rng.next() - 0.5) * 2.2, 2.6, (rng.next() - 0.5) * 2.2),
                    item,
                    n,
                    0.4,
                );
            }
            commands.entity(e).despawn();
        }
        let broken_id = world.get(cell[0], cell[1], cell[2]);
        crate::feedback::spawn_block_burst(
            &mut commands,
            &mut feedback,
            &mut meshes,
            &mut materials,
            Vec3::new(
                cell[0] as f32 + 0.5,
                cell[1] as f32 + 0.5,
                cell[2] as f32 + 0.5,
            ),
            broken_id,
            (cell[0] as u32).wrapping_mul(73856093) ^ (cell[2] as u32).wrapping_mul(19349663),
        );
        let pitch = match data::block_by_id(broken_id).key {
            "glass" | "ice" => 1.32,
            "metal" | "iron_ore" | "titanium_ore" => 0.78,
            "leaves" | "fern" | "sodium_plant" | "oxygen_plant" => 1.15,
            "sand" | "snow" | "salt" => 1.08,
            _ => 1.0,
        };
        audio::play(&mut commands, sfx.break_block.clone(), 0.7, Some(pitch));
        world.set(cell[0], cell[1], cell[2], ids::AIR);
        net_ev.write(crate::network::BlockChanged {
            x: cell[0],
            y: cell[1],
            z: cell[2],
            id: ids::AIR,
            dir: 0,
        });
        let above = world.get(cell[0], cell[1] + 1, cell[2]);
        if data::block_by_id(above).cross {
            // 上方十字植物一并掉落（JS player.js:968-972）
            let plant = data::block_by_id(above);
            for d in plant.drops {
                if rng.next() <= d.chance {
                    let n = (d.n as f32 * mult).round() as i32;
                    if n > 0 {
                        spawn_drop(
                            &mut commands,
                            &world,
                            &icons,
                            Vec3::new(
                                cell[0] as f32 + 0.5,
                                cell[1] as f32 + 1.5,
                                cell[2] as f32 + 0.5,
                            ),
                            Vec3::new((rng.next() - 0.5) * 2.2, 2.6, (rng.next() - 0.5) * 2.2),
                            d.item.to_string(),
                            n,
                            0.4,
                        );
                    }
                }
            }
            world.set(cell[0], cell[1] + 1, cell[2], ids::AIR);
            net_ev.write(crate::network::BlockChanged {
                x: cell[0],
                y: cell[1] + 1,
                z: cell[2],
                id: ids::AIR,
                dir: 0,
            });
        }
        for d in drops {
            if rng.next() <= d.chance {
                let n = (d.n as f32 * mult).round() as i32;
                if n > 0 {
                    spawn_drop(
                        &mut commands,
                        &world,
                        &icons,
                        Vec3::new(
                            cell[0] as f32 + 0.5,
                            cell[1] as f32 + 0.5,
                            cell[2] as f32 + 0.5,
                        ),
                        Vec3::new((rng.next() - 0.5) * 2.2, 2.6, (rng.next() - 0.5) * 2.2),
                        d.item.to_string(),
                        n,
                        0.4,
                    );
                }
            }
        }
        if let Ok(mut p) = q.get_mut(player_e) {
            p.mining = None;
        }
    }
}

// ---------- Cursor grab management ----------

pub fn cursor_system(
    mut q: Query<(&mut Window, &mut CursorOptions), With<bevy::window::PrimaryWindow>>,
    ui: Res<UiState>,
) {
    let want_lock = !ui.locked();
    for (_w, mut opts) in &mut q {
        let mode = if want_lock {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        if opts.grab_mode != mode {
            opts.grab_mode = mode;
            opts.visible = !want_lock;
            println!("CURSOR locked={} visible={}", want_lock, !want_lock);
        }
    }
}

// ---------- Laser beam & interact prompt (former main.rs gameplay HUD) ----------

#[derive(Component)]
pub struct Beam;

pub fn beam_system(
    mut q: Query<(&mut Transform, &mut Visibility), With<Beam>>,
    player: Query<&Player>,
    mouse: Res<ButtonInput<MouseButton>>,
    world: Res<World>,
    ui: Res<UiState>,
) {
    for (mut tf, mut vis) in &mut q {
        let Ok(p) = player.single() else {
            *vis = Visibility::Hidden;
            continue;
        };
        let firing = p.hot_idx == -1 && mouse.pressed(MouseButton::Left) && !ui.locked() && !p.dead;
        if !firing {
            *vis = Visibility::Hidden;
            continue;
        }
        let origin = p.eye();
        let dir = p.look_dir();
        let dist = world
            .raycast(origin, dir, 22.0)
            .map(|(_, _, d)| d)
            .unwrap_or(22.0)
            .max(0.3);
        let mid = origin + dir * (dist * 0.5);
        tf.translation = mid;
        tf.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
        tf.scale = Vec3::new(1.0, 1.0, dist);
        *vis = Visibility::Visible;
    }
}

pub fn prompt_system(
    mut ui_state: ResMut<UiState>,
    player: Query<&Player>,
    world: Res<World>,
    machines: Query<&crate::factory::Machine>,
    game: Res<crate::space::SpaceGame>,
) {
    if ui_state.locked() {
        ui_state.prompt = None;
        return;
    }
    let Ok(p) = player.single() else {
        ui_state.prompt = None;
        return;
    };
    // 飞船优先（与登船判定半径一致：JS 4.5）
    if p.pos.distance(game.ship_pos) < 4.5 {
        ui_state.prompt = Some("[E] 检查飞船 / 登船".into());
        return;
    }
    let mut prompt = None;
    if let Some((cell, _n, dist)) = world.raycast(p.eye(), p.look_dir(), 5.0)
        && dist <= 5.0
        && let Some(m) = machines.iter().find(|m| m.pos == cell)
    {
        prompt = Some(format!("[E] 打开{}", m.kind.label()));
    }
    ui_state.prompt = prompt;
}

// ---------- Enter/exit cursor handling ----------

fn on_enter_playing(mut windows: Query<&mut CursorOptions, With<bevy::window::PrimaryWindow>>) {
    for mut opts in &mut windows {
        opts.grab_mode = CursorGrabMode::Locked;
        opts.visible = false;
    }
}

/// 返回主菜单时释放鼠标（否则菜单里光标不可见/被锁）。
fn on_exit_playing(mut windows: Query<&mut CursorOptions, With<bevy::window::PrimaryWindow>>) {
    for mut opts in &mut windows {
        opts.grab_mode = CursorGrabMode::None;
        opts.visible = true;
    }
}

// ---------- Plugin ----------

/// Player controller plugin: movement/combat/inventory systems plus the
/// shared camera bootstrap and enter/exit cursor locking.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerCameraMode>()
            .init_resource::<BreakQueue>()
            .add_systems(OnEnter(GameState::Playing), on_enter_playing)
            .add_systems(OnExit(GameState::Playing), on_exit_playing)
            .add_systems(
                Update,
                (
                    movement_system.run_if(ground_mode),
                    collision_system.run_if(ground_mode),
                    survival_system.run_if(ground_mode),
                    mining_system.run_if(ground_mode),
                    break_system.run_if(ground_mode),
                    placement_system.run_if(ground_mode),
                    hotbar_system.run_if(ground_mode),
                )
                    .chain()
                    .in_set(GameSet::GroundPlayer)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    look_system.run_if(walk_look_mode),
                    camera_toggle_system.run_if(in_planet_mode),
                    camera_system.run_if(in_planet_mode),
                )
                    .chain()
                    .in_set(GameSet::LateLook)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                cursor_system
                    .in_set(GameSet::LateSwitchCursor)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                ((
                    beam_system.run_if(in_planet_mode),
                    prompt_system.run_if(in_planet_mode),
                )
                    .chain()
                    .in_set(GameSet::HudGhostPlayer),)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod equipment_tests {
    use super::*;

    #[test]
    fn equipment_replaces_only_its_slot_and_changes_caps() {
        let mut player = Player::new(crate::data::Difficulty::Normal);
        assert_eq!(player.stat_max("o2"), 100.0);
        assert!(player.equipment.equip("oxygen_tank").unwrap().is_none());
        assert_eq!(player.stat_max("o2"), 180.0);
        assert!(player.equipment.equip("thermal_module").unwrap().is_none());
        let previous = player.equipment.equip("cryo_module").unwrap();
        assert_eq!(previous.as_deref(), Some("thermal_module"));
        assert_eq!(player.hazard_resistance("cold"), 0.65);
        assert_eq!(player.hazard_resistance("heat"), 0.0);
    }

    #[test]
    fn equipment_sanitizer_removes_wrong_slot_items() {
        let mut equipment = Equipment {
            suit: Some("oxygen_tank".into()),
            tool: Some("laser_mk2".into()),
            ..default()
        };
        equipment.sanitize();
        assert!(equipment.suit.is_none());
        assert_eq!(equipment.tool.as_deref(), Some("laser_mk2"));
    }
}

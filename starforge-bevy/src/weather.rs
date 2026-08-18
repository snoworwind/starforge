//! Planet climate visuals: ground cloud banks, biome weather particles, and
//! procedural cloud shells visible from space.

use bevy::prelude::*;

use crate::player::Player;
use crate::save::Settings;
use crate::space::{FlightMode, SpaceScene};
use crate::world::World;

#[derive(Component)]
pub struct GroundCloud {
    base: Vec3,
    wind: Vec2,
}

#[derive(Component)]
pub struct WeatherParticle {
    speed: f32,
    index: u32,
    generation: u32,
}

#[derive(Component)]
pub struct SpaceCloud {
    planet: Entity,
    speed: f32,
}

#[derive(Resource, Default)]
pub struct ClimateRuntime {
    fingerprint: Option<(u32, &'static str, bool, bool)>,
    elapsed: f32,
}

#[derive(Clone, Copy)]
struct WeatherDef {
    color: Color,
    speed: f32,
    count: u32,
    size: Vec3,
}

fn weather_def(key: &str) -> WeatherDef {
    let (rgb, speed, count, size) = match key {
        "ocean" => ((0.56, 0.77, 0.91), 20.0, 300, (0.035, 0.36, 0.035)),
        "murk" => ((0.60, 0.85, 0.69), 14.0, 240, (0.055, 0.16, 0.055)),
        "fungal" => ((0.85, 0.72, 0.94), 12.0, 220, (0.06, 0.14, 0.06)),
        "frozen" => ((0.94, 0.97, 0.98), 5.0, 240, (0.09, 0.09, 0.09)),
        "crystal" => ((0.85, 0.96, 0.98), 5.0, 220, (0.09, 0.09, 0.09)),
        "ashen" => ((0.54, 0.54, 0.54), 7.0, 200, (0.07, 0.08, 0.07)),
        "volcanic" => ((1.0, 0.54, 0.23), 6.0, 160, (0.09, 0.11, 0.09)),
        "desert" => ((0.91, 0.82, 0.63), 9.0, 140, (0.06, 0.08, 0.06)),
        "amber" => ((0.94, 0.78, 0.38), 8.0, 130, (0.06, 0.08, 0.06)),
        "ferrous" => ((0.66, 0.60, 0.88), 16.0, 150, (0.045, 0.28, 0.045)),
        "alien" => ((0.69, 0.44, 0.88), 10.0, 200, (0.07, 0.10, 0.07)),
        "salt" => ((0.96, 0.98, 0.99), 6.0, 160, (0.06, 0.06, 0.06)),
        "obsidian" => ((1.0, 0.42, 0.23), 7.0, 130, (0.08, 0.12, 0.08)),
        "redmoss" => ((0.85, 0.47, 0.41), 10.0, 150, (0.06, 0.08, 0.06)),
        "hive" => ((0.91, 0.72, 0.38), 8.0, 170, (0.06, 0.08, 0.06)),
        _ => ((0.72, 0.86, 0.95), 16.0, 220, (0.035, 0.32, 0.035)),
    };
    WeatherDef {
        color: Color::srgba(rgb.0, rgb.1, rgb.2, 0.62),
        speed,
        count,
        size: Vec3::new(size.0, size.1, size.2),
    }
}

fn particle_position(world: &World, player: Vec3, index: u32, generation: u32) -> Vec3 {
    let mut rng = crate::rng::Rng::new(
        world.seed ^ index.wrapping_mul(0x9E37_79B9) ^ generation.wrapping_mul(0x85EB_CA6B),
    );
    let x = player.x + (rng.next() - 0.5) * 90.0;
    let z = player.z + (rng.next() - 0.5) * 90.0;
    let floor = world.g.height_at(x.floor(), z.floor()) as f32 + 1.0;
    Vec3::new(x, floor.max(player.y - 4.0) + 20.0 + rng.next() * 22.0, z)
}

/// Rebuilds climate entities when the planet or either graphics toggle changes,
/// then keeps the effects wrapped around the player like the original client.
#[allow(clippy::too_many_arguments)]
pub fn climate_system(
    time: Res<Time>,
    settings: Res<Settings>,
    mode: Res<FlightMode>,
    world: Res<World>,
    player: Query<&Player>,
    mut runtime: ResMut<ClimateRuntime>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    climate_entities: Query<Entity, Or<(With<GroundCloud>, With<WeatherParticle>)>>,
    mut clouds: Query<(&GroundCloud, &mut Transform, &mut Visibility), Without<WeatherParticle>>,
    mut particles: Query<
        (&mut WeatherParticle, &mut Transform, &mut Visibility),
        Without<GroundCloud>,
    >,
) {
    let Ok(player) = player.single() else { return };
    let fingerprint = (
        world.seed,
        world.biome().key,
        settings.clouds,
        settings.weather,
    );
    if runtime.fingerprint != Some(fingerprint) {
        for entity in &climate_entities {
            commands.entity(entity).despawn();
        }
        runtime.fingerprint = Some(fingerprint);
        runtime.elapsed = 0.0;

        if settings.clouds {
            // A single opaque-looking icosphere per cloud reads as floating
            // bubbles.  Use smooth, translucent lobes with three density
            // layers so each cloud has a soft core, a shaded middle, and a
            // broken wispy edge.
            let mesh = meshes.add(Sphere::new(1.0));
            let cloud_materials = [
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.92, 0.96, 1.0, 0.16),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    cull_mode: None,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(1.0, 1.0, 1.0, 0.24),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    cull_mode: None,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.72, 0.82, 0.94, 0.11),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    cull_mode: None,
                    ..default()
                }),
            ];
            let mut rng = crate::rng::Rng::new(world.seed ^ 0xC10D5);
            for _ in 0..48 {
                let center = Vec3::new(
                    (rng.next() - 0.5) * 1100.0,
                    126.0 + rng.next() * 34.0,
                    (rng.next() - 0.5) * 1100.0,
                );
                let parts = 5 + (rng.next() * 6.0) as usize;
                let wind = Vec2::new(1.5 + rng.next() * 2.5, (rng.next() - 0.5) * 0.45);
                for part in 0..parts {
                    let edge = part == 0 || part + 1 == parts;
                    let base = center
                        + Vec3::new(
                            (rng.next() - 0.5) * 34.0,
                            (rng.next() - 0.5) * 8.0,
                            (rng.next() - 0.5) * 34.0,
                        );
                    let scale = Vec3::new(
                        10.0 + rng.next() * 20.0,
                        if edge {
                            2.4 + rng.next() * 3.0
                        } else {
                            4.5 + rng.next() * 6.5
                        },
                        10.0 + rng.next() * 20.0,
                    );
                    commands.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(cloud_materials[part % cloud_materials.len()].clone()),
                        Transform::from_translation(base).with_scale(scale),
                        GroundCloud { base, wind },
                        crate::InGame,
                    ));
                }
            }
        }

        if settings.weather {
            let def = weather_def(world.biome().key);
            let mesh = meshes.add(Cuboid::new(def.size.x, def.size.y, def.size.z));
            let material = materials.add(StandardMaterial {
                base_color: def.color,
                emissive: def.color.to_linear() * 0.12,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            });
            for index in 0..def.count {
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(particle_position(&world, player.pos, index, 0)),
                    WeatherParticle {
                        speed: def.speed,
                        index,
                        generation: 0,
                    },
                    crate::InGame,
                ));
            }
        }
        return;
    }

    let dt = time.delta_secs();
    runtime.elapsed += dt;
    let show_clouds = settings.clouds && mode.ground_scene();
    let show_weather = settings.weather && matches!(*mode, FlightMode::Planet | FlightMode::Seated);
    let half = 550.0;
    for (cloud, mut transform, mut visibility) in &mut clouds {
        *visibility = if show_clouds {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if show_clouds {
            let x = player.pos.x
                + (cloud.base.x + runtime.elapsed * cloud.wind.x - player.pos.x + half)
                    .rem_euclid(1100.0)
                - half;
            let z = player.pos.z
                + (cloud.base.z + runtime.elapsed * cloud.wind.y - player.pos.z + half)
                    .rem_euclid(1100.0)
                - half;
            transform.translation = Vec3::new(x, cloud.base.y, z);
        }
    }
    for (mut particle, mut transform, mut visibility) in &mut particles {
        *visibility = if show_weather {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !show_weather {
            continue;
        }
        transform.translation.y -= particle.speed * dt;
        let floor = world.g.height_at(
            transform.translation.x.floor(),
            transform.translation.z.floor(),
        ) as f32
            + 1.0;
        if transform.translation.y < floor {
            particle.generation = particle.generation.wrapping_add(1);
            transform.translation =
                particle_position(&world, player.pos, particle.index, particle.generation);
        }
    }
}

fn cloud_threshold(key: &str) -> f32 {
    match key {
        "lush" => 0.55,
        "ocean" => 0.52,
        "fungal" => 0.50,
        "frozen" => 0.44,
        "murk" => 0.42,
        "alien" | "crystal" => 0.40,
        "hive" => 0.38,
        "redmoss" => 0.36,
        "ferrous" => 0.34,
        "salt" => 0.30,
        "obsidian" => 0.28,
        "amber" => 0.24,
        "desert" => 0.20,
        "volcanic" => 0.16,
        "ashen" => 0.12,
        _ => 0.40,
    }
}

fn cloud_texture(images: &mut Assets<Image>, key: &str, seed: u32) -> Handle<Image> {
    let (width, height) = (256usize, 128usize);
    let noise = crate::rng::Noise2::new(seed);
    let threshold = cloud_threshold(key);
    let stormy = matches!(key, "ferrous" | "murk");
    let mut bytes = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let seam = x as f32 / width as f32;
            let sample = |x: f32, y: f32| {
                let warp_x = noise.fbm2(x * 0.018 + 19.0, y * 0.024 - 7.0, 3, 2.0, 0.5) * 8.0;
                let warp_y = noise.fbm2(x * 0.021 - 11.0, y * 0.017 + 5.0, 3, 2.0, 0.5) * 6.0;
                noise.fbm2((x + warp_x) * 0.042, (y + warp_y) * 0.068, 5, 2.0, 0.5) * 0.5 + 0.5
            };
            let a = sample(x as f32, y as f32);
            let b = sample(x as f32 - width as f32, y as f32);
            let mut value = a * (1.0 - seam) + b * seam;
            let v = y as f32 / height as f32;
            value *= 0.88 + (1.0 - (v * 2.0 - 1.0).abs()) * 0.12;
            if stormy {
                value += (seam * std::f32::consts::TAU * 3.0).sin() * 0.16;
            }
            let opacity = ((value - threshold) / 0.30).clamp(0.0, 1.0);
            let opacity = opacity * opacity * (3.0 - 2.0 * opacity);
            let offset = (y * width + x) * 4;
            bytes[offset..offset + 3].fill(255);
            bytes[offset + 3] = (opacity * 220.0) as u8;
        }
    }
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        bytes,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    images.add(image)
}

/// Adds a separate procedural cloud shell to every space-scene planet and
/// rotates it slowly so the globe remains visibly alive from orbit.
#[allow(clippy::too_many_arguments)]
pub fn space_cloud_system(
    time: Res<Time>,
    settings: Res<Settings>,
    scene: Option<Res<SpaceScene>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(Entity, &SpaceCloud)>,
    mut clouds: Query<(&SpaceCloud, &mut Transform)>,
) {
    if !settings.clouds {
        for (entity, _) in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }
    let Some(scene) = scene else { return };
    for planet in &scene.planets {
        if existing
            .iter()
            .any(|(_, cloud)| cloud.planet == planet.entity)
        {
            continue;
        }
        let seed = 90_210 + planet.def.id as u32 * 777 + planet.def.biome.len() as u32 * 131;
        let texture = cloud_texture(&mut images, planet.def.biome, seed);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.85),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            cull_mode: None,
            ..default()
        });
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(Sphere::new(planet.def.radius * 1.035))),
                MeshMaterial3d(material),
                Transform::from_rotation(Quat::from_rotation_y(planet.def.id as f32 * 1.7)),
                SpaceCloud {
                    planet: planet.entity,
                    speed: 0.008 + planet.def.id as f32 * 0.0007,
                },
                crate::InGame,
            ))
            .id();
        commands.entity(planet.entity).add_child(entity);
    }
    for (cloud, mut transform) in &mut clouds {
        transform.rotate_y(cloud.speed * time.delta_secs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_biome_has_weather_and_cloud_density() {
        for biome in crate::data::BIOMES {
            let weather = weather_def(biome.key);
            assert!(weather.count >= 100);
            assert!(weather.speed > 0.0);
            assert!((0.0..=1.0).contains(&cloud_threshold(biome.key)));
        }
    }
}

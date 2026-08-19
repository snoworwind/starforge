#![doc = include_str!("../README.md")]

mod compute;
/// Controls the compute shader which renders the volumetric clouds.
pub mod config;
/// A utility plugin to control the camera using keyboard and mouse.
#[cfg(feature = "fly_camera")]
pub mod fly_camera;
mod images;
mod render;
mod skybox;
#[cfg(feature = "debug")]
mod ui;
mod uniforms;
use bevy::prelude::*;

#[cfg(feature = "debug")]
use self::ui::ui_system;
#[cfg(feature = "debug")]
use bevy_egui::EguiPrimaryContextPass;

use crate::{
    compute::CameraMatrices,
    images::{build_images, resize_render_images},
    render::{CloudsMaterial, CloudsShaderPlugin},
    skybox::{SkyboxMaterials, init_skybox_mesh, update_skybox_transform},
    uniforms::CloudsImage,
};

use self::compute::CloudsComputePlugin;

pub use config::CloudsConfig;
pub use skybox::SkyboxPlane;

/// A plugin for rendering clouds.
///
/// The configuration of the clouds can be changed using the [`CloudsConfig`] resource.
pub struct CloudsPlugin;

fn resize_cloud_render_target(
    mut images: ResMut<Assets<Image>>,
    config: Res<CloudsConfig>,
    clouds_image: Option<Res<CloudsImage>>,
    mut current_size: Local<Option<UVec2>>,
) {
    let Some(clouds_image) = clouds_image else {
        return;
    };
    let desired = UVec2::new(
        config.render_resolution.x.round().max(8.0) as u32,
        config.render_resolution.y.round().max(8.0) as u32,
    );
    if *current_size == Some(desired) {
        return;
    }

    let already_matches = images
        .get(&clouds_image.cloud_render_image)
        .map(|image| {
            image.texture_descriptor.size.width == desired.x
                && image.texture_descriptor.size.height == desired.y
        })
        .unwrap_or(false);
    if !already_matches {
        resize_render_images(
            &mut images,
            &clouds_image.cloud_render_image,
            &clouds_image.sky_image,
            desired.x,
            desired.y,
        );
    }
    *current_size = Some(desired);
}

impl Plugin for CloudsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CloudsConfig::default())
            .add_plugins((CloudsComputePlugin, CloudsShaderPlugin))
            .add_systems(Startup, clouds_setup)
            .add_systems(PostUpdate, resize_cloud_render_target)
            .add_systems(
                PostUpdate,
                (update_skybox_transform, update_camera_matrices)
                    .after(TransformSystems::Propagate),
            );
        #[cfg(feature = "debug")]
        app.add_systems(EguiPrimaryContextPass, ui_system);
    }
}

fn clouds_setup(
    mut commands: Commands,
    images: ResMut<Assets<Image>>,
    meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudsMaterial>>,
) {
    let (cloud_render_image, cloud_atlas_image, cloud_worley_image, sky_image) =
        build_images(images);

    let material = materials.add(CloudsMaterial {
        cloud_render_image: cloud_render_image.clone(),
        cloud_atlas_image: cloud_atlas_image.clone(),
        cloud_worley_image: cloud_worley_image.clone(),
        sky_image: sky_image.clone(),
    });
    init_skybox_mesh(
        &mut commands,
        meshes,
        SkyboxMaterials::from_one_material(MeshMaterial3d(material.clone())),
    );
    commands.insert_resource(CloudsImage {
        cloud_render_image,
        cloud_atlas_image,
        cloud_worley_image,
        sky_image,
    });
    commands.insert_resource(CameraMatrices {
        translation: Vec3::ZERO,
        inverse_camera_projection: Mat4::IDENTITY,
        inverse_camera_view: Mat4::IDENTITY,
    });
}

fn update_camera_matrices(
    cam_query: Single<(&GlobalTransform, &Camera), With<Camera3d>>,
    mut config: ResMut<CameraMatrices>,
) {
    let (camera_transform, camera) = *cam_query;
    config.translation = camera_transform.translation();
    config.inverse_camera_view = camera_transform.to_matrix();
    config.inverse_camera_projection = camera.computed.clip_from_view.inverse();
}

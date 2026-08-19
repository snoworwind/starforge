use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
};

// Default cloud render target. The in-game tuning panel can resize the two
// per-frame images; the atlas remains independent and reusable.
pub const RENDER_WIDTH: u32 = 1536;
pub const RENDER_HEIGHT: u32 = 864;
pub const ATLAS_SIZE: u32 = 1536;

fn storage_image_2d(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4 * 4 * 2],
        TextureFormat::Rgba32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
    image
}

pub(crate) fn resize_render_images(
    images: &mut Assets<Image>,
    cloud_render_image: &Handle<Image>,
    sky_image: &Handle<Image>,
    width: u32,
    height: u32,
) {
    if let Some(mut image) = images.get_mut(cloud_render_image) {
        *image = storage_image_2d(width, height);
    }
    if let Some(mut image) = images.get_mut(sky_image) {
        *image = storage_image_2d(width, height);
    }
}

pub fn build_images(
    mut images: ResMut<Assets<Image>>,
) -> (Handle<Image>, Handle<Image>, Handle<Image>, Handle<Image>) {
    let cloud_render_image = storage_image_2d(RENDER_WIDTH, RENDER_HEIGHT);

    let mut cloud_atlas_image = Image::new_fill(
        Extent3d {
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4 * 4 * 2],
        TextureFormat::Rgba32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    cloud_atlas_image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;

    let mut cloud_worley_image = Image::new_fill(
        Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 32,
        },
        TextureDimension::D3,
        &[0; 4 * 4 * 2],
        TextureFormat::Rgba32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    cloud_worley_image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;

    let sky_image = storage_image_2d(RENDER_WIDTH, RENDER_HEIGHT);

    (
        images.add(cloud_render_image),
        images.add(cloud_atlas_image),
        images.add(cloud_worley_image),
        images.add(sky_image),
    )
}

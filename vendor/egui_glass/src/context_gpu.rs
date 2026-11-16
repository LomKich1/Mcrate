use std::sync::Arc;

use egui::{Color32, ColorImage, Context};
use egui_wgpu::RenderState;

use crate::{GlassContext, GlassError};

fn render_state() -> RenderState {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance.request_adapter(&Default::default()).await.expect("native GPU adapter required");
        let (device, queue) = adapter.request_device(&Default::default()).await.expect("GPU device required");
        let target_format = wgpu::TextureFormat::Rgba8Unorm;
        let renderer = egui_wgpu::Renderer::new(&device, target_format, Default::default());
        RenderState {
            instance, adapter, available_adapters: vec![], device, queue, target_format,
            renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
        }
    })
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn managed_setup_rejects_duplicates_and_preserves_backdrop_on_error() {
    let rs = render_state();
    let ctx = Context::default();
    assert!(matches!(GlassContext::new(&ctx, &rs, 3), Err(GlassError::UnsupportedSampleCount(3))));
    let glass = GlassContext::new(&ctx, &rs, 0).unwrap();
    let id = glass.set_backdrop(&ColorImage::filled([4, 4], Color32::RED)).unwrap();
    assert!(matches!(GlassContext::new(&ctx, &rs, 1), Err(GlassError::AlreadyInitialized)));
    let mut invalid = ColorImage::filled([1, 1], Color32::WHITE);
    invalid.size = [2, 2];
    assert!(matches!(glass.set_backdrop(&invalid), Err(GlassError::InvalidImage { .. })));
    assert_eq!(crate::backdrop::backdrop_state(&ctx).unwrap().texture, id);
    assert!(rs.renderer.read().texture(&id).is_some());
    assert!(crate::live::test_support::contains_texture(&rs.renderer.read(), id));
    let replacement = glass.set_backdrop(&ColorImage::filled([4, 4], Color32::BLUE)).unwrap();
    assert_ne!(replacement, id);
    assert!(rs.renderer.read().texture(&id).is_none());
    assert!(!crate::live::test_support::contains_texture(&rs.renderer.read(), id));
    assert!(crate::live::test_support::contains_texture(&rs.renderer.read(), replacement));
    let legacy = render_state();
    crate::init(&legacy, 1);
    assert!(matches!(GlassContext::new(&ctx, &legacy, 1), Err(GlassError::AlreadyInitialized)));
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn native_texture_clones_keep_both_registrations_until_final_drop() {
    let rs = render_state();
    let glass = GlassContext::new(&Context::default(), &rs, 1).unwrap();
    let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("managed texture test"), size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
    });
    let handle = glass.register_native_texture(&texture.create_view(&Default::default()));
    let id = handle.id();
    let clone = handle.clone();
    drop(handle);
    drop(glass);
    assert!(rs.renderer.read().texture(&id).is_some());
    assert!(crate::live::test_support::contains_texture(&rs.renderer.read(), id));
    drop(clone);
    assert!(rs.renderer.read().texture(&id).is_none());
    assert!(!crate::live::test_support::contains_texture(&rs.renderer.read(), id));
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn managed_fonts_reach_visible_and_twin_contexts() {
    let rs = render_state();
    let ctx = Context::default();
    let glass = GlassContext::new(&ctx, &rs, 1).unwrap();
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(egui::FontFamily::Name("managed-test".into()), fonts.families[&egui::FontFamily::Proportional].clone());
    for family in ["initial", "updated"] {
        fonts.families.insert(egui::FontFamily::Name(family.into()), fonts.families[&egui::FontFamily::Proportional].clone());
        glass.set_fonts(fonts.clone());
        for _ in 0..2 {
            let mut calls = 0;
            ctx.run_ui(Default::default(), |ui| {
                glass.live_backdrop().clone().run(ui, Color32::WHITE, |ui| {
                    assert_eq!(ui.fonts(|view| view.definitions().families.clone()), fonts.families, "font families in layout {calls}");
                    calls += 1;
                });
            }).drop_without_applying_deltas();
            assert!(calls >= 2, "both visible and offscreen layouts must be checked");
        }
    }
    let legacy_rs = render_state();
    crate::init(&legacy_rs, 1);
    let legacy = crate::LiveBackdrop::new(&legacy_rs, Some(fonts.clone()));
    ctx.run_ui(Default::default(), |ui| {
        legacy.run(ui, Color32::WHITE, |ui| {
            assert_eq!(ui.fonts(|view| view.definitions().families.clone()), fonts.families);
        });
    }).drop_without_applying_deltas();
}

/// Renders one frame at 1 point per pixel and returns the RGBA pixel at `at`.
fn render_pixel(rs: &RenderState, ctx: &Context, size: [u32; 2], at: [u32; 2], add_contents: impl FnMut(&mut egui::Ui)) -> [u8; 4] {
    let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: 1.0 };
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0] as f32, size[1] as f32));
    let output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, add_contents);
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    let target = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("opacity test target"), size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: rs.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
    });
    let row_bytes = (size[0] * 4).div_ceil(256) * 256;
    let buffer = rs.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("opacity test readback"), size: u64::from(row_bytes) * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
    });
    let mut renderer = rs.renderer.write();
    let mut textures = output.textures_delta;
    for (id, deltas) in &textures.set {
        for delta in deltas {
            renderer.update_texture(&rs.device, &rs.queue, *id, delta);
        }
    }
    let mut encoder = rs.device.create_command_encoder(&Default::default());
    let commands = renderer.update_buffers(&rs.device, &rs.queue, &mut encoder, &primitives, &screen);
    {
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        }).forget_lifetime();
        renderer.render(&mut pass, &primitives, &screen);
    }
    for id in &textures.free {
        renderer.free_texture(id);
    }
    textures.clear();
    encoder.copy_texture_to_buffer(target.as_image_copy(), wgpu::TexelCopyBufferInfo {
        buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row_bytes), rows_per_image: Some(size[1]) },
    }, target.size());
    rs.queue.submit(commands.into_iter().chain([encoder.finish()]));
    buffer.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("readback map"));
    rs.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
    let data = buffer.slice(..).get_mapped_range().expect("mapped readback");
    let offset = (at[1] * row_bytes + at[0] * 4) as usize;
    data[offset..offset + 4].try_into().unwrap()
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn disabled_glass_fades_towards_the_backdrop_like_other_widgets() {
    let rs = render_state();
    let ctx = Context::default();
    let glass = GlassContext::new(&ctx, &rs, 1).unwrap();
    glass.set_backdrop(&ColorImage::filled([4, 4], Color32::BLACK)).unwrap();
    let button_pixel = |enabled: bool| {
        render_pixel(&rs, &ctx, [160, 80], [30, 40], |ui| {
            crate::show_backdrop(ui, ui.max_rect());
            ui.add_enabled_ui(enabled, |ui| {
                crate::GlassButton::new("").min_size(egui::vec2(120.0, 60.0)).show(ui);
            });
        })[0]
    };
    // White tint over a black backdrop: full glass is lighter, faded glass lies between.
    let (enabled, disabled) = (button_pixel(true), button_pixel(false));
    assert!(enabled > 20, "enabled glass should lighten the black backdrop, got {enabled}");
    assert!(disabled > 0 && disabled < enabled, "disabled glass should be fainter: enabled {enabled}, disabled {disabled}");
}

//! GPU-эффект распада строки на фрагменты при удалении трека.
//!
//! egui делает один снимок уже отрисованного окна, CPU вырезает из него только строку,
//! а создание частиц, движение и отрисовка выполняются в одном инстансированном GPU-проходе.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::layers::{LayerId, Order};
use egui::{Context, Id, Painter, Rect, Shape, vec2};
use egui_wgpu::{CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use mcrate_core::Track;

use crate::ui::anim::{STANDARD, cubic_bezier};

/// Полная длительность эффекта вместе с небольшим сдвигом старта частиц слева направо.
const EFFECT_DURATION: f64 = 0.94;
/// Время, за которое схлопывается место строки в списке.
const COLLAPSE_DURATION: f64 = 0.30;
/// Время жизни частицы в шейдере.
const PARTICLE_LIFETIME: f32 = 0.78;
/// Размер ячейки текстуры в физических пикселях.
const PARTICLE_CELL_PX: f32 = 1.0;
/// Ускорение вниз, логические точки в секунду².
const GRAVITY: f32 = 18.0;
/// Горизонтальная составляющая разлёта, логические точки в секунду.
const HORIZONTAL_SPREAD: f32 = 175.0;
/// Минимальный масштаб частицы перед исчезновением.
const PARTICLE_FINAL_SCALE: f32 = 0.58;
/// Идентификатор состояния эффекта в памяти egui.
const STATE_ID: &str = "thanos_snap_state";
/// Слой, в котором находятся GPU-частицы.
const PARTICLE_LAYER: &str = "thanos_snap_particles";
/// Сколько ждать снимок, прежде чем безопасно завершить удаление без анимации.
const CAPTURE_TIMEOUT: f64 = 1.5;

/// Тип коллекции, из которой удаляется трек.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemovalKind {
    /// Удалить лайк.
    Liked,
    /// Удалить постоянный аудиофайл и запись загрузки.
    Downloaded,
    /// Удалить трек из плейлиста с указанным ID.
    Playlist(u64),
}

/// Завершённое действие, которое UI передаёт контроллеру после анимации.
pub(crate) struct FinishedRemoval {
    /// Трек, который нужно убрать из коллекции.
    pub(crate) track: Track,
    /// Коллекция, из которой удаляется трек.
    pub(crate) kind: RemovalKind,
}

#[derive(Clone, Copy)]
struct RowPaintRange {
    /// Видимая часть строки с учётом клиппинга прокручиваемого списка.
    rect: Rect,
    /// Позиция строки в исходном списке на момент запроса удаления.
    index: usize,
    count: usize,
}

/// Изменение скругления соседней строки начинается сразу при запросе удаления,
/// ещё до того, как снимок строки готов и сам элемент исчез из данных.
struct SegmentTransition {
    row_id: Id,
    removed_index: usize,
    original_count: usize,
    started_at: f64,
}

/// Однажды вырезанное CPU-изображение строки, которое загружается в GPU-текстуру.
struct CapturedRow {
    rect: Rect,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Результат снимка: внешний `Option` означает «ещё не готово», внутренний — успех или отказ.
type CaptureSlot = Arc<Mutex<Option<Option<CapturedRow>>>>;

struct PendingCapture {
    row_id: Id,
    track: Track,
    kind: RemovalKind,
    requested_at: f64,
    result: CaptureSlot,
}

struct Effect {
    row_id: Id,
    track: Track,
    kind: RemovalKind,
    image: Arc<CapturedRow>,
    started_at: f64,
    gpu: Arc<Mutex<Option<GpuEffectState>>>,
}

#[derive(Default)]
struct State {
    rows: std::collections::HashMap<Id, RowPaintRange>,
    pending: Option<PendingCapture>,
    effects: Vec<Effect>,
    segment_transition: Option<SegmentTransition>,
}

fn state_id() -> Id {
    Id::unique(STATE_ID)
}

fn shared_state(ctx: &Context) -> Arc<Mutex<State>> {
    ctx.data_mut(|data| {
        data.get_temp::<Arc<Mutex<State>>>(state_id()).unwrap_or_else(|| {
            let state = Arc::new(Mutex::new(State::default()));
            data.insert_temp(state_id(), Arc::clone(&state));
            state
        })
    })
}

fn lock_state(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}

/// Ресурсы, общие для всех Thanos callbacks одного egui-wgpu renderer.
struct ThanosGpuResources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    output_srgb: bool,
}

impl ThanosGpuResources {
    fn new(render_state: &RenderState) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("thanos.wgsl"));
        let layout = Self::create_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mcrate_thanos_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = Self::create_pipeline(device, &shader, &pipeline_layout, render_state.target_format);
        let sampler = Self::create_sampler(device);
        Self { pipeline, layout, sampler, output_srgb: render_state.target_format.is_srgb() }
    }

    fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mcrate_thanos_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ParticleUniforms>() as u64),
                    },
                    count: None,
                },
            ],
        })
    }

    fn create_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        target_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mcrate_thanos_particles"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    fn create_sampler(device: &wgpu::Device) -> wgpu::Sampler {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mcrate_thanos_nearest_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        })
    }
}

/// Данные uniform-буфера. Раскладка совпадает со структурой `Params` в thanos.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ParticleUniforms {
    screen_size_points: [f32; 2],
    row_min_points: [f32; 2],
    image_size_px: [f32; 2],
    elapsed: f32,
    pixels_per_point: f32,
    particle_lifetime: f32,
    gravity: f32,
    horizontal_spread: f32,
    final_scale: f32,
    cell_size_px: f32,
    columns: u32,
    seed: u32,
    output_srgb: u32,
}

struct GpuEffectState {
    // TextureView удерживает ссылку на texture, но сама texture хранится явно для ясного lifetime.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    instance_count: u32,
}

/// Регистрирует графический проход в том же renderer, который рисует интерфейс.
pub(crate) fn init_gpu(render_state: &RenderState) {
    let resources = ThanosGpuResources::new(render_state);
    render_state.renderer.write().callback_resources.insert(resources);
}

/// Сбрасывает геометрию строк и активирует снимок, полученный после предыдущего кадра.
pub(crate) fn begin_frame(ctx: &Context) {
    let now = ctx.input(|input| input.time);
    let shared = shared_state(ctx);
    let mut state = lock_state(&shared);
    state.rows.clear();

    let ready = state.pending.as_ref().and_then(|pending| {
        pending.result.lock().unwrap_or_else(|error| error.into_inner()).take().map(|image| (pending.row_id, image))
    });
    let expired = state.pending.as_ref().is_some_and(|pending| now - pending.requested_at >= CAPTURE_TIMEOUT);

    if let Some((row_id, image)) = ready {
        let pending = state.pending.take().expect("готовый снимок принадлежит ожидающему удалению");
        match image {
            Some(image) => state.effects.push(Effect {
                row_id,
                track: pending.track,
                kind: pending.kind,
                image: Arc::new(image),
                started_at: now,
                gpu: Arc::default(),
            }),
            None => state.effects.push(Effect {
                row_id,
                track: pending.track,
                kind: pending.kind,
                image: Arc::new(empty_capture()),
                started_at: now - EFFECT_DURATION,
                gpu: Arc::default(),
            }),
        }
    } else if expired {
        let pending = state.pending.take().expect("просроченный снимок принадлежит ожидающему удалению");
        state.effects.push(Effect {
            row_id: pending.row_id,
            track: pending.track,
            kind: pending.kind,
            image: Arc::new(empty_capture()),
            started_at: now - EFFECT_DURATION,
            gpu: Arc::default(),
        });
    }
}

fn empty_capture() -> CapturedRow {
    CapturedRow { rect: Rect::NOTHING, width: 0, height: 0, rgba: Vec::new() }
}

/// Запоминает видимую область строки для однократного снимка при выборе удаления.
pub(crate) fn record_row(ctx: &Context, row_id: Id, rect: Rect, index: usize, count: usize) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let shared = shared_state(ctx);
    lock_state(&shared).rows.insert(row_id, RowPaintRange { rect, index, count });
}

/// Возвращает прогресс появления новых внешних углов у соседней строки.
/// Удаляемая строка не меняет собственные углы: её снимок должен сохранить исходный вид.
pub(crate) fn segment_corner_progress(
    ctx: &Context,
    row_id: Id,
    index: usize,
    now: f64,
) -> Option<(bool, bool, f32)> {
    let shared = shared_state(ctx);
    let state = lock_state(&shared);
    let transition = state.segment_transition.as_ref()?;
    if transition.row_id == row_id || transition.original_count < 2 {
        return None;
    }

    let reveal_top = transition.removed_index == 0 && index == 1;
    let reveal_bottom = transition.removed_index + 1 == transition.original_count
        && index + 1 == transition.removed_index;
    if !reveal_top && !reveal_bottom {
        return None;
    }

    let linear = ((now - transition.started_at) / COLLAPSE_DURATION).clamp(0.0, 1.0) as f32;
    let progress = cubic_bezier(STANDARD, linear).clamp(0.0, 1.0);
    Some((reveal_top, reveal_bottom, progress))
}

/// Запускает снимок строки. `false` означает, что анимация недоступна и трек надо удалить сразу.
pub(crate) fn start(ctx: &Context, row_id: Id, track: Track, kind: RemovalKind, now: f64, reduce_motion: bool) -> bool {
    if reduce_motion {
        return false;
    }
    let shared = shared_state(ctx);
    let mut state = lock_state(&shared);
    if state.effects.iter().any(|effect| effect.row_id == row_id)
        || state.pending.as_ref().is_some_and(|pending| pending.row_id == row_id)
    {
        return true;
    }
    if state.pending.is_some() || !state.effects.is_empty() {
        return false;
    }
    let Some(row) = state.rows.get(&row_id).copied() else { return false };
    state.segment_transition = Some(SegmentTransition {
        row_id,
        removed_index: row.index,
        original_count: row.count,
        started_at: now,
    });
    let pixels_per_point = ctx.pixels_per_point().max(0.1);
    let result: CaptureSlot = Arc::default();
    state.pending = Some(PendingCapture {
        row_id,
        track,
        kind,
        requested_at: now,
        result: Arc::clone(&result),
    });
    drop(state);

    // Снимок запрашивается после отрисовки следующего кадра, пока строка ещё видима.
    let repaint_ctx = ctx.clone();
    let viewport_rect = ctx.viewport_rect();
    ctx.request_screenshot(move |image| {
        let captured = crop_screenshot(&image, row.rect, viewport_rect, pixels_per_point);
        *result.lock().unwrap_or_else(|error| error.into_inner()) = Some(captured);
        repaint_ctx.request_repaint();
    });
    true
}

fn crop_screenshot(image: &egui::ColorImage, rect: Rect, viewport_rect: Rect, pixels_per_point: f32) -> Option<CapturedRow> {
    let image_width = image.size[0];
    let image_height = image.size[1];
    if image_width == 0 || image_height == 0 || pixels_per_point <= 0.0 {
        return None;
    }
    let x0 = ((rect.min.x - viewport_rect.min.x) * pixels_per_point).floor().max(0.0) as usize;
    let y0 = ((rect.min.y - viewport_rect.min.y) * pixels_per_point).floor().max(0.0) as usize;
    let x1 = ((rect.max.x - viewport_rect.min.x) * pixels_per_point).ceil().max(0.0) as usize;
    let y1 = ((rect.max.y - viewport_rect.min.y) * pixels_per_point).ceil().max(0.0) as usize;
    let x0 = x0.min(image_width);
    let y0 = y0.min(image_height);
    let x1 = x1.min(image_width);
    let y1 = y1.min(image_height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let width = (x1 - x0) as u32;
    let height = (y1 - y0) as u32;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in y0..y1 {
        let row_start = y * image_width;
        for x in x0..x1 {
            rgba.extend_from_slice(&image.pixels[row_start + x].to_array());
        }
    }
    let min = viewport_rect.min + vec2(x0 as f32 / pixels_per_point, y0 as f32 / pixels_per_point);
    let max = viewport_rect.min + vec2(x1 as f32 / pixels_per_point, y1 as f32 / pixels_per_point);
    Some(CapturedRow { rect: Rect::from_min_max(min, max), width, height, rgba })
}

/// Прогресс схлопывания строки (0..1). Пока эффект активен, обычное содержимое не рисуется.
pub(crate) fn row_progress(ctx: &Context, row_id: Id, now: f64) -> Option<f32> {
    let shared = shared_state(ctx);
    let state = lock_state(&shared);
    let effect = state.effects.iter().find(|effect| effect.row_id == row_id)?;
    let linear = ((now - effect.started_at) / COLLAPSE_DURATION).clamp(0.0, 1.0) as f32;
    Some(cubic_bezier(STANDARD, linear).clamp(0.0, 1.0))
}

/// Есть ли незавершённое удаление: второй эффект не запускается во время снимка или распада.
pub(crate) fn has_active_removal(ctx: &Context) -> bool {
    let shared = shared_state(ctx);
    let state = lock_state(&shared);
    state.pending.is_some() || !state.effects.is_empty()
}

/// Добавляет GPU callback поверх интерфейса и запрашивает следующий кадр, пока эффект активен.
pub(crate) fn paint(ctx: &Context, now: f64) {
    let shared = shared_state(ctx);
    let state = lock_state(&shared);
    if state.effects.is_empty() {
        let waiting_for_capture = state.pending.is_some();
        drop(state);
        if waiting_for_capture {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        return;
    }
    let layer = LayerId::new(Order::Middle, Id::unique(PARTICLE_LAYER));
    ctx.move_to_top(layer);
    let screen_rect = ctx.viewport_rect();
    let painter = Painter::new(ctx.clone(), layer, screen_rect);
    for effect in &state.effects {
        let elapsed = (now - effect.started_at) as f32;
        if effect.image.width == 0 || effect.image.height == 0 || elapsed >= EFFECT_DURATION as f32 {
            continue;
        }
        let callback = ThanosParticleCallback {
            image: Arc::clone(&effect.image),
            elapsed,
            gpu: Arc::clone(&effect.gpu),
            seed: seed_for(&effect.track),
            viewport_rect: screen_rect,
        };
        let callback = egui_wgpu::Callback::new_paint_callback(screen_rect, callback);
        painter.add(Shape::Callback(callback));
    }
    drop(state);
    ctx.request_repaint_after(Duration::from_millis(16));
}

/// Возвращает завершённые удаления и освобождает текстуры/снимки.
pub(crate) fn take_finished(ctx: &Context, now: f64) -> Vec<FinishedRemoval> {
    let shared = shared_state(ctx);
    let mut state = lock_state(&shared);
    let mut finished = Vec::new();
    let mut active = Vec::with_capacity(state.effects.len());
    for effect in std::mem::take(&mut state.effects) {
        if now - effect.started_at >= EFFECT_DURATION {
            finished.push(FinishedRemoval { track: effect.track, kind: effect.kind });
            state.rows.remove(&effect.row_id);
            if state.segment_transition.as_ref().is_some_and(|transition| transition.row_id == effect.row_id) {
                state.segment_transition = None;
            }
        } else {
            active.push(effect);
        }
    }
    state.effects = active;
    finished
}

struct ThanosParticleCallback {
    image: Arc<CapturedRow>,
    elapsed: f32,
    gpu: Arc<Mutex<Option<GpuEffectState>>>,
    seed: u32,
    viewport_rect: Rect,
}

impl CallbackTrait for ThanosParticleCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(shared_gpu) = resources.get::<ThanosGpuResources>() else { return Vec::new() };
        let mut gpu = self.gpu.lock().unwrap_or_else(|error| error.into_inner());
        if gpu.is_none() {
            *gpu = Some(create_gpu_effect_state(device, queue, shared_gpu, &self.image));
        }
        if let Some(effect) = gpu.as_ref() {
            self.write_uniforms(queue, screen, shared_gpu, effect);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &CallbackResources) {
        let Some(shared_gpu) = resources.get::<ThanosGpuResources>() else { return };
        let gpu = self.gpu.lock().unwrap_or_else(|error| error.into_inner());
        let Some(effect) = gpu.as_ref() else { return };
        pass.set_pipeline(&shared_gpu.pipeline);
        pass.set_bind_group(0, &effect.bind_group, &[]);
        pass.draw(0..6, 0..effect.instance_count);
    }
}

/// Создаёт текстуру снимка, uniform-буфер и единственную группу привязок для эффекта.
fn create_gpu_effect_state(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shared_gpu: &ThanosGpuResources,
    image: &CapturedRow,
) -> GpuEffectState {
    let size = wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mcrate_thanos_row_snapshot"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &image.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width * 4),
            rows_per_image: Some(image.height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mcrate_thanos_particle_uniforms"),
        size: std::mem::size_of::<ParticleUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mcrate_thanos_effect_bind_group"),
        layout: &shared_gpu.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&shared_gpu.sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniforms.as_entire_binding() },
        ],
    });
    let columns = (image.width as f32 / PARTICLE_CELL_PX).ceil().max(1.0) as u32;
    let rows = (image.height as f32 / PARTICLE_CELL_PX).ceil().max(1.0) as u32;
    GpuEffectState { _texture: texture, bind_group, uniforms, instance_count: columns.saturating_mul(rows) }
}

impl ThanosParticleCallback {
    /// Обновляет только время и параметры viewport; сетка и случайные параметры не передаются с CPU.
    fn write_uniforms(
        &self,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        shared_gpu: &ThanosGpuResources,
        effect: &GpuEffectState,
    ) {
        let pixels_per_point = screen.pixels_per_point.max(0.1);
        let screen_size_points = [self.viewport_rect.width(), self.viewport_rect.height()];
        let columns = (self.image.width as f32 / PARTICLE_CELL_PX).ceil().max(1.0) as u32;
        let uniforms = ParticleUniforms {
            screen_size_points,
            row_min_points: [
                self.image.rect.min.x - self.viewport_rect.min.x,
                self.image.rect.min.y - self.viewport_rect.min.y,
            ],
            image_size_px: [self.image.width as f32, self.image.height as f32],
            elapsed: self.elapsed,
            pixels_per_point,
            particle_lifetime: PARTICLE_LIFETIME,
            gravity: GRAVITY,
            horizontal_spread: HORIZONTAL_SPREAD,
            final_scale: PARTICLE_FINAL_SCALE,
            cell_size_px: PARTICLE_CELL_PX,
            columns,
            seed: self.seed,
            output_srgb: u32::from(shared_gpu.output_srgb),
        };
        queue.write_buffer(&effect.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }
}

fn seed_for(track: &Track) -> u32 {
    let mut seed = 0x811c_9dc5_u32;
    for byte in track.provider.key().bytes().chain(track.id.bytes()) {
        seed = (seed ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    seed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_uniform_layout_matches_wgsl_alignment() {
        assert_eq!(std::mem::size_of::<ParticleUniforms>(), 64);
    }

    #[test]
    fn particle_grid_is_derived_from_texture_dimensions() {
        let columns = (800.0_f32 / PARTICLE_CELL_PX).ceil() as u32;
        let rows = (64.0_f32 / PARTICLE_CELL_PX).ceil() as u32;
        assert!(columns > 200);
        assert!(rows > 20);
        assert!(columns.saturating_mul(rows) > 4_000);
    }
}

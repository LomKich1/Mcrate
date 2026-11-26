struct Params {
    screen_size_points: vec2<f32>,
    row_min_points: vec2<f32>,
    image_size_px: vec2<f32>,
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
};

@group(0) @binding(0) var row_texture: texture_2d<f32>;
@group(0) @binding(1) var row_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
};

fn hash_u32(value: u32) -> u32 {
    var x = value;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn random01(instance: u32, salt: u32) -> f32 {
    let value = hash_u32(params.seed ^ (instance * 0x9e3779b9u) ^ salt);
    return f32(value) / 4294967295.0;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    var output: VertexOutput;

    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex_index];

    let columns = max(params.columns, 1u);
    let column = instance_index % columns;
    let row = instance_index / columns;
    let cell = vec2<f32>(params.cell_size_px, params.cell_size_px);
    let source_min = vec2<f32>(f32(column), f32(row)) * cell;
    let source_size = min(cell, params.image_size_px - source_min);
    let uv = (source_min + corner * source_size) / params.image_size_px;

    let random_x = random01(instance_index, 0x13579bdfu);
    let random_y = random01(instance_index, 0x2468ace0u);
    let random_delay = random01(instance_index, 0xa511e9b3u);
    let random_rotation = random01(instance_index, 0x63d83595u);
    let delay = (f32(column) / f32(columns)) * 0.12 + random_delay * 0.035;
    let age = params.elapsed - delay;
    let lifetime = params.particle_lifetime;

    output.uv = uv;
    if age < 0.0 || age >= lifetime || source_size.x <= 0.0 || source_size.y <= 0.0 {
        output.position = vec4<f32>(2.0, 2.0, 0.0, 1.0);
        output.fade = 0.0;
        return output;
    }

    let fade = pow(max(1.0 - age / lifetime, 0.0), 1.25);
    let scale = params.final_scale + (1.0 - params.final_scale) * fade;
    let normalized_x = (source_min.x + source_size.x * 0.5) / params.image_size_px.x - 0.5;
    let velocity_x = normalized_x * params.horizontal_spread + (random_x - 0.5) * 95.0;
    let velocity_y = -18.0 - random_y * 92.0;
    let offset = vec2<f32>(velocity_x * age, velocity_y * age + 0.5 * params.gravity * age * age);
    let center = params.row_min_points + (source_min + source_size * 0.5) / params.pixels_per_point + offset;
    let local = (corner - vec2<f32>(0.5, 0.5)) * (source_size / params.pixels_per_point) * scale;
    let rotation = (random_rotation - 0.5) * 3.6 * age;
    let cosine = cos(rotation);
    let sine = sin(rotation);
    let rotated = vec2<f32>(local.x * cosine - local.y * sine, local.x * sine + local.y * cosine);
    let screen_position = center + rotated;
    let ndc = vec2<f32>(
        screen_position.x / params.screen_size_points.x * 2.0 - 1.0,
        1.0 - screen_position.y / params.screen_size_points.y * 2.0,
    );

    output.position = vec4<f32>(ndc, 0.0, 1.0);
    output.fade = fade;
    return output;
}

fn srgb_to_linear(channel: vec3<f32>) -> vec3<f32> {
    let low = channel / 12.92;
    let high = pow((channel + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(low, high, channel > vec3<f32>(0.04045));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(row_texture, row_sampler, input.uv);
    if params.output_srgb != 0u {
        color = vec4<f32>(srgb_to_linear(color.rgb), color.a);
    }
    return color * input.fade;
}

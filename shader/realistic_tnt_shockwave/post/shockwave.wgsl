// Realistic TNT: fireball, mushroom cloud, dust skirt, flash and shockwave (MIT).
//
// The Realistic TNT native mod hands this pass up to three blasts in `frame.mods`
// (`native_shader_params`, eight vec4s). Slot layout, two vec4s per blast k = 0, 1, 2:
//
//   frame.mods[2k]     = (x, y, z, age)       where it went off (world position, the ground
//                                             under it) and its age in seconds
//   frame.mods[2k + 1] = (size, seed, grounded, on)
//                                             its size in blocks (about 4.5 for one TNT), a
//                                             number of its own in 0..1 so that no two clouds
//                                             look alike, 1 when it went off on the ground (0
//                                             in the air: no dust), and 1 while it is live
//
// Everything is zero when the mod sets nothing, which this pass reads as "no blast": the
// picture is returned as it came.
//
// What is drawn, in the order it happens:
//   0.00 s  a flash over the whole picture, and the fireball's light on the land round it
//   0.00 s  a shockwave: a sphere of bent air racing out, seen as a ring, with colour fringes
//   0.05 s  the fireball: white-hot, then yellow, orange and red as it cools and rises
//   0.20 s  a skirt of dust racing out along the ground
//   0.50 s  the fireball rolls up into a mushroom cloud of black smoke on a stem
//   6-12 s  the smoke greys, thins and fades
// The clouds are ray marched against the world's depth, so hills and walls hide them.

// How bright the flash is; 0 turns it off (photosensitivity).
override tnt_flash: f32 = 1.0;
// How hard the shockwave bends the picture; 0 turns it off.
override tnt_distortion: f32 = 1.0;
// How finely the smoke is drawn: 0 fast, 1 balanced, 2 fine.
override tnt_detail: u32 = 1u;

// Blocks a second. The mod times its sound and camera shake to the same speed.
const TNT_SHOCK_SPEED: f32 = 42.0;
// The size of one TNT's blast. A bigger blast lives slower and its front runs faster, by the
// square root of how much bigger it is (`tnt_scale`); the mod keeps to the same clock.
const TNT_BASE_SIZE: f32 = 4.8;
// Seconds a blast is drawn for.
const TNT_LIFE: f32 = 12.0;

struct TntBlast {
    // Where it went off, relative to the camera.
    c: vec3<f32>,
    // Seconds since.
    // Seconds since, on the blast's own clock (slower for a bigger one: `tnt_scale`).
    t: f32,
    // Seconds since, as they are.
    age: f32,
    // Its size, in blocks.
    s: f32,
    seed: f32,
    // 1 when it went off on the ground (it raises dust), 0 in the air.
    grounded: f32,
}

// The mushroom's measures at the blast's age.
struct TntCloud {
    // Height of the cap's middle over the ground.
    yc: f32,
    // The cap's radius.
    rc: f32,
    // The stem's radius.
    rs: f32,
    // 0 as it goes off, 1 a moment later.
    grow: f32,
}

// 1 below `lo`, 0 above `hi`.
fn tnt_fall(lo: f32, hi: f32, x: f32) -> f32 {
    return 1.0 - smoothstep(lo, hi, x);
}

fn tnt_hash(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q = q + vec3<f32>(dot(q, q.yxz + vec3<f32>(33.33)));
    return fract((q.x + q.y) * q.z);
}

fn tnt_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec3<f32>(3.0) - 2.0 * f);
    let x00 = mix(tnt_hash(i), tnt_hash(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(tnt_hash(i + vec3<f32>(0.0, 1.0, 0.0)), tnt_hash(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(tnt_hash(i + vec3<f32>(0.0, 0.0, 1.0)), tnt_hash(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(tnt_hash(i + vec3<f32>(0.0, 1.0, 1.0)), tnt_hash(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// `octaves` octaves of noise, 0..1.
fn tnt_fbm(p: vec3<f32>, octaves: i32) -> f32 {
    var q = p;
    var amp = 0.5;
    var sum = 0.0;
    var total = 0.0;
    for (var i = 0; i < octaves; i = i + 1) {
        sum = sum + amp * tnt_noise(q);
        total = total + amp;
        q = q * 2.03 + vec3<f32>(17.1, 3.7, 9.2);
        amp = amp * 0.5;
    }
    return sum / total;
}

// Where the eye really is, relative to `frame.camera_position`, in blocks. View bobbing, a
// camera shake and nausea move it a few hundredths of a block from there, which `frame.projection`
// holds; the near plane is only 0.05 blocks away, so a ray taken from the camera position through
// a pixel's near-plane point would swing wildly with every step. The eye is the point a
// perspective projection sends to (0, 0, z, 0): `projection_inverse` brings it back.
fn tnt_eye() -> vec3<f32> {
    let e = frame.projection_inverse * vec4<f32>(0.0, 0.0, 1.0, 0.0);
    if abs(e.w) < 1e-6 {
        return vec3<f32>(0.0);
    }
    return (frame.view_inverse * vec4<f32>(e.xyz / e.w, 0.0)).xyz;
}

// Blast `k` of `frame.mods`, its position taken from the eye (`eye`, from `tnt_eye`).
// How much slower a blast of `size` lives than one TNT's, and how much faster its front runs.
fn tnt_scale(size: f32) -> f32 {
    return sqrt(max(size / TNT_BASE_SIZE, 1.0));
}

fn tnt_blast(k: i32, eye: vec3<f32>) -> TntBlast {
    let a = frame.mods[2 * k];
    let p = frame.mods[2 * k + 1];
    var b: TntBlast;
    b.c = a.xyz - frame.camera_position - eye;
    b.age = a.w;
    b.t = a.w / tnt_scale(p.x);
    b.s = p.x;
    b.seed = p.y;
    b.grounded = p.z;
    return b;
}

fn tnt_live(k: i32) -> bool {
    let a = frame.mods[2 * k];
    let p = frame.mods[2 * k + 1];
    return p.w > 0.5 && p.x > 0.0 && a.w >= 0.0 && a.w < TNT_LIFE * tnt_scale(p.x);
}

fn tnt_cloud(b: TntBlast) -> TntCloud {
    let rise = 1.0 - exp(-b.t / 1.6);
    var cl: TntCloud;
    cl.grow = 1.0 - exp(-b.t / 0.2);
    cl.yc = b.s * (0.4 + 1.5 * rise);
    cl.rc = b.s * (0.3 + 0.75 * cl.grow) * (1.0 + 0.06 * b.t);
    cl.rs = b.s * (0.22 + 0.12 * cl.grow) * (1.0 + 0.05 * b.t);
    return cl;
}

// The cloud at `q` (relative to where the blast went off): how thick it is, how hot, and the
// noise that shaped it there. `octaves` of noise: fewer for the shadow probe.
fn tnt_shape(q: vec3<f32>, b: TntBlast, cl: TntCloud, octaves: i32) -> vec3<f32> {
    // The noise drifts up through the cloud, so it seems to roll. It puffs the cloud out where
    // it is high and pinches it in between: billows, with a crisp edge.
    let n = tnt_fbm(q * (2.0 / b.s) + vec3<f32>(b.seed * 7.3, -b.t * 0.55, b.seed * 3.1), octaves);
    let puff = (n - 0.5) * 1.4;
    // The cap: a flattened ball.
    let d = q - vec3<f32>(0.0, cl.yc, 0.0);
    let e = length(vec3<f32>(d.x, d.y * 1.25, d.z)) / cl.rc;
    let cap = tnt_fall(0.8, 1.0, e + puff);
    // The stem under it, wide and ragged.
    let rr = length(q.xz);
    let span = smoothstep(-0.4, 0.5, q.y) * tnt_fall(cl.yc - cl.rc * 0.4, cl.yc + cl.rc * 0.1, q.y);
    let stem = tnt_fall(cl.rs * 0.8, cl.rs * 1.4, rr + puff * cl.rs * 1.8) * span * 0.8;
    // What stays low where it went off: dust and smoke round the crater.
    let br = b.s * (0.5 + 0.7 * cl.grow);
    let be = length(vec3<f32>(q.x, q.y * 1.8, q.z)) / br;
    let low = tnt_fall(0.75, 1.0, be + puff) * 0.75 * exp(-b.t / 5.0);
    let density = max(max(cap, stem), low);
    // The middle of the cap is the hottest. As it cools the fire breaks up into patches that
    // glow through the smoke for a moment.
    let heart = tnt_fall(0.1, 1.05, e + puff * 0.7);
    let patches = smoothstep(b.t * 0.22 - 0.1, b.t * 0.22 + 0.3, n);
    let heat = (heart * 2.4 * exp(-b.t / 1.15) * patches + stem * exp(-b.t / 0.9) + low * 1.4 * exp(-b.t / 0.6))
        * (0.5 + n);
    return vec3<f32>(density, heat, n);
}

// The colour fire glows with at `h`: nothing, red, orange, yellow, white.
fn tnt_fire(h: f32) -> vec3<f32> {
    let x = clamp(h, 0.0, 1.8);
    return vec3<f32>(1.0, 0.22, 0.03) * smoothstep(0.04, 0.45, x)
        + vec3<f32>(0.75, 0.5, 0.08) * smoothstep(0.3, 0.85, x)
        + vec3<f32>(0.7, 0.75, 0.65) * smoothstep(0.75, 1.5, x);
}

// How much daylight there is, 0.06 (night) to 1.
fn tnt_daylight() -> f32 {
    return clamp(frame.sun_direction.y * 4.0 + 0.35, 0.06, 1.0);
}

// The fireball and its smoke along the view ray `dir`, up to `max_dist` blocks (the world's
// depth there): the colour to add and how much of the picture behind it is hidden.
fn tnt_march(dir: vec3<f32>, max_dist: f32, b: TntBlast, jitter: f32) -> vec4<f32> {
    let cl = tnt_cloud(b);
    // A ball round the whole cloud: only the part of the ray inside it is walked.
    let crown = cl.yc + cl.rc * 0.85;
    let radius = max(crown * 0.5, cl.rc * 1.2) + b.s * 0.6;
    let centre = b.c + vec3<f32>(0.0, crown * 0.5, 0.0);
    let along = dot(centre, dir);
    let h = along * along - dot(centre, centre) + radius * radius;
    if h <= 0.0 {
        return vec4<f32>(0.0);
    }
    let reach = sqrt(h);
    let t0 = max(along - reach, 0.0);
    let t1 = min(along + reach, max_dist);
    if t1 <= t0 {
        return vec4<f32>(0.0);
    }
    let count = 16 + 10 * i32(tnt_detail);
    let dt = (t1 - t0) / f32(count);
    // It thins as it spreads, and goes at the end of its life.
    let sigma = 1.5 / (1.0 + b.t * 0.3) * tnt_fall(TNT_LIFE * 0.45, TNT_LIFE, b.t);
    let day = tnt_daylight();
    // Sooty brown at first, a dirty grey as the soot settles out.
    let soot = mix(vec3<f32>(0.17, 0.14, 0.11), vec3<f32>(0.36, 0.34, 0.31), smoothstep(1.0, 8.0, b.t));
    let sun = vec3<f32>(1.0, 0.94, 0.84) * (0.95 * day);
    let sky = (frame.sky_color * 0.22 + vec3<f32>(0.05, 0.045, 0.04)) * day + vec3<f32>(0.012);
    let ember = vec3<f32>(1.0, 0.42, 0.12) * (exp(-b.t / 2.2) * mix(0.75, 0.3, day));
    let toward = frame.light_direction * (b.s * 0.3);
    var trans = 1.0;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < count; i = i + 1) {
        let q = dir * (t0 + (f32(i) + jitter) * dt) - b.c;
        let shape = tnt_shape(q, b, cl, 4);
        if shape.x > 0.01 {
            let a = 1.0 - exp(-shape.x * sigma * dt);
            // A step toward the sun (or the moon): thick smoke that way leaves this spot in
            // the cloud's own shadow.
            let lit = exp(-tnt_shape(q + toward, b, cl, 2).x * 2.6);
            // The fire lights the smoke from below.
            let under = 1.0 - clamp(q.y / (cl.yc + cl.rc), 0.0, 1.0);
            let c = soot * (sky + sun * lit) * (0.75 + 0.5 * shape.z) + ember * under + tnt_fire(shape.y) * 2.2;
            colour = colour + trans * a * c;
            trans = trans * (1.0 - a);
            if trans < 0.03 {
                break;
            }
        }
    }
    return vec4<f32>(colour, 1.0 - trans);
}

// The dust the blast drives out along the ground: a low, clumpy ring that races out and settles.
fn tnt_dust(dir: vec3<f32>, max_dist: f32, b: TntBlast, jitter: f32) -> vec4<f32> {
    let fade = exp(-b.t / 1.8) * smoothstep(0.0, 0.1, b.t);
    if fade < 0.02 {
        return vec4<f32>(0.0);
    }
    let spread = 1.0 - exp(-b.t / 0.7);
    let ring = b.s * (0.5 + 2.2 * spread);
    let width = b.s * (0.25 + 0.35 * spread);
    let height = 0.4 + b.s * 0.28 * (1.0 - exp(-b.t / 0.5));
    // The part of the ray between the ground and the dust's top...
    let y0 = b.c.y - 0.4;
    let y1 = b.c.y + height * 1.5;
    var t0 = 0.0;
    var t1 = max_dist;
    if abs(dir.y) > 0.0001 {
        let ta = y0 / dir.y;
        let tb = y1 / dir.y;
        t0 = max(t0, min(ta, tb));
        t1 = min(t1, max(ta, tb));
    } else if y0 > 0.0 || y1 < 0.0 {
        return vec4<f32>(0.0);
    }
    // ...and inside the ring's outer edge.
    let outer = ring + width * 2.0;
    let o = -b.c.xz;
    let fa = dot(dir.xz, dir.xz);
    if fa > 0.000001 {
        let fb = dot(o, dir.xz);
        let disc = fb * fb - fa * (dot(o, o) - outer * outer);
        if disc <= 0.0 {
            return vec4<f32>(0.0);
        }
        let root = sqrt(disc);
        t0 = max(t0, (-fb - root) / fa);
        t1 = min(t1, (-fb + root) / fa);
    } else if dot(o, o) > outer * outer {
        return vec4<f32>(0.0);
    }
    if t1 <= t0 {
        return vec4<f32>(0.0);
    }
    let dt = (t1 - t0) / 10.0;
    let day = tnt_daylight();
    let tone = vec3<f32>(0.52, 0.47, 0.4) * (0.3 + 0.8 * day)
        + vec3<f32>(1.0, 0.45, 0.12) * exp(-b.t / 0.5) * 0.5;
    var trans = 1.0;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < 10; i = i + 1) {
        let q = dir * (t0 + (f32(i) + jitter) * dt) - b.c;
        if q.y > -0.5 {
            let rr = length(q.xz);
            let n = tnt_fbm(vec3<f32>(q.x, q.y * 2.0, q.z) * (3.0 / b.s)
                + vec3<f32>(b.seed * 5.0, -b.t * 0.8, b.seed * 2.0), 3);
            let g = (rr - ring) / width;
            // A wall at the ring, in clumps, and thinner dust left behind it.
            let wall = exp(-g * g) * smoothstep(0.3, 0.65, n) * 1.6 + 0.25 * tnt_fall(0.0, ring, rr);
            let lid = tnt_fall(0.0, height * (0.4 + n), q.y);
            let a = 1.0 - exp(-wall * lid * (0.35 + n) * 0.6 * fade * dt);
            colour = colour + trans * a * tone * (0.7 + 0.6 * n);
            trans = trans * (1.0 - a);
        }
    }
    return vec4<f32>(colour, 1.0 - trans);
}

// How the shockwave bends the picture at `uv`: (offset.x, offset.y, crest). The offset is
// how far to pull the picture, in uv units. `scene_dist` is how far the world is along
// `dir`, and `ground` whether there is any (not the sky).
fn tnt_shock(uv: vec2<f32>, dir: vec3<f32>, scene_dist: f32, ground: bool, b: TntBlast, eye: vec3<f32>) -> vec3<f32> {
    let front = b.age * TNT_SHOCK_SPEED * tnt_scale(b.s);
    let range = max(b.s * 16.0, 40.0);
    if front >= range {
        return vec3<f32>(0.0);
    }
    let centre = b.c + vec3<f32>(0.0, 0.5, 0.0);
    // Where the blast sits on screen: the picture is pulled away from there. A blast behind
    // the camera has no place on screen, so it bends nothing.
    let clip = frame.projection * (frame.view * vec4<f32>(centre + eye, 1.0));
    if clip.w <= 0.001 {
        return vec3<f32>(0.0);
    }
    let ndc = clip.xy / clip.w;
    let origin = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let aspect = frame.resolution.x / frame.resolution.y;
    let away = (uv - origin) * vec2<f32>(aspect, 1.0);
    let outward = away / max(length(away), 0.0001);
    var band = 0.0;
    // The edge of the sphere of bent air, seen from outside it: the rays that graze it.
    let along = dot(centre, dir);
    if along > 0.0 && along < scene_dist + 1.0 && length(centre) > front {
        let g = (length(centre - dir * along) - front) / max(0.5, front * 0.04);
        band = exp(-g * g);
    }
    // Where it crosses the land.
    if ground {
        let g = (length(dir * scene_dist - centre) - front) / max(1.0, front * 0.08);
        band = max(band, 0.8 * exp(-g * g));
    }
    // As the front comes up to the camera it would fill the whole picture: it fades out first,
    // and is gone by the time it passes.
    let ahead = length(centre) - front;
    let life = 1.0 - front / range;
    band = band * life * life * smoothstep(2.0, 12.0, ahead);
    let pull = band * clamp(b.s / 4.5, 0.5, 1.6) * 0.03 * tnt_distortion;
    return vec3<f32>(outward * vec2<f32>(1.0 / aspect, 1.0) * pull, band);
}

fn post(input: PostInput) -> vec4<f32> {
    var busy = false;
    for (var k = 0; k < 3; k = k + 1) {
        if tnt_live(k) {
            busy = true;
        }
    }
    if !busy {
        return input.color;
    }
    // Rays start at the eye, not at the camera position (`tnt_eye`).
    let eye = tnt_eye();
    let dir = normalize(view_position(input.uv, 1.0) - eye);
    let ground = input.depth > 0.0;
    // The sky is as far as anything: a huge cloud stands farther out than the far plane.
    var scene_dist = 1.0e6;
    if ground {
        scene_dist = length(view_position(input.uv, input.depth) - eye);
    }

    // 1. The shockwaves bend the picture, the colours a little apart (as a lens does).
    var offset = vec2<f32>(0.0);
    var crest = 0.0;
    for (var k = 0; k < 3; k = k + 1) {
        if tnt_live(k) {
            let bend = tnt_shock(input.uv, dir, scene_dist, ground, tnt_blast(k, eye), eye);
            offset = offset + bend.xy;
            crest = crest + bend.z;
        }
    }
    var colour = input.color.rgb;
    if crest > 0.001 {
        let lo = vec2<f32>(0.001);
        let hi = vec2<f32>(0.999);
        let red = textureSampleLevel(previous, linear_sampler, clamp(input.uv - offset * 0.9, lo, hi), 0.0).r;
        let green = textureSampleLevel(previous, linear_sampler, clamp(input.uv - offset, lo, hi), 0.0).g;
        let blue = textureSampleLevel(previous, linear_sampler, clamp(input.uv - offset * 1.1, lo, hi), 0.0).b;
        colour = vec3<f32>(red, green, blue) + vec3<f32>(0.9, 0.85, 0.7) * (crest * 0.05);
    }

    // A pattern over the pixels that hides the steps of the clouds' marching.
    let jitter = fract(52.9829189 * fract(dot(input.pixel, vec2<f32>(0.06711056, 0.00583715))));

    for (var k = 0; k < 3; k = k + 1) {
        if tnt_live(k) {
            let b = tnt_blast(k, eye);
            let cl = tnt_cloud(b);
            let fire = b.c + vec3<f32>(0.0, cl.yc, 0.0);
            let glow = exp(-b.t / 0.7);

            // 2. The fireball lights the land round it, and the flash whitens the picture.
            if ground {
                let to_fire = dir * scene_dist - fire;
                let l = glow * 1.4 * b.s * b.s / (b.s * b.s + dot(to_fire, to_fire));
                colour = colour * (vec3<f32>(1.0) + vec3<f32>(1.6, 0.9, 0.35) * l)
                    + vec3<f32>(1.0, 0.5, 0.15) * (l * 0.05);
            }
            let far = length(b.c) / (b.s * 12.0);
            let flash = tnt_flash * exp(-b.t / 0.07) / (1.0 + far * far);
            colour = colour * (1.0 + 1.3 * flash) + vec3<f32>(1.0, 0.95, 0.85) * (0.4 * flash);

            // 3. The dust along the ground, then the fireball and its smoke over it.
            if b.grounded > 0.5 {
                let dust = tnt_dust(dir, scene_dist, b, jitter);
                colour = colour * (1.0 - dust.a) + dust.rgb;
            }
            let cloud = tnt_march(dir, scene_dist, b, jitter);
            colour = colour * (1.0 - cloud.a) + cloud.rgb;

            // 4. The air round the fireball glows.
            let along = dot(fire, dir);
            if along > 0.0 {
                let near = length(fire - dir * min(along, scene_dist));
                let r = cl.rc * 1.1;
                let halo = glow * (1.0 - exp(-b.t / 0.03)) * 0.5 / (1.0 + near * near / (r * r));
                colour = colour + vec3<f32>(1.0, 0.55, 0.18) * (halo * halo * 1.6 + halo * 0.1);
            }
        }
    }
    return vec4<f32>(max(colour, vec3<f32>(0.0)), input.color.a);
}

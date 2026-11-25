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
    // Cross-section rotation and transition from the young fireball to the vortex.
    roll: vec2<f32>,
    vortex: f32,
}

// Shader API 0.2 hands this pass every colour in linear light. The pass's colours were chosen on
// display values, so it works on the picture encoded with a 2.2 power and decodes what it returns.
fn tnt_encode(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}

fn tnt_decode(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
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
    let angle = b.t * 1.5 / (1.0 + 0.08 * b.t);
    cl.roll = vec2<f32>(cos(angle), sin(angle));
    cl.vortex = smoothstep(0.25, 1.25, b.t);
    return cl;
}

// The cloud at `q` (relative to where the blast went off): how thick it is, how hot, and the
// noise that shaped it there. `octaves` of noise: fewer for the shadow probe.
fn tnt_shape(q: vec3<f32>, b: TntBlast, cl: TntCloud, octaves: i32) -> vec3<f32> {
    let rr = length(q.xz);
    let dy = q.y - cl.yc;
    let core = cl.rc * 0.59;
    let tube = vec2<f32>(rr - core, dy);
    let ball = length(vec2<f32>(rr, dy * 1.25)) / cl.rc;
    let torus = length(tube * vec2<f32>(1.0, 1.08)) / (cl.rc * 0.49);
    let e = mix(ball, torus, cl.vortex);

    // Trace the noise backwards around the tube: billows rise on the inside,
    // turn over the crown, and descend at the outer edge.
    let turned = vec2<f32>(dot(tube, vec2<f32>(cl.roll.x, -cl.roll.y)), dot(tube, cl.roll.yx));
    let unit = q.xz / max(rr, 0.001 * b.s);
    // Half the roll: a full turn drags the billows into streaks.
    let weight = 0.5 * cl.vortex * tnt_fall(1.0, 1.65, torus) * smoothstep(0.03, 0.2, rr / cl.rc);
    let shift = (turned - tube) * weight;
    var sample_pos = q + vec3<f32>(unit.x * shift.x, shift.y, unit.y * shift.x);
    // A slow warp of the noise, so the billows are lumps of different sizes, not a grid of blobs.
    let warp = tnt_noise(q * (0.9 / b.s) + vec3<f32>(b.seed * 4.1, -b.t * 0.2, 1.7)) - 0.5;
    sample_pos = sample_pos + vec3<f32>(warp, -warp * 0.5, warp) * (b.s * 0.35);
    let n = tnt_fbm(sample_pos * (2.0 / b.s) + vec3<f32>(b.seed * 7.3, -b.t * 0.55, b.seed * 3.1), octaves);
    // Billows: rounded tops where the noise peaks, as cumulus has.
    let billow = 1.0 - abs(2.0 * n - 1.0);
    let puff = (mix(n, billow, 0.45) - 0.5) * 1.4;
    // A soft edge that frays into the air, thicker inside.
    let cap = tnt_fall(0.55, 1.05, e + puff) * (0.6 + 0.6 * n);
    // The stem under it, wide and ragged.
    let span = smoothstep(-0.4, 0.5, q.y) * tnt_fall(cl.yc - cl.rc * 0.4, cl.yc + cl.rc * 0.1, q.y);
    let stem = tnt_fall(cl.rs * 0.8, cl.rs * 1.4, rr + puff * cl.rs * 1.8) * span * 0.8;
    // What stays low where it went off: dust and smoke round the crater.
    let br = b.s * (0.5 + 0.7 * cl.grow);
    let be = length(vec3<f32>(q.x, q.y * 1.8, q.z)) / br;
    let low = tnt_fall(0.75, 1.0, be + puff) * 0.75 * exp(-b.t / 5.0);
    let density = max(max(cap, stem), low);
    // The middle of the cap is the hottest. As it cools the fire breaks up into patches that
    // glow through the smoke for a moment.
    // Fire lives on real seconds (a little longer for a bigger blast), not on the slow clock the
    // cloud rolls on: a big cloud burns out into black smoke instead of staying orange.
    let burn = mix(b.t, b.age, 0.65);
    let heart = tnt_fall(0.1, 1.05, e + puff * 0.7);
    let patches = smoothstep(burn * 0.22 - 0.1, burn * 0.22 + 0.3, n);
    let heat = (heart * 2.4 * exp(-burn / 1.15) * patches + stem * exp(-burn / 0.9) + low * 1.4 * exp(-burn / 0.6))
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
    let count = 20 + 12 * i32(tnt_detail);
    let dt = (t1 - t0) / f32(count);
    // It thins as it spreads, and goes at the end of its life.
    let sigma = 1.5 / (1.0 + b.t * 0.3) * tnt_fall(TNT_LIFE * 0.45, TNT_LIFE, b.t);
    let day = tnt_daylight();
    // Sooty brown at first, a dirty grey as the soot settles out.
    let soot = mix(vec3<f32>(0.17, 0.14, 0.11), vec3<f32>(0.36, 0.34, 0.31), smoothstep(1.0, 8.0, b.t));
    let sun = vec3<f32>(1.0, 0.94, 0.84) * (0.95 * day);
    let sky = (tnt_encode(frame.sky_color) * 0.22 +vec3<f32>(0.05, 0.045, 0.04)) * day + vec3<f32>(0.012);
    // Only sunlight behind the smoke produces a bright forward-scattering rim.
    let backlight = smoothstep(0.35, 0.92, dot(dir, frame.sun_direction))
        * smoothstep(-0.04, 0.15, frame.sun_direction.y);
    let embers = vec3<f32>(1.0, 0.36, 0.08) * exp(-b.t / 2.2) * mix(0.85, 0.58, day);
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
            // Reuse the sun probe: dense interior samples stay in shadow.
            let rim = backlight * lit * (1.0 - 0.7 * shape.x);
            let under = tnt_fall(-0.4, 0.22, (q.y - cl.yc) / cl.rc);
            let below = under / (1.0 + dot(q.xz, q.xz) / (cl.rc * cl.rc));
            let c = soot * (sky + sun * lit) * (0.75 + 0.5 * shape.z)
                + vec3<f32>(1.0, 0.91, 0.71) * rim * 0.4
                + embers * below + tnt_fire(shape.y) * 2.2;
            colour = colour + trans * a * c;
            trans = trans * (1.0 - a);
            if trans < 0.03 {
                break;
            }
        }
    }
    return vec4<f32>(colour, 1.0 - trans);
}

// The cold vapor shell is visible mainly at grazing angles. Its lifetime is in
// real seconds even when the rolling cloud runs on a slower clock.
fn tnt_vapor(dir: vec3<f32>, scene_dist: f32, b: TntBlast) -> f32 {
    if b.age >= 1.65 {
        return 0.0;
    }
    let centre = b.c + vec3<f32>(0.0, b.s * 0.24, 0.0);
    let radius = b.s * (0.18 + 2.2 * (1.0 - exp(-2.3 * b.age)));
    let along = dot(centre, dir);
    let radial = dir * along - centre;
    let edge = (length(radial) - radius) / (0.075 * radius + 0.025 * b.s);
    if along <= 0.0 || along >= scene_dist || abs(edge) > 2.0 {
        return 0.0;
    }
    let n = tnt_noise(radial * (4.0 / radius) + vec3<f32>(b.seed * 9.0, -b.age, 0.0));
    let band = tnt_fall(0.1, 1.4, abs(edge + (n - 0.5) * 1.2));
    let envelope = smoothstep(0.02, 0.12, b.age) * tnt_fall(0.55, 1.65, b.age);
    return band * envelope * (0.12 + 0.18 * n);
}

// How much of a condensation ring a blast of `size` raises: none for small blasts, in full from
// a few thousand TNT.
fn tnt_ring_strength(size: f32) -> f32 {
    return smoothstep(17.0, 50.0, size);
}

// The condensation ring of a big blast (a Wilson cloud): where the shockwave passes through damp
// air, the drop in pressure behind it chills the air and its vapor condenses into a thin white
// cloud. It shows round the mushroom's stem as a flat, ragged ring that forms as the front goes
// by, then drifts slowly outwards, thinning and fraying away. `near` draws the half of the ray in
// front of the blast's middle, else the half behind it (the mushroom stands between the two).
// The colour to add and how much of the picture it hides.
fn tnt_ring(dir: vec3<f32>, max_dist: f32, b: TntBlast, jitter: f32, near: bool) -> vec4<f32> {
    let strength = tnt_ring_strength(b.s);
    if strength <= 0.0 {
        return vec4<f32>(0.0);
    }
    // Round the stem, under the cap, and when the front gets there.
    let h0 = b.s * (0.55 + 0.2 * b.seed);
    let r0 = b.s * 1.1;
    let born = length(vec2<f32>(r0, h0)) / (TNT_SHOCK_SPEED * tnt_scale(b.s));
    let since = b.age - born;
    if since <= 0.0 {
        return vec4<f32>(0.0);
    }
    // It drifts on the blast's slow clock, quickly at first, then hardly at all, and thins as it goes.
    let slow = tnt_scale(b.s);
    let drift = 1.0 - exp(-since / (3.0 * slow));
    let radius = r0 + b.s * 2.0 * drift;
    let width = b.s * (0.13 + 0.1 * drift);
    let fade = smoothstep(0.0, 0.4, since) * tnt_fall(0.25, 1.0, since / (7.0 * slow)) * (1.0 - 0.6 * drift)
        * strength;
    if fade < 0.01 {
        return vec4<f32>(0.0);
    }
    let y = b.c.y + h0;
    let thick = width * 0.7;
    // The slab of air it sits in...
    var t0 = 0.0;
    var t1 = max_dist;
    if abs(dir.y) > 0.0001 {
        let ta = (y - thick) / dir.y;
        let tb = (y + thick) / dir.y;
        t0 = max(t0, min(ta, tb));
        t1 = min(t1, max(ta, tb));
    } else if abs(y) > thick {
        return vec4<f32>(0.0);
    }
    // ...the disc of its outer edge...
    let outer = radius + width * 1.6;
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
    // ...and the half of the ray asked for.
    let middle = dot(b.c + vec3<f32>(0.0, h0, 0.0), dir);
    if near {
        t1 = min(t1, middle);
    } else {
        t0 = max(t0, middle);
    }
    if t1 <= t0 {
        return vec4<f32>(0.0);
    }
    let count = 8 + 4 * i32(tnt_detail);
    let dt = (t1 - t0) / f32(count);
    let day = tnt_daylight();
    let lit = vec3<f32>(0.93, 0.95, 0.98) * (0.3 + 0.75 * day) + tnt_encode(frame.sky_color) * 0.12 * day;
    let shade = lit * 0.62;
    var trans = 1.0;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < count; i = i + 1) {
        let q = dir * (t0 + (f32(i) + jitter) * dt) - b.c;
        let rr = length(q.xz);
        let bearing = q.xz / max(rr, 0.01 * b.s);
        // Lumpy along its length, thick here, thin there, and torn through in places.
        let lumps = tnt_fbm(vec3<f32>(bearing * 6.0, b.seed * 11.0 + since * 0.05), 3);
        let fine = tnt_fbm(q * (5.0 / b.s) + vec3<f32>(b.seed * 3.0, since * 0.1, 0.0), select(2, 3, tnt_detail > 0u));
        let up = (q.y - h0) / width;
        let d = length(vec2<f32>((rr - radius) / width, up * 2.2)) / (0.5 + 0.9 * lumps);
        let tear = smoothstep(0.3, 0.55, lumps + 0.3 * (fine - 0.5));
        let dens = tnt_fall(0.35, 1.0, d + (fine - 0.5) * 1.1) * tear * (0.5 + fine);
        if dens > 0.01 {
            let a = 1.0 - exp(-dens * 1.8 * fade * dt / width);
            // Lit on top, greyer underneath.
            let c = mix(shade, lit, smoothstep(-0.6, 0.5, up)) * (0.85 + 0.3 * fine);
            colour = colour + trans * a * c;
            trans = trans * (1.0 - a);
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
    let y1 = b.c.y + height * 1.8;
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
            // Sample along the circumference so each lobe travels with the front.
            let bearing = q.xz / max(rr, 0.05 * b.s);
            let front_noise = tnt_noise(vec3<f32>(bearing * 4.5, b.seed * 8.0 + b.t * 0.45));
            let g = (rr - ring) / width - (front_noise - 0.5) * 1.5;
            let n = tnt_fbm(vec3<f32>(q.x, q.y * 2.0 - g * height * 0.35, q.z) * (3.0 / b.s)
                + vec3<f32>(b.seed * 5.0, -b.t * 0.8, b.seed * 2.0), select(3, 2, tnt_detail == 0u));
            let ridge = exp(-2.1 * g * g);
            let wall = ridge * smoothstep(0.24, 0.66, n) * 1.8 + 0.25 * tnt_fall(0.0, ring, rr);
            let lid = tnt_fall(0.0, height * (0.32 + 0.78 * n + 0.69 * ridge * front_noise), q.y);
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
    let pull = band * clamp(b.s / 4.5, 0.5, 1.6) * 0.012 * tnt_distortion;
    return vec3<f32>(outward * vec2<f32>(1.0 / aspect, 1.0) * pull, band);
}

// Shimmer the background a little where the ray crosses the hot air rising over the fire: a
// smooth field of noise, weak and short, never over the smoke's whole outline.
fn tnt_heat_haze(dir: vec3<f32>, scene_dist: f32, b: TntBlast) -> vec2<f32> {
    if tnt_distortion <= 0.0 || b.age >= 1.6 {
        return vec2<f32>(0.0);
    }
    let cl = tnt_cloud(b);
    let centre = b.c + vec3<f32>(0.0, cl.yc + cl.rc * 0.6, 0.0);
    let along = dot(dir, centre);
    let q = (dir * along - centre) / cl.rc;
    let r = length(q);
    if along <= 0.0 || scene_dist <= along || r >= 1.4 {
        return vec2<f32>(0.0);
    }
    let envelope = smoothstep(0.35, 0.8, r) * tnt_fall(0.9, 1.4, r) * smoothstep(-0.2, 0.3, q.y)
        * smoothstep(0.02, 0.15, b.age) * tnt_fall(0.4, 1.6, b.age);
    let p = q * 2.5 + vec3<f32>(b.seed * 6.0, -b.age * 3.0, 0.0);
    let wobble = vec2<f32>(tnt_noise(p), tnt_noise(p + vec3<f32>(5.2, 1.3, 7.7))) * 2.0 - vec2<f32>(1.0);
    let aspect = frame.resolution.y / frame.resolution.x;
    return wobble * vec2<f32>(aspect, 1.0) * (0.006 * min(cl.rc / along, 0.7) * envelope * tnt_distortion);
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
    var haze = vec2<f32>(0.0);
    for (var k = 0; k < 3; k = k + 1) {
        if tnt_live(k) {
            let b = tnt_blast(k, eye);
            let bend = tnt_shock(input.uv, dir, scene_dist, ground, b, eye);
            haze += tnt_heat_haze(dir, scene_dist, b);
            offset = offset + bend.xy;
            crest = crest + bend.z;
        }
    }
    var colour = tnt_encode(input.color.rgb);
    let heated_uv = input.uv - haze;
    if crest > 0.001 {
        let lo = vec2<f32>(0.001);
        let hi = vec2<f32>(0.999);
        let red = textureSampleLevel(previous, linear_sampler, clamp(heated_uv - offset * 0.97, lo, hi), 0.0).r;
        let green = textureSampleLevel(previous, linear_sampler, clamp(heated_uv - offset, lo, hi), 0.0).g;
        let blue = textureSampleLevel(previous, linear_sampler, clamp(heated_uv - offset * 1.03, lo, hi), 0.0).b;
        colour = tnt_encode(vec3<f32>(red, green, blue)) + vec3<f32>(0.9, 0.85, 0.7) * (crest * 0.015);
    } else if dot(haze, haze) > 1e-12 {
        colour = tnt_encode(textureSampleLevel(previous, linear_sampler,
            clamp(heated_uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb);
    }

    var flash = 0.0;
    var afterimage = 0.0;
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
            if tnt_flash > 0.0 {
                let far = length(b.c) / (b.s * 12.0);
                let intensity = tnt_flash / (1.0 + far * far);
                flash = max(flash, intensity * exp(-b.t / 0.09));
                // The iris closes after the peak, then reopens in real time.
                afterimage = max(afterimage, intensity * smoothstep(0.03, 0.36, b.age)
                    * exp(-b.age / 1.25));
            }

            // 3. The dust along the ground, then the fireball and its smoke over it.
            let vapor = tnt_vapor(dir, scene_dist, b);
            colour = mix(colour, vec3<f32>(0.92, 0.96, 1.0), vapor);
            if b.grounded > 0.5 {
                let dust = tnt_dust(dir, scene_dist, b, jitter);
                colour = colour * (1.0 - dust.a) + dust.rgb;
            }
            // The condensation ring round the mushroom: its far half behind the cloud, its near
            // half in front.
            let ring_far = tnt_ring(dir, scene_dist, b, jitter, false);
            colour = colour * (1.0 - ring_far.a) + ring_far.rgb;
            let cloud = tnt_march(dir, scene_dist, b, jitter);
            colour = colour * (1.0 - cloud.a) + cloud.rgb;
            let ring_near = tnt_ring(dir, scene_dist, b, jitter, true);
            colour = colour * (1.0 - ring_near.a) + ring_near.rgb;

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
    // Exposure is a gain on linear radiance over the whole composed image.
    let gain = exp2(2.35 * flash - min(afterimage, 1.0) * 1.25);
    let exposed = tnt_decode(colour) * gain + vec3<f32>(1.0, 0.94, 0.83) * (flash * 0.75);
    return vec4<f32>(exposed, 1.0);
}

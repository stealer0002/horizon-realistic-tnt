"""Makes the mod's explosion sounds, as a real blast is heard: the booms, rumbles, debris and cracks
cut from real recorded explosions (CC0, see `SOURCES`), the rest synthesised, into
realistic_tnt/assets/realistic_tnt/sounds/ (Ogg Vorbis, mono, so the game places them in the world)
and writes the sound events to realistic_tnt/assets/realistic_tnt/sounds.json.

What a listener hears of a big blast (the 2020 Beirut blast, recorded by phones and seismometers):
  - the flash at once, and with it, near the fireball, its roar: a hiss and crackle (`flash`);
  - the ground shaking before the air wave gets there: sound runs through rock several times faster
    than through air, felt and heard as a low rumble growing (`ground`);
  - the shock front itself, a sharp crack (an N-wave: a jump in pressure, a drop below it) close by
    (`crack`), then the boom: a heavy low thump close by (`boom_near`), only a dull low thud far off,
    where the air has eaten its high pitches (`boom_far`);
  - the boom rolling on, echoed by the ground and hills, like thunder (`rumble`);
  - what it threw up raining down, glass among it (`debris`);
  - close by, the ears ringing for a while after (`ringing`).

Every sound comes from a fixed seed: running it again writes the same files.
Needs numpy and soundfile:   python tools/make_sounds.py
"""

import json
import pathlib

import numpy as np
import soundfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
ASSETS = ROOT / "realistic_tnt" / "assets" / "realistic_tnt"
RATE = 44100


def t(seconds):
    return np.arange(int(seconds * RATE)) / RATE


def noise(rng, seconds):
    return rng.standard_normal(int(seconds * RATE))


def band(x, low=0.0, high=None, slope=1.0):
    """`x` with what lies outside `low`..`high` Hz cut away, with soft edges (`slope` octaves)."""
    spectrum = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / RATE)
    gain = np.ones_like(f)
    if low > 0:
        gain *= 1 / (1 + (low / np.maximum(f, 1e-3)) ** (4 / slope))
    if high is not None:
        gain *= 1 / (1 + (f / high) ** (4 / slope))
    return np.fft.irfft(spectrum * gain, len(x))


def sweep_low(x, start, end, tau):
    """`x` low-passed with a cut-off gliding from `start` to `end` Hz (time constant `tau`), in blocks."""
    out = np.zeros_like(x)
    block = 2048
    for i in range(0, len(x), block):
        k = i / RATE
        cut = end + (start - end) * np.exp(-k / tau)
        lo, hi = max(0, i - block), min(len(x), i + 2 * block)
        part = band(x[lo:hi], high=cut)
        out[i:i + block] = part[i - lo:i - lo + min(block, len(x) - i)]
    return out


def envelope(seconds, attack, decay, hold=0.0):
    k = t(seconds)
    rise = np.clip(k / max(attack, 1e-4), 0, 1)
    fall = np.exp(-np.maximum(k - attack - hold, 0) / decay)
    return rise * fall


def fade_out(x, seconds=0.25):
    n = min(len(x), int(seconds * RATE))
    x[-n:] *= np.linspace(1, 0, n) ** 2
    return x


def squash(x, drive=1.0):
    """A soft limit: loud parts swell instead of clipping, as a blast overloads a mic and the ear."""
    return np.tanh(x * drive) / np.tanh(drive)


def normal(x, peak=0.9):
    x = fade_out(x - np.mean(x))
    return x * (peak / max(np.max(np.abs(x)), 1e-9))


def flash(rng):
    """The fireball's roar as it flares: a hiss rising and falling, crackling, over a tense hum."""
    secs = 2.2
    k = t(secs)
    hiss = band(noise(rng, secs), 900, 7000) * envelope(secs, 0.03, 0.7, 0.15)
    crackle = np.zeros_like(k)
    for at in rng.uniform(0.0, 1.6, 140):
        i = int(at * RATE)
        n = int(rng.uniform(0.002, 0.008) * RATE)
        crackle[i:i + n] += rng.uniform(0.3, 1.0) * rng.standard_normal(min(n, len(k) - i))
    crackle = band(crackle, 1500, 9000) * envelope(secs, 0.01, 0.6, 0.2)
    # The hum: two close tones beating, gliding down as the fireball cools.
    pitch = 160 + 90 * np.exp(-k / 0.5)
    phase = 2 * np.pi * np.cumsum(pitch) / RATE
    hum = (np.sin(phase) + 0.6 * np.sin(phase * 1.013) + 0.3 * np.sin(phase * 2.01)) * envelope(secs, 0.02, 0.8, 0.2)
    roar = band(noise(rng, secs), 60, 400) * envelope(secs, 0.05, 0.9, 0.1)
    return normal(0.55 * hiss + 0.45 * crackle + 0.35 * hum + 0.6 * roar, 0.8)


def ground(rng):
    """The ground shaking ahead of the air wave: a low rumble that grows, then dies away."""
    secs = 3.5
    k = t(secs)
    rumble = band(noise(rng, secs), 18, 90)
    swell = np.clip(k / 1.4, 0, 1) ** 2 * np.exp(-np.maximum(k - 1.4, 0) / 0.7)
    shudder = 1 + 0.5 * np.sin(2 * np.pi * 7.5 * k + rng.uniform(0, 6)) * band(noise(rng, secs), 0, 12)
    rattle = band(noise(rng, secs), 300, 1800) * swell ** 2 * 0.12
    return normal(squash(rumble * swell * shudder * 2.2, 1.5) + rattle, 0.85)


def n_wave(length, rng):
    """A blast's pressure, as a mic close by takes it: a jump, a fall past nothing, a slow recovery."""
    k = t(length)
    rise = 0.0006
    wave = np.where(k < rise, k / rise, 1 - (k - rise) / (length * 0.45))
    wave = np.where(k > length * 0.45, -0.55 * np.sin(np.pi * (k - length * 0.45) / (length * 0.55)), wave)
    return wave + 0.05 * rng.standard_normal(len(k))


def crack(rng):
    """The shock front close by: one sharp crack, and the air tearing after it."""
    secs = 0.9
    out = np.zeros(int(secs * RATE))
    wave = n_wave(0.012, rng)
    out[: len(wave)] += wave
    tear = band(noise(rng, secs), 1200, 12000) * envelope(secs, 0.0005, 0.06)
    snap = band(noise(rng, secs), 200, 3000) * envelope(secs, 0.001, 0.12)
    return normal(squash(1.2 * out + 0.7 * tear + 0.5 * snap, 2.5), 0.55)


def boom(rng, far):
    """The boom: close by a heavy low thump with grit in it; far off a dull, slower thud."""
    secs = 5.5 if far else 4.5
    k = t(secs)
    out = np.zeros_like(k)
    wave = n_wave(0.09 if far else 0.045, rng)
    out[: len(wave)] += wave * (0.7 if far else 1.0)
    body = sweep_low(noise(rng, secs), 260 if far else 1600, 70 if far else 140, 0.25 if far else 0.4)
    body *= envelope(secs, 0.03 if far else 0.004, 1.2 if far else 0.9, 0.05)
    sub = np.sin(2 * np.pi * (38 + 25 * np.exp(-k / 0.3)) * k) * envelope(secs, 0.01, 0.8 if far else 0.6)
    mix = 1.3 * out + 1.2 * body + 0.9 * sub
    if far:
        mix = band(mix, 0, 220, 2)
    return normal(squash(mix, 2.0 if far else 3.0), 0.82)


def rumble(rng):
    """The boom rolling on: its echoes off the ground and hills, coming back later and lower."""
    secs = 9.0
    k = t(secs)
    out = np.zeros_like(k)
    for i in range(26):
        at = rng.uniform(0.05, 6.0) ** 1.15
        loud = np.exp(-at / 2.2) * rng.uniform(0.4, 1.0)
        width = rng.uniform(0.25, 0.9)
        out += loud * np.exp(-0.5 * ((k - at) / width) ** 2) * (1 + 0.3 * np.sin(2 * np.pi * rng.uniform(2, 5) * k))
    body = band(noise(rng, secs), 25, 170, 2)
    rolling = body * out + 0.2 * band(noise(rng, secs), 25, 80) * envelope(secs, 0.4, 2.5)
    return normal(squash(rolling * 3.0, 1.4), 0.85)


def boom_small(rng):
    """One TNT, close by: a vanilla explosion done well. A hard crack, a punchy mid-range bang with
    grit in it, and a short tail; little of the deep body a big blast has."""
    secs = 2.2
    k = t(secs)
    out = np.zeros_like(k)
    wave = n_wave(0.018, rng)
    out[: len(wave)] += wave
    bang = band(noise(rng, secs), 250, 4500) * envelope(secs, 0.001, 0.13)
    body = sweep_low(noise(rng, secs), 4000, 300, 0.12) * envelope(secs, 0.002, 0.38, 0.02)
    sub = np.sin(2 * np.pi * (62 + 40 * np.exp(-k / 0.08)) * k) * envelope(secs, 0.003, 0.22)
    grit = band(noise(rng, secs), 2000, 9000) * envelope(secs, 0.001, 0.05)
    return normal(squash(1.1 * out + 0.9 * bang + 1.0 * body + 0.7 * sub + 0.4 * grit, 3.0), 0.6)


def boom_medium(rng):
    """Tens of TNT: heavier and deeper than one, the bang spreading into a low body."""
    secs = 3.6
    k = t(secs)
    out = np.zeros_like(k)
    wave = n_wave(0.03, rng)
    out[: len(wave)] += wave
    bang = band(noise(rng, secs), 150, 3000) * envelope(secs, 0.002, 0.2)
    body = sweep_low(noise(rng, secs), 2600, 190, 0.28) * envelope(secs, 0.003, 0.7, 0.04)
    sub = np.sin(2 * np.pi * (48 + 30 * np.exp(-k / 0.2)) * k) * envelope(secs, 0.006, 0.45)
    return normal(squash(1.2 * out + 0.6 * bang + 1.2 * body + 0.9 * sub, 3.0), 0.7)


def boom_huge(rng):
    """A nuclear-sized blast: a long, crushing low roar, the deepest pitches swelling for seconds,
    with the fireball's crackling torn into it."""
    secs = 10.0
    k = t(secs)
    out = np.zeros_like(k)
    wave = n_wave(0.16, rng)
    out[: len(wave)] += 1.2 * wave
    body = sweep_low(noise(rng, secs), 900, 55, 1.3) * envelope(secs, 0.015, 2.6, 0.35)
    sub = np.sin(2 * np.pi * (26 + 18 * np.exp(-k / 0.9)) * k) * envelope(secs, 0.03, 2.4, 0.2)
    sub2 = np.sin(2 * np.pi * (41 + 12 * np.exp(-k / 1.2)) * k + 1.3) * envelope(secs, 0.05, 1.8, 0.2)
    crackle = band(noise(rng, secs) * (rng.random(int(secs * RATE)) < 0.002), 300, 3000)
    crackle *= envelope(secs, 0.2, 2.5, 0.5) * 6.0
    return normal(squash(1.1 * out + 1.5 * body + 1.0 * sub + 0.6 * sub2 + 0.4 * crackle, 2.2), 0.85)


def rumble_huge(rng):
    """A nuclear blast's rolling: twenty seconds of echoes and low thunder, slowly dying."""
    secs = 22.0
    k = t(secs)
    out = np.zeros_like(k)
    for _ in range(70):
        at = rng.uniform(0.1, 17.0) ** 1.1
        loud = np.exp(-at / 6.5) * rng.uniform(0.4, 1.0)
        width = rng.uniform(0.4, 1.6)
        out += loud * np.exp(-0.5 * ((k - at) / width) ** 2)
    body = band(noise(rng, secs), 18, 130, 2)
    rolling = body * out + 0.25 * band(noise(rng, secs), 18, 60) * envelope(secs, 1.0, 7.0)
    return normal(squash(rolling * 3.0, 1.4), 0.85)


def wind(rng):
    """After a nuclear-sized blast, its wind: a gale rushing out and then back in toward the rising
    fireball, gusting, roaring through everything."""
    secs = 11.0
    k = t(secs)
    swell = np.clip(k / 2.5, 0, 1) ** 1.5 * np.exp(-np.maximum(k - 4.0, 0) / 2.5)
    gusts = 1 + 0.45 * band(noise(rng, secs), 0, 1.5) / 0.05
    gusts = np.clip(gusts, 0.2, 2.0)
    roar = band(noise(rng, secs), 40, 700, 2) * swell * gusts
    howl = band(noise(rng, secs), 500, 1800) * swell ** 2 * gusts * 0.3
    return normal(roar + howl, 0.8)


def flash_small(rng):
    """One TNT's flash: a short fizz and pop as it goes."""
    secs = 0.6
    hiss = band(noise(rng, secs), 1500, 9000) * envelope(secs, 0.005, 0.1)
    pop = band(noise(rng, secs), 200, 1500) * envelope(secs, 0.002, 0.05)
    return normal(0.7 * hiss + 0.5 * pop, 0.6)


def flash_huge(rng):
    """A nuclear-sized fireball: a deep, swelling roar for seconds, crackling, over a low hum."""
    secs = 7.0
    k = t(secs)
    roar = band(noise(rng, secs), 30, 500, 2) * envelope(secs, 0.4, 2.2, 1.2)
    hiss = band(noise(rng, secs), 800, 5000) * envelope(secs, 0.05, 1.5, 0.5)
    crackle = band(noise(rng, secs) * (rng.random(int(secs * RATE)) < 0.004), 1000, 7000) * 8.0
    crackle *= envelope(secs, 0.1, 2.0, 1.0)
    pitch = 90 + 50 * np.exp(-k / 1.5)
    phase = 2 * np.pi * np.cumsum(pitch) / RATE
    hum = (np.sin(phase) + 0.5 * np.sin(phase * 1.009)) * envelope(secs, 0.2, 2.5, 1.0)
    return normal(1.1 * roar + 0.4 * hiss + 0.4 * crackle + 0.35 * hum, 0.85)


def debris(rng):
    """What the blast threw up coming down: thuds and clatter of rock and earth, and glass."""
    secs = 5.0
    k = t(secs)
    out = np.zeros_like(k)
    for _ in range(220):
        at = rng.uniform(0.0, 4.2) ** 1.3 / 4.2 ** 0.3
        i = int(at * RATE)
        if i >= len(k) - 10:
            continue
        loud = rng.uniform(0.2, 1.0) * np.exp(-at / 2.0)
        n = int(rng.uniform(0.01, 0.06) * RATE)
        hit = rng.standard_normal(n) * np.exp(-np.arange(n) / (n * 0.25))
        lo = rng.uniform(80, 500)
        hit = band(np.pad(hit, (0, 256)), lo, lo * rng.uniform(3, 8))[:n]
        out[i:i + n] += loud * hit[: len(k) - i]
    for _ in range(26):
        at = rng.uniform(0.2, 3.5)
        i = int(at * RATE)
        n = int(0.35 * RATE)
        f = rng.uniform(2500, 6500)
        ping = np.sin(2 * np.pi * f * t(0.35)) * np.exp(-t(0.35) / rng.uniform(0.03, 0.09))
        out[i:i + n] += 0.25 * rng.uniform(0.3, 1.0) * ping[: len(k) - i]
    return normal(out, 0.8)


def ringing(rng):
    """Close by, the ears ring after the boom: a thin high tone that fades slowly."""
    secs = 8.0
    k = t(secs)
    pitch = 3900 * (1 + 0.004 * np.sin(2 * np.pi * 0.7 * k))
    phase = 2 * np.pi * np.cumsum(pitch) / RATE
    tone = np.sin(phase) + 0.15 * np.sin(phase * 1.5) + 0.08 * band(noise(rng, secs), 3500, 4500)
    env = np.clip(k / 0.35, 0, 1) * np.exp(-np.maximum(k - 0.6, 0) / 2.2)
    return normal(tone * env, 0.5)


# Real explosions, recorded: the SSE Library's EXPLOSIONS (USC Cinema / Sunset Editorial), released
# under CC0 1.0 on the Internet Archive (archive.org/details/SSE_Library_EXPLOSIONS). The booms,
# rumbles, debris and cracks are cut from these; the flash, ground, wind and ringing stay synthesised.
# Downloaded once into tools/sounds-source/ (not committed).
SOURCE_URL = "https://archive.org/download/SSE_Library_EXPLOSIONS/REAL/"
SOURCES = {
    "dynamite": "EXPLReal_Dynamite explosion_CS_USC.flac",
    "grenade": "EXPLReal_Grenade explodes_CHS_USC.flac",
    "sharp": "EXPLReal_Heavy sharp explosion with reverb_CS_USC.flac",
    "heavy1": "EXPLReal_Single heavy explosions 1_CS_USC.flac",
    "heavy2": "EXPLReal_Single heavy explosions 2_CS_USC.flac",
    "reverb": "EXPLReal_Heavy explosion with reverberation_CS_USC.flac",
    "land": "EXPLReal_Land explosion_CS_USC.flac",
    "shell": "EXPLReal_Explosion; bomb or shell_CHS_USC.flac",
    "large": "EXPLReal_Large explosion with debris_CS_USC.flac",
    "violent": "EXPLReal_Violent explosion and debris; 2 takes_CHS_USC.flac",
    "falling": "EXPLReal_Explosion with falling debris_CS_USC.flac",
    "heavy_takes": "EXPLReal_Heavy explosions; 2 takes_CS_USC.flac",
    "atom": "EXPLReal_Atom bomb explosion_CS_USC.flac",
    "atom_rumble": "EXPLReal_Atomic bomb explosion then rumble_CS_USC.flac",
    "atomic": "EXPLReal_Atomic bomb_CHS_USC.flac",
    "distant": "EXPLReal_Medium distant explosions; cluster_CS_USC.flac",
}
SOURCE_DIR = pathlib.Path(__file__).resolve().parent / "sounds-source"
_loaded = {}


def source(key):
    """A recording, mono at RATE, fetched the first time."""
    if key not in _loaded:
        path = SOURCE_DIR / SOURCES[key]
        if not path.exists():
            import urllib.parse
            import urllib.request
            SOURCE_DIR.mkdir(exist_ok=True)
            urllib.request.urlretrieve(SOURCE_URL + urllib.parse.quote(SOURCES[key]), path)
        x, rate = soundfile.read(path, always_2d=True)
        x = x.mean(axis=1)
        if rate != RATE:
            n = int(round(len(x) * RATE / rate))
            spectrum = np.fft.rfft(x)
            keep = n // 2 + 1
            x = np.fft.irfft(spectrum[:keep], n) * (n / len(x)) if keep <= len(spectrum) else \
                np.fft.irfft(np.pad(spectrum, (0, keep - len(spectrum))), n) * (n / len(x))
        _loaded[key] = x
    return _loaded[key]


def onset(x, after=0.0):
    """Where, from `after` seconds on, the recording's first bang starts (a little before it)."""
    start = int(after * RATE)
    loud = np.abs(x[start:])
    hit = np.argmax(loud > 0.3 * loud.max())
    return max(0, start + hit - int(0.008 * RATE))


def cut(key, length, after=0.0, low=0.0, high=None, fade=0.6, peak=0.9, drive=0.0, from_bang=True):
    """`length` seconds of a recording from its bang (or from `after`), band-limited, faded, levelled.
    `drive` adds the overload of a blast hitting the mic close by."""
    x = source(key)
    start = onset(x, after) if from_bang else int(after * RATE)
    y = x[start:start + int(length * RATE)].copy()
    if low > 0 or high is not None:
        y = band(y, low, high)
    n = min(len(y), int(0.004 * RATE))
    y[:n] *= np.linspace(0, 1, n)
    y = fade_out(y, min(fade, length / 2))
    y = y * (1.0 / max(np.max(np.abs(y)), 1e-9))
    if drive > 0:
        y = squash(y, drive)
    return normal(y, peak)


def mix(*parts):
    """Recordings laid over each other, as (sound, gain, delay in seconds)."""
    length = max(len(p) + int(d * RATE) for p, _, d in parts)
    out = np.zeros(length)
    for p, gain, delay in parts:
        i = int(delay * RATE)
        out[i:i + len(p)] += gain * p
    return out


def quieter(make, gain):
    return lambda rng: make(rng) * gain


def compress(x, threshold_db=-24.0, ratio=4.0, window=0.012, release=0.25):
    """Squeezes the loud and lifts the quiet, as a blast heard close is all loud: a block compressor."""
    w = max(1, int(window * RATE))
    n = len(x) // w + 1
    padded = np.pad(x, (0, n * w - len(x)))
    level = np.sqrt((padded.reshape(n, w) ** 2).mean(axis=1)) + 1e-9
    # Slow release: the level falls only gradually, so the tail swells instead of pumping.
    fall = np.exp(-window / release)
    for i in range(1, n):
        level[i] = max(level[i], level[i - 1] * fall)
    db = 20 * np.log10(level)
    gain_db = np.where(db > threshold_db, (threshold_db - db) * (1 - 1 / ratio), 0.0)
    gain = 10 ** (gain_db / 20)
    per_sample = np.interp(np.arange(len(x)), np.arange(n) * w + w / 2, gain)
    return x * per_sample


def thump(length, start_hz, end_hz, sweep, decay, attack=0.002):
    """The blow felt in the chest: a very low tone dropping in pitch as the pressure wave passes,
    with its first overtone so small speakers carry it too."""
    k = t(length)
    hz = end_hz + (start_hz - end_hz) * np.exp(-k / sweep)
    phase = 2 * np.pi * np.cumsum(hz) / RATE
    tone = np.sin(phase) + 0.35 * np.sin(2 * phase) + 0.12 * np.sin(3 * phase)
    return tone * envelope(length, attack, decay)


def brutal(x, rng, sub=(90, 32, 0.12, 0.6), sub_gain=0.9, low_boost=0.8, crack_gain=0.5, drive=2.5,
           squeeze=(-26.0, 5.0), tail=0.0):
    """A recorded blast made as it is felt close by: the old tape's missing lows put back (a low boost
    and the chest thump), the first instant's crack sharpened, the whole overdriven as the ear and a
    mic are by a blast, and squeezed so the roar rolls on loud. `tail` adds seconds of low rumble."""
    x = x / max(np.max(np.abs(x)), 1e-9)
    length = len(x) / RATE + tail
    out = np.zeros(int(length * RATE))
    out[:len(x)] += x
    out[:len(x)] += low_boost * band(x, high=140)
    start_hz, end_hz, sweep, decay = sub
    out += sub_gain * thump(length, start_hz, end_hz, sweep, decay)[:len(out)]
    edge = min(len(x), int(0.06 * RATE))
    hit = band(x[:edge], low=1500) * np.exp(-np.arange(edge) / (0.012 * RATE))
    out[:edge] += crack_gain * hit / max(np.max(np.abs(hit)), 1e-9)
    if tail > 0:
        roll = band(rng.standard_normal(len(out)), high=90) * envelope(length, 0.4, tail * 0.45)
        out += 0.5 * roll / max(np.max(np.abs(roll)), 1e-9)
    out = compress(out, *squeeze)
    out = squash(out / max(np.max(np.abs(out)), 1e-9), drive)
    # Below full scale: Vorbis overshoots a peak a little when the file is decoded.
    return normal(out, 0.86)


def brutal_make(make, **opts):
    return lambda rng: brutal(make(rng), rng, **opts)


RECORDED = {
    # One TNT close by: a dynamite and a grenade, hard and punchy.
    "boom_small": [
        lambda r: normal(mix((cut("dynamite", 3.2, drive=1.2), 1.0, 0), (cut("sharp", 1.2, low=60), 0.35, 0))),
        lambda r: normal(mix((cut("grenade", 3.0, drive=1.2), 1.0, 0), (cut("heavy2", 1.5, high=900), 0.3, 0))),
    ],
    # Tens of TNT: heavier single blasts with a low body.
    "boom_medium": [
        lambda r: normal(mix((cut("heavy1", 4.2, drive=1.5), 1.0, 0), (cut("land", 3.0, high=300), 0.5, 0.02))),
        lambda r: normal(mix((cut("shell", 5.0, drive=1.5), 1.0, 0), (cut("heavy2", 3.0, high=400), 0.45, 0.01))),
    ],
    # Hundreds and thousands: big blasts throwing debris.
    "boom_large": [
        lambda r: normal(mix((cut("large", 8.5, drive=1.8), 1.0, 0), (cut("reverb", 6.0, high=250), 0.5, 0.03))),
        lambda r: normal(mix((cut("violent", 7.5, after=0.4, drive=1.8), 1.0, 0),
                             (cut("heavy_takes", 4.5, high=250), 0.45, 0.02))),
    ],
    # Nuclear-sized: atomic test recordings.
    "boom_huge": [
        lambda r: normal(mix((cut("atom_rumble", 14.5, fade=4.0, drive=1.3), 1.0, 0),
                             (cut("large", 6.0, high=180), 0.5, 0))),
        lambda r: normal(mix((cut("atomic", 22.0, after=0.3, fade=6.0, drive=1.6), 1.0, 0),
                             (cut("violent", 6.0, after=0.4, high=200), 0.5, 0))),
    ],
    # Far off: the air has taken the high pitches.
    "boom_far": [
        lambda r: cut("distant", 5.5, high=700, fade=1.5),
        lambda r: cut("reverb", 6.0, high=450, fade=2.0),
    ],
    # The boom rolling on: the tails of real blasts, low-passed.
    "rumble": [
        lambda r: cut("heavy_takes", 4.0, after=6.2, from_bang=False, high=380, fade=2.0, peak=0.7),
        lambda r: cut("land", 6.0, after=1.8, from_bang=False, high=350, fade=2.5, peak=0.7),
    ],
    "rumble_huge": [lambda r: cut("atom", 30.0, after=12.0, from_bang=False, high=320, fade=10.0, peak=0.8)],
    # Debris coming down.
    "debris": [lambda r: normal(mix((cut("large", 5.0, after=3.2, from_bang=False, low=250, fade=2.0), 1.0, 0),
                                    (cut("falling", 2.5, after=0.5, from_bang=False, low=300, fade=1.0), 0.6, 0.4)),
                                0.6)],
    # The crack close by: a real blast's first instant, its high part.
    "crack": [
        lambda r: cut("sharp", 0.35, low=1200, fade=0.2, peak=0.5),
        lambda r: cut("dynamite", 0.35, low=1200, fade=0.2, peak=0.5),
    ],
}


def main():
    sounds = ASSETS / "sounds" / "blast"
    sounds.mkdir(parents=True, exist_ok=True)
    # By how big the blast is (src/lib.rs `SoundTier`): one TNT (small), tens (medium), hundreds and
    # thousands (large), nuclear-sized (huge).
    made = {
        "flash_small": [flash_small],
        "flash": [flash],
        "flash_huge": [flash_huge],
        "ground": [ground],
        "crack": [crack, crack],
        "boom_small": [boom_small, boom_small],
        "boom_medium": [boom_medium, boom_medium],
        "boom_large": [lambda r: boom(r, False), lambda r: boom(r, False)],
        "boom_huge": [boom_huge, boom_huge],
        "boom_far": [lambda r: boom(r, True), lambda r: boom(r, True)],
        "rumble": [rumble, rumble],
        "rumble_huge": [rumble_huge],
        "wind": [wind],
        "debris": [debris],
        "ringing": [ringing],
    }
    made.update(RECORDED)
    # Brutal as a blast is felt close by, more so the bigger it is.
    heavier = {
        "boom_small": dict(sub=(110, 45, 0.05, 0.35), sub_gain=0.8, low_boost=0.7, crack_gain=0.6, drive=2.2,
                           squeeze=(-24.0, 4.0)),
        "boom_medium": dict(sub=(95, 36, 0.08, 0.7), sub_gain=0.9, low_boost=0.9, crack_gain=0.55, drive=2.6,
                            squeeze=(-26.0, 5.0), tail=1.0),
        "boom_large": dict(sub=(85, 30, 0.14, 1.4), sub_gain=1.0, low_boost=1.0, crack_gain=0.5, drive=3.0,
                           squeeze=(-28.0, 6.0), tail=2.5),
        "boom_huge": dict(sub=(70, 24, 0.4, 4.0), sub_gain=1.1, low_boost=1.2, crack_gain=0.45, drive=3.2,
                          squeeze=(-30.0, 6.0), tail=6.0),
        "boom_far": dict(sub=(60, 28, 0.2, 1.0), sub_gain=1.0, low_boost=1.2, crack_gain=0.0, drive=1.6,
                         squeeze=(-28.0, 4.0)),
        "rumble": dict(sub=(50, 26, 0.5, 2.0), sub_gain=0.6, low_boost=1.2, crack_gain=0.0, drive=1.8,
                       squeeze=(-30.0, 5.0)),
        "rumble_huge": dict(sub=(45, 22, 2.0, 12.0), sub_gain=0.8, low_boost=1.4, crack_gain=0.0, drive=2.0,
                            squeeze=(-32.0, 6.0)),
        "crack": dict(sub=(160, 80, 0.01, 0.05), sub_gain=0.3, low_boost=0.0, crack_gain=1.0, drive=3.0,
                      squeeze=(-20.0, 3.0)),
    }
    for name, opts in heavier.items():
        made[name] = [brutal_make(make, **opts) for make in made[name]]
        if name.startswith("rumble"):
            # Under the boom it rolls on from, not as loud.
            made[name] = [quieter(make, 0.75) for make in made[name]]
    # The synthesised layers sit under the recorded boom, not over it.
    for name in ("flash_small", "flash", "flash_huge", "ground"):
        made[name] = [quieter(make, 0.55) for make in made[name]]
    for old in (ASSETS / "sounds" / "blast").glob("*.ogg"):
        old.unlink()
    events = {}
    for seed, (name, makers) in enumerate(made.items()):
        files = []
        for variant, make in enumerate(makers):
            rng = np.random.default_rng(1000 + seed * 10 + variant)
            stem = name if len(makers) == 1 else f"{name}{variant + 1}"
            samples = make(rng).astype(np.float32)
            # In blocks: libsndfile's Vorbis encoder fails on a long sound written at once.
            with soundfile.SoundFile(sounds / f"{stem}.ogg", "w", RATE, 1, format="OGG", subtype="VORBIS") as out:
                for i in range(0, len(samples), 8192):
                    out.write(samples[i:i + 8192])
            files.append({"name": f"realistic_tnt:blast/{stem}"})
        events[f"blast.{name}"] = {"sounds": files}
    (ASSETS / "sounds.json").write_text(json.dumps(events, indent=2) + "\n", encoding="utf-8")
    total = sum(p.stat().st_size for p in sounds.glob("*.ogg"))
    print(f"{len(list(sounds.glob('*.ogg')))} sounds, {total // 1024} KiB, in {sounds}")


if __name__ == "__main__":
    main()

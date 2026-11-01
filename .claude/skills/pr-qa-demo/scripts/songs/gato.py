# "Gato indie" (first heard on PR #832): a lo-fi Spanish-flavoured indie loop with cats, synthesised
# with the standard library only. Writes a 16-bit stereo WAV of `seconds`.
#
#   python3 -I gato.py track.wav 62
#
# Andalusian cadence (Am G F E) on a plucked nylon guitar (Karplus-Strong),
# rumba strums, palmas (hand claps), a round bass, a whistled lead with
# tremolo, vinyl crackle, and a cat: formant meows on the E chord, a purr at
# the end. Fade in and out.
import math, struct, sys, wave, random, array

RATE = 24000
BPM = 118
BEAT = 60 / BPM
BAR = 4 * BEAT
rng = random.Random(42)

def hz(n):
    return 440.0 * 2 ** ((n - 69) / 12)

# Bars: Am | G | F | E7, four bars a loop. Each chord: bass note, guitar voicing (MIDI).
CHORDS = [
    (45, [57, 60, 64, 69]),      # A2 ; A3 C4 E4 A4
    (43, [55, 59, 62, 67]),      # G2 ; G3 B3 D4 G4
    (41, [53, 57, 60, 65]),      # F2 ; F3 A3 C4 F4
    (40, [52, 56, 59, 62, 64]),  # E2 ; E3 G#3 B3 D4 E4  (E7)
]
# Rumba strum pattern in beats within a bar: (time, direction, accent)
STRUMS = [(0, 1, 1.0), (1.5, -1, .55), (2, 1, .9), (2.75, -1, .5), (3, 1, .7), (3.5, -1, .55)]
# Palmas: beats 2 and 4, a doubled "contra" on the & of 3 every other bar.
CLAPS = [(1, .9), (3, 1.0)]
# The whistled lead (MIDI, 0 = rest), eighth notes, two 4-bar phrases.
A4, B4, C5, D5, E5, F5, G5, GS5, A5 = 69, 71, 72, 74, 76, 77, 79, 80, 81
PHRASE_A = [
    E5, 0, D5, C5, B4, 0, A4, 0,      C5, 0, B4, A4, G5, 0, 0, 0,     # Am G
    A5, 0, G5, F5, E5, 0, F5, E5,     D5, 0, 0, GS5, B4, 0, E5, 0,    # F  E7
    E5, 0, D5, C5, B4, 0, A4, 0,      C5, 0, B4, A4, G5, F5, E5, D5,  # Am G
    C5, 0, A4, 0, F5, 0, E5, 0,       GS5, 0, B4, 0, E5, 0, 0, 0,     # F  E7
]
PHRASE_B = [
    A5, 0, 0, G5, E5, 0, D5, C5,      B4, 0, D5, 0, G5, 0, 0, 0,
    A5, 0, F5, 0, C5, 0, A4, 0,       B4, 0, GS5, 0, E5, 0, D5, 0,
    C5, 0, E5, 0, A5, 0, 0, 0,        G5, 0, F5, E5, D5, 0, B4, 0,
    C5, 0, A4, 0, F5, 0, E5, D5,      E5, 0, 0, 0, 0, 0, 0, 0,
]

def env(t, length, a=0.005, r=0.08):
    if t < a: return t / a
    if t > length - r: return max(0.0, (length - t) / r)
    return 1.0

class Mix:
    def __init__(self, seconds):
        self.n = int(seconds * RATE)
        self.L = array.array("d", bytes(8 * self.n))
        self.R = array.array("d", bytes(8 * self.n))
    def add(self, i, v, pan=0.0):  # pan -1 left .. 1 right
        if 0 <= i < self.n:
            self.L[i] += v * (1 - max(pan, 0))
            self.R[i] += v * (1 + min(pan, 0))

def pluck(mix, t0, f, amp, dur=1.1, decay=0.995, pan=-0.3, bright=0.6):
    """Karplus-Strong string: a noise burst through an averaging loop."""
    N = max(2, int(RATE / f))
    buf = [rng.uniform(-1, 1) for _ in range(N)]
    # a duller pick: pre-smooth the burst
    for _ in range(int((1 - bright) * 4)):
        buf = [(buf[k] + buf[k - 1]) / 2 for k in range(N)]
    i0 = int(t0 * RATE)
    n = int(dur * RATE)
    L, R = mix.L, mix.R
    gl, gr = amp * (1 - max(pan, 0)), amp * (1 + min(pan, 0))
    nn = mix.n
    for k in range(n):
        i = i0 + k
        if i >= nn: break
        j = k % N
        v = buf[j]
        buf[j] = decay * 0.5 * (v + buf[(k + 1) % N])
        L[i] += gl * v
        R[i] += gr * v

def strum(mix, t0, notes, direction, accent):
    order = notes if direction > 0 else list(reversed(notes))
    for s, n in enumerate(order):
        pluck(mix, t0 + s * 0.014, hz(n), 0.11 * accent, dur=0.9 if accent < .8 else 1.3, pan=-0.35 + 0.04 * s)

def bass(mix, t0, n, length):
    f = hz(n)
    i0 = int(t0 * RATE)
    for k in range(int(length * RATE)):
        t = k / RATE
        e = env(t, length, 0.006, 0.12) * math.exp(-t * 1.2)
        v = math.sin(2 * math.pi * f * t) + 0.25 * math.sin(2 * math.pi * 2 * f * t)
        mix.add(i0 + k, 0.22 * e * v, 0.0)

def clap(mix, t0, amp):
    # three quick noise bursts a few ms apart, band-limited by a crude averaging filter, wide stereo
    i0 = int(t0 * RATE)
    last = 0.0
    for burst in range(3):
        off = int(burst * 0.011 * RATE)
        length = 0.045 if burst < 2 else 0.14
        for k in range(int(length * RATE)):
            t = k / RATE
            w = rng.uniform(-1, 1)
            last = 0.6 * last + 0.4 * w  # soften the hiss
            v = amp * 0.30 * env(t, length, 0.001, length * 0.8) * (w - 0.5 * last)
            mix.add(i0 + off + k, v, 0.5 if (burst % 2) else -0.5)

def whistle(mix, t0, n, length, pan=0.35):
    f = hz(n)
    i0 = int(t0 * RATE)
    for k in range(int(length * RATE)):
        t = k / RATE
        vib = 1 + 0.006 * math.sin(2 * math.pi * 5.5 * t) * min(1.0, t / 0.15)
        trem = 1 - 0.18 * (0.5 + 0.5 * math.sin(2 * math.pi * 6.3 * t))
        ph = 2 * math.pi * f * vib * t
        v = math.sin(ph) + 0.08 * math.sin(2 * ph) + 0.015 * rng.uniform(-1, 1)  # a little breath
        mix.add(i0 + k, 0.13 * env(t, length, 0.03, 0.09) * trem * v, pan)

def meow(mix, t0, f0, length=0.75, amp=0.32, pan=0.0):
    """A cat: a pitch that scoops up then slides down, with two vowel formants ('mee-ow')."""
    i0 = int(t0 * RATE)
    n = int(length * RATE)
    ph = 0.0
    for k in range(n):
        t = k / RATE
        x = t / length
        if x < 0.3: glide = 0.9 + 0.45 * (x / 0.3)          # scoop up
        else: glide = 1.35 - 0.75 * ((x - 0.3) / 0.7) ** 0.8  # slide down
        f = f0 * glide * (1 + 0.012 * math.sin(2 * math.pi * 6.5 * t))
        ph += 2 * math.pi * f / RATE
        # formants: "ee" (300/2300 Hz) into "ow" (600/1000 Hz)
        F1 = 300 + 300 * x
        F2 = 2300 - 1300 * x
        v = 0.0
        for h in range(1, 13):
            fh = f * h
            w = (1 / h) * (1.0 + 2.0 * math.exp(-((fh - F1) / 160) ** 2) + 1.6 * math.exp(-((fh - F2) / 260) ** 2))
            v += w * math.sin(h * ph)
        e = env(t, length, 0.06, 0.18) * (0.6 + 0.4 * math.sin(math.pi * x))
        mix.add(i0 + k, amp * 0.22 * e * v, pan)

def purr(mix, t0, length=3.0, amp=0.25):
    i0 = int(t0 * RATE)
    last = 0.0
    for k in range(int(length * RATE)):
        t = k / RATE
        gate = 0.5 + 0.5 * math.cos(2 * math.pi * 26 * t)
        gate = gate ** 3
        breath = 0.5 + 0.5 * math.sin(2 * math.pi * 0.9 * t)  # in and out
        w = rng.uniform(-1, 1)
        last = 0.93 * last + 0.07 * w  # a very low rumble
        v = last * 6 * gate * (0.4 + 0.6 * breath)
        mix.add(i0 + k, amp * env(t, length, 0.3, 0.8) * v, 0.0)

def crackle(mix, seconds):
    t = 0.0
    while t < seconds:
        t += rng.expovariate(5.0)
        i0 = int(t * RATE)
        a = rng.uniform(0.02, 0.09)
        for k in range(int(0.004 * RATE)):
            mix.add(i0 + k, a * rng.uniform(-1, 1) * (1 - k / (0.004 * RATE)), rng.uniform(-0.6, 0.6))

def render(seconds):
    mix = Mix(seconds + 2)
    bars = int(seconds / BAR) + 1
    for b in range(bars):
        t_bar = b * BAR
        root, voicing = CHORDS[b % 4]
        # guitar: the first two bars come in alone, then everyone
        for (beat, direction, accent) in STRUMS:
            strum(mix, t_bar + beat * BEAT, voicing, direction, accent)
        if b >= 2:
            bass(mix, t_bar, root, BEAT * 1.6)
            bass(mix, t_bar + 2 * BEAT, root + 7 if b % 4 != 3 else root, BEAT * 0.9)
            bass(mix, t_bar + 3.5 * BEAT, root + 12, BEAT * 0.45)
            for (beat, a) in CLAPS:
                clap(mix, t_bar + beat * BEAT, a)
            if b % 2:
                clap(mix, t_bar + 2.5 * BEAT, 0.6)
        # lead from bar 4
        if b >= 4:
            phrase = PHRASE_A if ((b - 4) // 4) % 2 == 0 else PHRASE_B
            seg = phrase[((b - 4) % 4) * 16:((b - 4) % 4) * 16 + 16]
            for s, n in enumerate(seg):
                if not n: continue
                # length: until the next note or rest, max a beat and a half
                length = 1
                while s + length < 16 and seg[s + length] == 0 and length < 3: length += 1
                whistle(mix, t_bar + s * BEAT / 2, n, length * BEAT / 2 * 0.95, pan=0.35 if b % 8 < 4 else 0.2)
        # the cat answers the E7 bar
        if b % 4 == 3 and b >= 3:
            kind = (b // 4) % 3
            if kind == 0:
                meow(mix, t_bar + 2.6 * BEAT, hz(64), 0.8, pan=0.45)
            elif kind == 1:
                meow(mix, t_bar + 2.3 * BEAT, hz(64), 0.5, amp=0.28, pan=-0.5)
                meow(mix, t_bar + 3.1 * BEAT, hz(68), 0.65, amp=0.3, pan=0.5)
            else:
                meow(mix, t_bar + 2.5 * BEAT, hz(59), 1.05, amp=0.34, pan=0.0)
    crackle(mix, seconds)
    purr(mix, max(0.0, seconds - 3.6), 3.4)
    return mix

def write(path, seconds):
    mix = render(seconds)
    n = int(seconds * RATE)
    fade_in = int(1.0 * RATE)
    fade_out = int(2.6 * RATE)
    frames = bytearray()
    for i in range(n):
        g = 1.0
        if i < fade_in: g = i / fade_in
        if i > n - fade_out: g = min(g, (n - i) / fade_out)
        l = math.tanh(mix.L[i] * 1.4) * 0.55 * g
        r = math.tanh(mix.R[i] * 1.4) * 0.55 * g
        frames += struct.pack("<hh", int(max(-1, min(1, l)) * 32767), int(max(-1, min(1, r)) * 32767))
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(bytes(frames))

if __name__ == "__main__":
    write(sys.argv[1], float(sys.argv[2]))

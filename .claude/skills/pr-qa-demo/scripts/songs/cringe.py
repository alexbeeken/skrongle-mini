# "Cringe" (first heard on PR #656): deliberately cringe "corporate celebration" music, standard library only.
# Out-of-tune kazoo lead that scoops into every note, a squeaky recorder
# harmony, ukulele plucks, an oom-pah tuba, MORE cowbell, air horns, a slide
# whistle into a key change up a semitone every eight bars, and a sad trombone
# to finish. It is timed to the video: horns over the 2.6 s title card,
# the trombone in the last 3.6 s. Usage: python3 -I cringe.py out.wav <seconds>
import math, random, struct, sys, wave

RATE = 44100
BPM = 112
BEAT = 60 / BPM
EIGHTH = BEAT / 2
BAR = BEAT * 4
rng = random.Random(656)

def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)

def env(t, length, a=0.01, r=0.06):
    if t < a:
        return t / a
    if t > length - r:
        return max(0.0, (length - t) / r)
    return 1.0

class Mix:
    def __init__(self, seconds):
        self.n = int(seconds * RATE)
        self.buf = [0.0] * self.n

    def add(self, t0, length, fn, gain):
        i0 = int(t0 * RATE)
        for k in range(int(length * RATE)):
            i = i0 + k
            if i >= self.n:
                return
            self.buf[i] += gain * fn(k / RATE)

def saw(ph):
    return 2 * (ph % 1.0) - 1

def kazoo(m, length):
    # Scoops up from 70 cents flat, misses the pitch by up to 35 cents,
    # nervous 6 Hz vibrato, nasal buzz from a saw times a fast pulse.
    miss = rng.uniform(-0.35, 0.35)
    state = {"ph": 0.0, "lp": 0.0}
    def fn(t):
        scoop = -0.7 * math.exp(-t * 18)
        f = hz(m + miss + scoop + 0.25 * math.sin(2 * math.pi * 6.2 * t))
        state["ph"] += f / RATE
        x = saw(state["ph"]) * (0.6 + 0.4 * (1 if (state["ph"] * 3) % 1 < 0.5 else -1))
        state["lp"] += 0.35 * (x - state["lp"])
        return env(t, length, 0.02, 0.05) * state["lp"]
    return fn

def recorder(m, length):
    squeak = rng.random() < 0.12
    if squeak:
        m += 19  # the octave-and-a-fifth squeak every school concert has
    miss = rng.uniform(-0.3, 0.3)
    state = {"ph": 0.0}
    def fn(t):
        f = hz(m + miss + 0.08 * math.sin(2 * math.pi * 4.5 * t))
        state["ph"] += f / RATE
        breath = (rng.random() * 2 - 1) * 0.25
        return env(t, length, 0.03, 0.05) * (math.sin(2 * math.pi * state["ph"]) + breath) * (1.4 if squeak else 1)
    return fn

def uke(m, length):
    period = max(2, int(RATE / hz(m)))
    buf = [rng.uniform(-1, 1) for _ in range(period)]
    state = {"i": 0}
    def fn(t):
        i = state["i"] % period
        j = (i + 1) % period
        v = buf[i]
        buf[i] = 0.497 * (buf[i] + buf[j])
        state["i"] += 1
        return v * env(t, length, 0.001, 0.05)
    return fn

def tuba(m, length):
    def fn(t):
        f = hz(m)
        return env(t, length, 0.015, 0.08) * (math.sin(2 * math.pi * f * t) + 0.3 * math.sin(4 * math.pi * f * t) + 0.15 * (1 if (f * t) % 1 < 0.5 else -1))
    return fn

def noise(length, decay):
    return lambda t: (rng.random() * 2 - 1) * math.exp(-t * decay) * env(t, length, 0.001, 0.02)

def kick(t):
    return math.sin(2 * math.pi * (55 + 90 * math.exp(-t * 30)) * t) * math.exp(-t * 14)

def cowbell(t):
    sq = lambda f: 1 if (f * t) % 1 < 0.5 else -1
    return (sq(562) + sq(845)) * 0.5 * math.exp(-t * 16)

def slide_whistle(length, lo, hi):
    state = {"ph": 0.0}
    def fn(t):
        f = hz(lo + (hi - lo) * (t / length) ** 0.8 + 0.3 * math.sin(2 * math.pi * 7 * t))
        state["ph"] += f / RATE
        return env(t, length, 0.03, 0.05) * (math.sin(2 * math.pi * state["ph"]) + 0.1 * (rng.random() * 2 - 1))
    return fn

def air_horn(length, m=70):
    fs = [hz(m) * 0.997, hz(m) * 1.004, hz(m + 4) * 1.01, hz(m + 7) * 0.993]
    def fn(t):
        drop = 1 - 0.04 * min(1, t / 0.3)
        x = sum(saw(f * drop * t) for f in fs) / 2
        return env(t, length, 0.01, 0.04) * max(-1, min(1, 2.2 * x))
    return fn

def trombone(m, length, wah):
    state = {"ph": 0.0}
    def fn(t):
        bend = -0.5 * (t / length) ** 2 if wah else 0
        f = hz(m + bend + (0.35 * math.sin(2 * math.pi * 5 * t) if wah else 0.05 * math.sin(2 * math.pi * 5 * t)))
        state["ph"] += f / RATE
        mute = 0.5 + 0.5 * math.sin(2 * math.pi * 2.2 * t) if wah else 0.75
        x = saw(state["ph"]) * mute + 0.5 * math.sin(2 * math.pi * state["ph"])
        return env(t, length, 0.03, 0.12) * x
    return fn

C4, D4, E4, F4, G4, A4, B4 = 60, 62, 64, 65, 67, 69, 71
C5, D5, E5, F5, G5 = 72, 74, 76, 77, 79
# Four bars, eighth notes, over C G Am F. 0 = rest.
MELODY = [
    E5, E5, G5, E5, C5, 0, D5, E5,
    D5, D5, B4, G4, 0, G4, A4, B4,
    C5, C5, E5, C5, A4, 0, A4, C5,
    F5, E5, D5, C5, A4, C5, D5, 0,
]
CHORDS = [(C4, E4, G4), (G4 - 12, B4 - 12, D4), (A4 - 12, C4, E4), (F4 - 12, A4 - 12, C4)]
ROOTS = [C4 - 24, G4 - 36, A4 - 36, F4 - 36]

def render(seconds):
    mix = Mix(seconds)
    intro = 2.6          # the title card: horns only
    sad = 3.6            # the sad trombone at the end
    music_end = seconds - sad
    # Intro: air horn "bwa bwa bwaaaa".
    for t0, l in [(0.25, 0.14), (0.5, 0.14), (0.78, 0.9)]:
        mix.add(t0, l, air_horn(l), 0.16)
    mix.add(1.9, 0.6, slide_whistle(0.6, 62, 86), 0.12)
    bars = int((music_end - intro) / BAR)
    for b in range(bars):
        t_bar = intro + b * BAR
        section = b // 8
        key = section  # up a semitone every eight bars, every time
        if b % 8 == 0 and section > 0:
            # The truck driver's gear change, announced.
            mix.add(t_bar - 0.7, 0.65, slide_whistle(0.65, 60 + key, 84 + key), 0.13)
            for t0, l in [(t_bar, 0.12), (t_bar + 0.18, 0.12), (t_bar + 0.38, 0.55)]:
                mix.add(t0, l, air_horn(l, 70 + key), 0.11)
        ci = b % 4
        for s in range(8):
            t = t_bar + s * EIGHTH
            m = MELODY[ci * 8 + s]
            if m:
                mix.add(t, EIGHTH * 0.92, kazoo(m + key - 12, EIGHTH * 0.92), 0.17)
                if section >= 1:
                    mix.add(t, EIGHTH * 0.85, recorder(m + key + 4, EIGHTH * 0.85), 0.07)
            if s % 2 == 1:  # ukulele on the off-beats
                for k, cm in enumerate(CHORDS[ci]):
                    mix.add(t + k * 0.012, EIGHTH * 0.9, uke(cm + key + 12, EIGHTH * 0.9), 0.09)
        for beat in range(4):
            t = t_bar + beat * BEAT
            if beat % 2 == 0:  # oom
                mix.add(t, BEAT * 0.7, tuba(ROOTS[ci] + key + (7 if beat == 2 else 0), BEAT * 0.7), 0.2)
                mix.add(t, 0.25, kick, 0.3)
            else:  # pah
                mix.add(t, 0.12, noise(0.12, 30), 0.14)
            mix.add(t, 0.2, cowbell, 0.07)  # more cowbell
            mix.add(t + EIGHTH, 0.2, cowbell, 0.045)
    # Sad trombone: wah, wah, wah, waaaaah.
    t0 = intro + bars * BAR + 0.2
    for i, (m, l) in enumerate([(55, 0.5), (54, 0.5), (53, 0.5), (52, 1.7)]):
        mix.add(t0, l, trombone(m, l, i == 3), 0.2)
        t0 += l + 0.06
    peak = max(abs(v) for v in mix.buf) or 1
    return [math.tanh(1.2 * v / peak) * 0.62 for v in mix.buf]

def write(path, seconds):
    s = render(seconds)
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for i, v in enumerate(s):
            r = s[i - 400] if i >= 400 else 0.0
            frames += struct.pack("<hh", int(v * 32767), int((0.7 * r + 0.3 * v) * 32767))
        w.writeframes(bytes(frames))

if __name__ == "__main__":
    write(sys.argv[1], float(sys.argv[2]))

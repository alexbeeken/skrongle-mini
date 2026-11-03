# "Silent night, drifting": Gruber's Silent Night (1818, public domain) for
# four voices at a brisk double tempo, in two keys at once: soprano and alto
# in C, tenor and bass a tritone away in F# (LOW_KEY). Their notes are not
# quite the same length either: the alto's are 3 % shorter than the
# soprano's, the tenor's 6 %, the bass's 9 %. They start together and come
# apart further with every bar; a voice that finishes starts the carol again.
# On the last 4 s all four land on one final chord, still in their two keys,
# under a church bell, which also tolls over the 2.6 s title card.
# Standard library only.
#
#   python3 -I silent-night-drift.py track.wav 62
import math, random, sys, wave
from array import array

RATE = 22050
INTRO = 2.6
OUTRO = 4.0
TABLE = 2048
BPM = 144          # twice a carol's usual 72
LOW_KEY = -6       # tenor and bass, in semitones from the upper voices' C
rng = random.Random(1818)

def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)

NAMES = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}

def note(s):
    n, i = NAMES[s[0]], 1
    while s[i] in "#b":
        n += 1 if s[i] == "#" else -1
        i += 1
    return n + 12 * (int(s[i:]) + 1)

# The tune in C, a bar of three beats per group, with the chord of each bar.
TUNE = [
    ("C", "G4:1.5 A4:.5 G4:1"), ("C", "E4:3"), ("C", "G4:1.5 A4:.5 G4:1"), ("C", "E4:3"),
    ("G7", "D5:2 D5:1"), ("G7", "B4:3"), ("C", "C5:2 C5:1"), ("C", "G4:3"),
    ("F", "A4:2 A4:1"), ("F", "C5:1.5 B4:.5 A4:1"), ("C", "G4:1.5 A4:.5 G4:1"), ("C", "E4:3"),
    ("F", "A4:2 A4:1"), ("F", "C5:1.5 B4:.5 A4:1"), ("C", "G4:1.5 A4:.5 G4:1"), ("C", "E4:3"),
    ("G7", "D5:2 D5:1"), ("G7", "F5:1.5 D5:.5 B4:1"), ("C", "C5:3"), ("C", "E5:3"),
    ("C", "C5:1 G4:1 E4:1"), ("G7", "G4:1.5 F4:.5 D4:1"), ("C", "C4:3"), ("C", "C4:3"),
]
CHORD = {"C": (0, 4, 7), "F": (5, 9, 0), "G7": (7, 11, 2, 5)}
BASS_ROOT = {"C": 48, "F": 41, "G7": 43}

def below(pitch, chord, lo, hi, avoid=()):
    """The highest chord tone at least a minor third under `pitch`, within
    lo..hi, preferring pitch classes not already sung."""
    tones = [m for m in range(lo, hi + 1) if m % 12 in CHORD[chord] and m <= pitch - 3]
    fresh = [m for m in tones if m % 12 not in avoid]
    return max(fresh or tones or [lo])

def parts():
    """The four lines, as (midi, beats) lists: the tune, and for each note
    an alto, tenor and bass note from that bar's chord."""
    s, a, t, b = [], [], [], []
    for chord, spec in TUNE:
        for tok in spec.split():
            name, beats = tok.split(":")
            m, d = note(name), float(beats)
            alto = below(m, chord, 55, 72)
            tenor = below(alto, chord, 48, 64, avoid={m % 12, alto % 12})
            bass = BASS_ROOT[chord] if d >= 1 or chord != "G7" else BASS_ROOT[chord] - 5
            s.append((m, d)); a.append((alto, d)); t.append((tenor, d)); b.append((bass, d))
    return s, a, t, b

def wavetable(harmonics):
    t = array("f", [0.0]) * TABLE
    for n, amp in enumerate(harmonics, 1):
        for i in range(TABLE):
            t[i] += amp * math.sin(2 * math.pi * n * i / TABLE)
    peak = max(abs(v) for v in t) or 1
    for i in range(TABLE):
        t[i] /= peak
    return t

# Soft "aah" voices: a strong fundamental, a little breath in the overtones.
SOPRANO = wavetable([1, .35, .2, .08, .04])
ALTO = wavetable([1, .45, .28, .12, .06, .03])
TENOR = wavetable([1, .55, .4, .22, .12, .06, .03])
BASS = wavetable([1, .7, .5, .3, .18, .1, .05, .03])

def voice(buf, t0, dur, midi, tab, gain, attack=0.09, release=0.22, vib=0.006, cents=0.0):
    inc = hz(midi) * 2 ** (cents / 1200) * TABLE / RATE
    i0 = int(t0 * RATE)
    if i0 >= len(buf):
        return
    held = dur * RATE
    a = max(1, int(attack * RATE))
    r = max(1, int(release * RATE))
    end = min(int(held) + r, len(buf) - i0)
    ph = rng.random() * TABLE
    vrate = 2 * math.pi * (4.8 + rng.random()) / RATE
    vstart = int(0.2 * RATE)
    for k in range(end):
        e = k / a if k < a else (1.0 if k < held else 1 - (k - held) / r)
        ph += inc * (1 + vib * math.sin(vrate * k)) if k > vstart else inc
        buf[i0 + k] += gain * e * tab[int(ph) & 2047]

def bell(buf, t0, f0, gain, length=4.0):
    i0 = int(t0 * RATE)
    n = max(0, min(int(length * RATE), len(buf) - i0))
    for ratio, amp, dec in [(0.5, .5, 3.0), (1, 1.0, 2.2), (1.19, .4, 1.6), (1.5, .35, 1.3), (2.0, .3, 1.0), (2.74, .2, .6)]:
        w = 2 * math.pi * f0 * ratio / RATE
        for k in range(n):
            buf[i0 + k] += gain * amp * math.exp(-k / RATE / dec) * math.sin(w * k)

def sing(buf, line, scale, spb, t0, t1, tab, gain, key=0):
    """Sing `line` from t0, each note `scale` times as long as written,
    round and round, stopping at t1, `key` semitones from C."""
    t = t0
    while t < t1:
        for m, beats in line:
            if t >= t1:
                return
            d = min(beats * spb * scale, t1 - t)
            voice(buf, t, d * 0.97, m + key, tab, gain)
            t += beats * spb * scale

def render(seconds):
    n = int(seconds * RATE)
    voices = [array("f", [0.0]) * n for _ in range(4)]
    bells = array("f", [0.0]) * n
    bell(bells, 0.1, 392.0, 0.16)
    bell(bells, 1.4, 392.0, 0.12)
    body = max(0.0, seconds - INTRO - OUTRO)
    s, a, t, b = parts()
    beats = sum(d for _, d in s)
    # Whole verses for the soprano where the tempo allows (BPM -25 % / +30 %);
    # otherwise the final chord cuts in.
    verses = max(1, round(body / (beats * 60 / BPM)))
    spb = min(60 / (BPM * 0.75), max(60 / (BPM * 1.3), body / (verses * beats)))
    end = INTRO + body
    for buf, line, scale, tab, gain, key in zip(voices, (s, a, t, b), (1.0, 0.97, 0.94, 0.91),
                                               (SOPRANO, ALTO, TENOR, BASS), (.2, .16, .15, .17),
                                               (0, 0, LOW_KEY, LOW_KEY)):
        if body > 0:
            sing(buf, line, scale, spb, INTRO, end, tab, gain, key)
    # All four meet on the last chord, the bell above them.
    hold = max(1.0, seconds - end - 0.7)
    for buf, m, tab, gain in zip(voices, (72, 64, 55 + LOW_KEY, 48 + LOW_KEY), (SOPRANO, ALTO, TENOR, BASS), (.2, .16, .15, .17)):
        voice(buf, end + 0.05, hold, m, tab, gain, 0.25, 0.6)
    bell(bells, end + 0.05, 392.0, 0.14)
    # Soprano and tenor a little left, alto and bass a little right, in a stone church.
    pans = (0.35, 0.65, 0.3, 0.7)
    left = array("f", [0.0]) * n
    right = array("f", [0.0]) * n
    for buf, p in zip(voices, pans):
        gl, gr = math.cos(p * math.pi / 2), math.sin(p * math.pi / 2)
        for i in range(n):
            left[i] += gl * buf[i]
            right[i] += gr * buf[i]
    for i in range(n):
        left[i] += 0.7 * bells[i]
        right[i] += 0.7 * bells[i]
    combs = [(int(RATE * d), fb) for d, fb in ((0.0311, .84), (0.0383, .82), (0.0427, .8), (0.0479, .78))]
    for ch, offset in ((left, 0), (right, 2)):
        lines = [array("f", [0.0]) * (size + offset * 37) for size, _ in combs]
        pos = [0] * len(combs)
        for i in range(n):
            x = ch[i]
            acc = 0.0
            for j, (_, fb) in enumerate(combs):
                line = lines[j]
                q = pos[j]
                y = line[q]
                line[q] = x + fb * y
                pos[j] = q + 1 if q + 1 < len(line) else 0
                acc += y
            ch[i] = x + 0.1 * acc
    peak = max(max(abs(v) for v in left), max(abs(v) for v in right)) or 1
    fade = int(0.5 * RATE)
    for i in range(n):
        e = min(1.0, (n - i) / fade)
        left[i] = math.tanh(1.1 * left[i] / peak) * 0.7 * e
        right[i] = math.tanh(1.1 * right[i] / peak) * 0.7 * e
    return left, right

def write(path, seconds):
    left, right = render(seconds)
    frames = array("h", [0]) * (2 * len(left))
    frames[0::2] = array("h", (int(v * 32767) for v in left))
    frames[1::2] = array("h", (int(v * 32767) for v in right))
    if sys.byteorder == "big":
        frames.byteswap()
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(frames.tobytes())

if __name__ == "__main__":
    write(sys.argv[1], float(sys.argv[2]))

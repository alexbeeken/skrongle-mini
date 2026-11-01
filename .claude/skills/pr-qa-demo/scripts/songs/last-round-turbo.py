# "Last round turbo": a sad boxing-game orchestral medley of public-domain
# melodies (Chopin's funeral march, Dvorak's "Going Home", Greensleeves,
# Amazing Grace, Beethoven's Ode to Joy as the comeback; a ring bell and
# timpani) at a brisk double tempo, changing key every third beat.
# The keys walk the circle of fifths (C, G, D, A, ... folded to stay within
# half an octave of home), so the tune never climbs out of range. In the 4/4
# melodies a change every three beats drifts across the bar lines by itself;
# in the 3/4 ones (Greensleeves, Amazing Grace) the changes sit on beat 2,
# off the bar line. The final chord comes home to C. Standard library only.
#
#   python3 -I last-round-turbo.py track.wav 62
#
# Timed to the video like "Last round": bells over the 2.6 s title card,
# whole melodies stretched a little to fill the time, the Ode last, the
# final chord and three bells in the last 4 s.
import math, random, sys, wave
from array import array

RATE = 22050
INTRO = 2.6
OUTRO = 4.0
TABLE = 2048
SPEED = 2
rng = random.Random(1987)

def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)

NAMES = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}

def note(s):
    n, i = NAMES[s[0]], 1
    while s[i] in "#b":
        n += 1 if s[i] == "#" else -1
        i += 1
    return n + 12 * (int(s[i:]) + 1)

def melody(spec):
    out = []
    for tok in spec.split():
        if tok == "|":
            continue
        name, beats = tok.split(":")
        out.append((None if name == "R" else note(name), float(beats)))
    return out

def chords(spec):
    out = []
    for tok in spec.split():
        name, beats = tok.split(":")
        i = 2 if len(name) > 1 and name[1] in "#b" else 1
        root = 48 + (NAMES[name[0]] + (name[1:i] == "#") - (name[1:i] == "b")) % 12
        out.append((root, 3 if name[i:] == "m" else 4, float(beats)))
    return out

def wavetable(harmonics):
    t = array("f", [0.0]) * TABLE
    for n, a in enumerate(harmonics, 1):
        for i in range(TABLE):
            t[i] += a * math.sin(2 * math.pi * n * i / TABLE)
    peak = max(abs(v) for v in t) or 1
    for i in range(TABLE):
        t[i] /= peak
    return t

STRINGS = wavetable([1, .5, .33, .25, .2, .16, .13, .11, .09, .08])
HORN = wavetable([1, .55, .3, .16, .08, .04])
BRASS = wavetable([1, .85, .65, .5, .38, .28, .2, .13, .08])
HARP = wavetable([1, .35, .15, .07, .03])
BASS = wavetable([1, .6, .35, .2, .1])

def voice(buf, t0, dur, midi, tab, gain, attack=0.08, release=0.25, vib=0.0, decay=0.0, cents=0.0):
    inc = hz(midi) * 2 ** (cents / 1200) * TABLE / RATE
    i0 = int(t0 * RATE)
    if i0 >= len(buf):
        return
    held = dur * RATE
    a = max(1, int(attack * RATE))
    r = max(1, int(release * RATE))
    end = min(int(held) + r, len(buf) - i0)
    ph = rng.random() * TABLE
    vrate = 2 * math.pi * 5.2 / RATE
    vstart = int(0.3 * RATE)
    kd = math.exp(-1 / (decay * RATE)) if decay else 1.0
    d = 1.0
    for k in range(end):
        if k < a:
            e = k / a
        elif k < held:
            e = 1.0
        else:
            e = 1 - (k - held) / r
        if decay:
            d *= kd
        if vib and k > vstart:
            ph += inc * (1 + vib * math.sin(vrate * k))
        else:
            ph += inc
        buf[i0 + k] += gain * e * d * tab[int(ph) & 2047]

def timpani(buf, t0, midi, gain, length=1.4):
    f0 = hz(midi)
    i0 = int(t0 * RATE)
    ph = 0.0
    for k in range(max(0, min(int(length * RATE), len(buf) - i0))):
        t = k / RATE
        ph += 2 * math.pi * f0 * (1 + 0.12 * math.exp(-t * 30)) / RATE
        body = math.exp(-t * 3.0) * (math.sin(ph) + 0.35 * math.sin(1.5 * ph))
        thump = math.exp(-t * 70) * (rng.random() * 2 - 1) * 0.6
        buf[i0 + k] += gain * (body + thump)

def roll(buf, t0, t1, midi, g0, g1):
    t = t0
    while t < t1:
        x = (t - t0) / max(1e-6, t1 - t0)
        timpani(buf, t, midi, (g0 + (g1 - g0) * x) * (0.8 + 0.4 * rng.random()), 0.5)
        t += 0.065

def bell(buf, t0, gain):
    f0 = 1180.0
    i0 = int(t0 * RATE)
    n = max(0, min(int(2.2 * RATE), len(buf) - i0))
    for ratio, amp, dec in [(1, 1.0, 1.4), (2.0, .55, 1.0), (2.76, .5, .8), (5.4, .28, .45), (8.93, .18, .25)]:
        w = 2 * math.pi * f0 * ratio / RATE
        for k in range(n):
            buf[i0 + k] += gain * amp * math.exp(-k / RATE / dec) * math.sin(w * k)
    for k in range(min(n, int(0.01 * RATE))):
        buf[i0 + k] += gain * 0.8 * (rng.random() * 2 - 1) * (1 - k / (0.01 * RATE))

def crowd(buf, t0, t1, gain):
    i0, i1 = int(t0 * RATE), min(int(t1 * RATE), len(buf))
    lp = 0.0
    for i in range(max(0, i0), i1):
        x = (i - i0) / max(1, i1 - i0)
        lp += 0.08 * ((rng.random() * 2 - 1) - lp)
        buf[i] += gain * math.sin(math.pi * x) * lp

# The rounds. bpm, melody, chords, the lead's layers (table, octave shift,
# gain, vibrato, attack), pad gain, harp arpeggios, march timpani.
ROUNDS = {
    "funeral": dict(bpm=56, gain=0.9,
        melody=melody("A3:1 A3:.75 A3:.25 A3:2 | C4:.75 B3:.25 B3:.75 A3:.25 A3:.75 G#3:.25 A3:1"),
        chords=chords("Am:4 Am:2 E:1 Am:1"),
        lead=[(HORN, 0, .2, .004, .05), (STRINGS, -12, .08, 0, .05)], pad=.035, march=True),
    "going-home": dict(bpm=66, gain=0.9,
        melody=melody("E4:1.5 G4:.5 G4:2 | E4:1.5 D4:.5 C4:2 | D4:1 E4:1 D4:1 C4:1 | C4:4"),
        chords=chords("C:4 Am:4 F:2 G:2 C:4"),
        lead=[(HORN, 0, .22, .005, .08)], pad=.04),
    "greensleeves": dict(bpm=84, gain=1.0,
        melody=melody("A4:1 | C5:2 D5:1 | E5:1.5 F5:.5 E5:1 | D5:2 B4:1 | G4:1.5 A4:.5 B4:1 | C5:1.5 B4:.5 A4:1 | G#4:1.5 F#4:.5 G#4:1 | A4:3"),
        chords=chords("Am:1 Am:3 C:3 G:3 G:3 Am:3 E:3 Am:3"),
        lead=[(STRINGS, 0, .16, .008, .1)], pad=.035, harp=True, meter=3),
    "amazing-grace": dict(bpm=78, gain=1.1,
        melody=melody("G4:1 | C5:2 E5:.5 C5:.5 | E5:2 D5:1 | C5:2 A4:1 | G4:2 G4:1 | C5:2 E5:.5 C5:.5 | E5:2 D5:1 | G5:3"),
        chords=chords("C:1 C:3 C:3 F:3 C:3 C:3 Am:3 G:3"),
        lead=[(STRINGS, 0, .15, .007, .12), (HORN, -12, .1, .004, .1)], pad=.05, meter=3),
    "ode": dict(bpm=76, gain=1.2,
        melody=melody("E5:1 E5:1 F5:1 G5:1 | G5:1 F5:1 E5:1 D5:1 | C5:1 C5:1 D5:1 E5:1 | E5:1.5 D5:.5 D5:2"),
        chords=chords("C:4 G:4 Am:4 G:4"),
        lead=[(BRASS, -12, .16, .003, .04), (STRINGS, 0, .1, .006, .08)], pad=.05, downbeats=True),
}
FINALE = dict(ROUNDS["ode"],
    melody=melody("E5:1 E5:1 F5:1 G5:1 | G5:1 F5:1 E5:1 D5:1 | C5:1 C5:1 D5:1 E5:1 | D5:1.5 C5:.5 C5:2"),
    chords=chords("C:4 G:4 C:4 G:2 C:2"), gain=1.3)
ORDER = ["funeral", "going-home", "greensleeves", "amazing-grace", "ode"]

def length(r):
    return sum(b for _, b in r["melody"]) * 60 / (r["bpm"] * SPEED)

def plan(body):
    """The rounds before the finale and the tempo stretch. At twice the
    speed a video needs the sad rounds more than once, so they come round
    in order (funeral, going home, greensleeves, amazing grace, funeral, ...),
    as many as fit with the stretch between 0.85 and 1.2, then the Ode's
    first half if there is room."""
    fin = length(FINALE)
    sad = ORDER[:-1]
    best = None
    for k in range(4 * len(sad) + 1):
        for ode in ([], ["ode"]):
            names = [sad[i % len(sad)] for i in range(k)] + ode
            s = body / (sum(length(ROUNDS[n]) for n in names) + fin)
            score = (0.85 <= s <= 1.2, -abs(math.log(s)))
            if best is None or score > best[0]:
                best = (score, names, s)
    _, names, s = best
    return names, max(0.5, min(1.2, s))

# Where the keys are: changes so far, and the song's beat count.
KEYS = {"changes": 0, "beat": 0.0}

def shift(k):
    """The k-th key on the circle of fifths, as semitones within -6..+5 of C."""
    return (7 * k + 6) % 12 - 6

def changes(r):
    """The beats (counted from the start of round r) where the key changes:
    every third beat of the whole song, or, in 3/4, beat 2 of every bar."""
    total = sum(b for _, b in r["melody"])
    if r.get("meter") == 3:
        first = sum(b for _, b in r["melody"][:1]) + 1  # the pickup, then beat 2
    else:
        first = (-KEYS["beat"]) % 3 or 3
    out = []
    b = first
    while b < total:
        out.append(b)
        b += 3
    return out

def play(buf, r, t0, stretch, first):
    spb = 60 / (r["bpm"] * SPEED) * stretch
    g = r["gain"]
    marks = changes(r)
    key0 = KEYS["changes"]

    def tr(beat):
        return shift(key0 + sum(1 for m in marks if m <= beat + 1e-9))

    if not first:
        roll(buf, t0 - 1.0, t0, 33, 0.02, 0.09)
    timpani(buf, t0, 33 if r is not FINALE else 36, 0.22 * g)
    beat = 0.0
    for m, beats in r["melody"]:
        if m is not None:
            for tab, octave, gain, vib, att in r["lead"]:
                voice(buf, t0 + beat * spb, beats * spb * 0.95, m + octave + tr(beat), tab, gain * g, att, 0.15, vib)
        beat += beats
    beat = 0.0
    for root, third, beats in r["chords"]:
        # Split the chord where the key changes, so each piece sounds in its key.
        cuts = [beat] + [m for m in marks if beat < m < beat + beats] + [beat + beats]
        for a, b in zip(cuts, cuts[1:]):
            k = tr(a)
            t, d = t0 + a * spb, (b - a) * spb
            for iv in (0, third, 7, 12):
                for c in (-7, 7):
                    voice(buf, t, d, root + k + iv, STRINGS, r["pad"] * g, 0.12, 0.2, 0.003, cents=c)
            voice(buf, t, d, root + k - 12, BASS, 0.08 * g, 0.05, 0.15)
        if r.get("harp"):
            for j in range(int(beats * 2)):
                k = tr(beat + j / 2)
                arp = [root + 12, root + 12 + third, root + 19, root + 24]
                voice(buf, t0 + (beat + j / 2) * spb, 0.05, arp[j % 4] + k, HARP, 0.09 * g, 0.003, 0.05, decay=0.5)
        if r.get("march"):
            for j in range(int(beats)):
                timpani(buf, t0 + (beat + j) * spb, 33 + tr(beat + j), 0.09 * g)
        if r.get("downbeats"):
            base = root - 24 if root - 24 >= 33 else root - 12
            timpani(buf, t0 + beat * spb, base + tr(beat), 0.16 * g)
        beat += beats
    KEYS["changes"] = key0 + len(marks)
    KEYS["beat"] += beat
    return t0 + beat * spb

def render(seconds):
    n = int(seconds * RATE)
    buf = array("f", [0.0]) * n
    # Over the title card: "ding ding", then a timpani roll.
    bell(buf, 0.15, 0.18)
    bell(buf, 0.55, 0.18)
    body = seconds - INTRO - OUTRO
    t = INTRO
    if body > 4:
        roll(buf, 1.1, INTRO, 33, 0.01, 0.1)
        names, stretch = plan(body)
        for i, name in enumerate(names):
            t = play(buf, ROUNDS[name], t, stretch, i == 0)
        crowd(buf, t - 2.5, t + 1.5, 0.05)
        t = play(buf, FINALE, t, stretch, not names)
    # The final chord, a last roll and hit, and the bell three times.
    hold = max(1.0, seconds - t - 0.6)
    for iv in (0, 4, 7, 12, 16):
        for c in (-7, 7):
            voice(buf, t, hold, 48 + iv, STRINGS, 0.05, 0.2, 0.6, 0.003, cents=c)
    for m in (60, 64, 67):
        voice(buf, t, hold, m, BRASS, 0.07, 0.05, 0.6)
    voice(buf, t, hold, 36, BASS, 0.1, 0.05, 0.6)
    roll(buf, t, t + min(1.4, hold), 36, 0.04, 0.12)
    timpani(buf, t + min(1.4, hold), 36, 0.3)
    for k in range(3):
        bell(buf, t + 0.25 + 0.4 * k, 0.16)
    # The hall: four combs on the mix, the right side a little later.
    combs = [(int(RATE * d), fb) for d, fb in ((0.0297, .78), (0.0371, .76), (0.0411, .74), (0.0437, .72))]
    wet = array("f", [0.0]) * n
    lines = [array("f", [0.0]) * c for c, _ in combs]
    pos = [0] * len(combs)
    for i in range(n):
        x = buf[i]
        acc = 0.0
        for j, (size, fb) in enumerate(combs):
            line = lines[j]
            p = pos[j]
            y = line[p]
            line[p] = x + fb * y
            pos[j] = p + 1 if p + 1 < size else 0
            acc += y
        wet[i] = acc * 0.12
    lag = int(0.011 * RATE)
    peak = 1e-9
    left = array("f", [0.0]) * n
    right = array("f", [0.0]) * n
    for i in range(n):
        left[i] = buf[i] + wet[i]
        right[i] = buf[i] + (wet[i - lag] if i >= lag else 0.0)
        peak = max(peak, abs(left[i]), abs(right[i]))
    fade = int(0.4 * RATE)
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

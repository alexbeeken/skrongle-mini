# "Anxious Heart, reharmonised": Nobuo Uematsu's "Anxious Heart" (Final
# Fantasy VII, 1997, the slums theme) set the way Jacob Collier reharmonises
# a song. The tune and form come from three fan MIDI sequences on vgmusic.com
# (sigma01.mid, nibelheim.mid, Ganxiousheart.mid), which agree: F minor,
# about 68 bpm, one chord a bar rocking between F minor and D minor, then
# Ab and C major; a held note a bar in the melody (F, D, F to G, D, Eb, C,
# Eb to F, C) with a chromatic slide down before each D and C; the strings
# carry it first, then a piccolo over harp arpeggios, then a section on an
# F pedal with dyads moving above it.
#
# Here every bar gets new chords under that tune: close clusters (G A under
# a D, F G Bb under a C), the melody as a 9th, #11th or 13th, major chords a
# chromatic mediant away (Fm -> Dbmaj9, Dm -> Bbmaj13), a half-diminished
# and a dominant 13th on the way to Ab, Gb major under the Eb, and a cadence
# Ebmaj13 -> Dbmaj13 -> C13sus -> C6/9. The harp arpeggiates the new
# voicings; the pedal section turns its dyads into Fm11, F6/9, Bbmaj9/F and
# Dbmaj7/F. And every four bars the whole thing modulates: up a major
# third, up a major third, up a minor third, round and round through all
# twelve keys (F, A, Db, E, Ab, C, Eb, G, B, D, Gb, Bb), each key reached
# through a dominant on the last beat of the fourth bar, the V7 (or its
# tritone twin) of the key to come, voiced with whichever tensions let the
# note the tune is holding sit on top of it. All of it at twice the speed
# over a bluegrass backbeat: a plucked bass booming root and fifth on one
# and three, a mandolin chop on two and four, and the rolled maj13#11 of the
# last key to close. Standard library only.
#
#   python3 -I anxious-heart-reharm.py track.wav 62
import math, random, sys, wave
from array import array

RATE = 22050
INTRO = 2.6
OUTRO = 4.5
TABLE = 2048
BPM = 136           # twice the original
rng = random.Random(1997)

def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)

NAMES = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}

def note(s):
    n, i = NAMES[s[0]], 1
    while s[i] in "#b":
        n += 1 if s[i] == "#" else -1
        i += 1
    return n + 12 * (int(s[i:]) + 1)

# The tune as the strings hold it (A) and as the piccolo sings it (B, with
# the little chromatic slides). note:beats, bars separated by |.
MELODY_A = """
F5:4 | D5:4 | F5:3 G5:1 | D5:4 | Eb5:4 | C5:4 | Eb5:3 F5:1 | C5:4 | C5:4
"""
MELODY_B = """
F5:3.65 E5:.18 Eb5:.17 | D5:4 | F5:3 G5:.65 F5:.12 E5:.12 Eb5:.11 | D5:4 |
Eb5:4 | C5:4 | Eb5:3 F5:.65 Eb5:.12 D5:.12 C#5:.11 | C5:4 | C5:4
"""

# The reharmonisation: beats, chord name, voicing from the bass up. The
# melody of the moment sits above every voicing; the comment says what it is.
CHORDS_A = """
# bar 1 (was Fm)                                 melody
2 Fm9            F2 C3 Ab3 Eb4 G4 C5           # F  (1)
2 Dbmaj13#11/F   F2 C4 Eb4 G4 Bb4              # F  (3)
# bar 2 (was Dm)
2 Dm11           D2 A2 F3 C4 E4 G4             # D  (1)
2 Bbmaj13        Bb2 F3 A3 C4 G4            # D  (3)
# bar 3 (was Fm, the G at the end)
2 Dbmaj9         Db2 Ab2 C4 Eb4 Ab4            # F  (3)
1 Bbm11          Bb2 F3 Ab3 Db4 Eb4            # F  (5)
1 Ebmaj9         Eb2 Bb2 G3 D4 F4           # G  (3)
# bar 4 (was Dm)
2 Dm11           D2 A2 F3 C4 E4 G4             # D  (1)
2 Gm7b5          G2 Db3 F3 Bb3              # D  (5)
# bar 5 (was Cm)
2 Cm11           C2 G2 Eb3 Bb3 F4              # Eb (b3)
2 Gbmaj13        Gb2 Db3 F3 Bb3             # Eb (13)
# bar 6 (was C)
2 Am11           A2 E3 G3 D4                   # C  (b3)
2 Dbmaj13        Db2 Ab2 F3 Bb3             # C  (7)
# bar 7 (was Cm, the F at the end)
2 Cm11           C2 G2 Eb3 Bb3 F4              # Eb (b3)
1 Abmaj13        Ab2 Eb3 G3 C4 F4              # Eb (5)
1 Bbm11          Bb2 F3 Ab3 Db4 Eb4            # F  (5)
# bars 8-9 (was C, sus4 to 3): the cadence
2 Ebmaj13        Eb2 Bb2 G3 D4 F4           # C  (13)
2 Dbmaj13        Db2 Ab2 F3 Bb3 Eb4         # C  (7)
2 C13sus         C2 G2 D4 F4 A4                # C  (1)
2 C6/9           C2 G2 E3 A3 D4                # C  (1)
"""
CHORDS_B = """
# bar 1
2 Fm9            F2 C3 Ab3 Eb4 G4              # F
2 Dbmaj13#11/F   F2 C4 Eb4 G4 Bb4              # F
# bar 2
2 Dm11           D2 A2 F3 C4 E4 G4             # D
2 Bbmaj13        Bb2 F3 A3 C4 G4            # D
# bar 3
2 Dbmaj9         Db2 Ab2 C4 Eb4 Ab4            # F
1 Bbm11          Bb2 F3 Ab3 Db4 Eb4            # F
1 Ebmaj9         Eb2 Bb2 G3 D4 F4           # G
# bar 4
2 Dm11           D2 A2 F3 C4 E4 G4             # D
2 Bb13           Bb2 Ab3 D4 G4              # D  (3)
# bar 5 (was Ab)
2 Abmaj13        Ab2 Eb3 G3 C4 F4              # Eb (5)
2 Gbmaj13        Gb2 Db3 F3 Bb3             # Eb (13)
# bar 6 (was C)
2 Am11           A2 E3 G3 D4                   # C
2 Dbmaj13        Db2 Ab2 F3 Bb3             # C
# bar 7 (was Ab, the F at the end)
2 Abmaj13        Ab2 Eb3 G3 C4 F4              # Eb
1 Cm11           C2 G2 Eb3 Bb3 F4              # Eb
1 Bbm11          Bb2 F3 Ab3 Db4 Eb4            # F
# bars 8-9: the cadence
2 Ebmaj13        Eb2 Bb2 G3 D4 F4           # C
2 Dbmaj13        Db2 Ab2 F3 Bb3 Eb4         # C
2 C13sus         C2 G2 D4 F4 A4                # C
2 C6/9           C2 G2 E3 A3 D4                # C
"""
# The pedal section: the original's Ab-Eb, G-D, Bb-F dyads over F, C, F.
CHORDS_D = """
1 Fm11           F2 C3 Ab3 Bb3 Eb4 G4
1 F6/9           F2 C3 G3 A3 D4
1 Bbmaj9/F       F2 C3 A3 D4 F4
5 Fm9            F2 C3 Ab3 Eb4 G4
1 Fm11           F2 C3 Ab3 Bb3 Eb4 G4
1 F6/9           F2 C3 G3 A3 D4
1 Bbmaj9/F       F2 C3 A3 D4 F4
5 Dbmaj7/F       F2 C3 Ab3 Db4 F4
"""

def parse_melody(text):
    out = []
    for tok in text.replace("|", " ").split():
        name, beats = tok.split(":")
        out.append((None if name == "R" else note(name), float(beats)))
    return out

def parse_chords(text):
    out = []
    for line in text.splitlines():
        toks = []
        for tok in line.split():
            if tok.startswith("#"):   # a comment; chord names carry # inside
                break
            toks.append(tok)
        if toks:
            beats, _name, *names = toks
            out.append((float(beats), [note(n) for n in names]))
    return out

# name: (melody, chords, lead, harp, bass)
SECTIONS = {
    "A": (parse_melody(MELODY_A), parse_chords(CHORDS_A), "strings", False, "boom"),
    "B": (parse_melody(MELODY_B), parse_chords(CHORDS_B), "piccolo", True, "boom"),
    "D": (None, parse_chords(CHORDS_D), None, False, "boom"),
}
FORM = ["A", "B", "D"]

def beats_of(name):
    return sum(b for b, _ in SECTIONS[name][1])

def wavetable(harmonics):
    t = array("f", [0.0]) * TABLE
    for n, amp in enumerate(harmonics, 1):
        for i in range(TABLE):
            t[i] += amp * math.sin(2 * math.pi * n * i / TABLE)
    peak = max(abs(v) for v in t) or 1
    for i in range(TABLE):
        t[i] /= peak
    return t

STRINGS = wavetable([1, .5, .33, .25, .2, .16, .13, .11, .09])
PICCOLO = wavetable([1, .25, .08, .03])
HARP = wavetable([1, .35, .15, .07, .03])
MANDOLIN = wavetable([1, .8, .6, .45, .3, .2, .12, .07])
PLUCK = wavetable([1, .5, .2, .08])             # the upright bass

def voice(L, R, t0, dur, midi, tab, gain, pan, attack=0.08, release=0.25, vib=0.0, cents=0.0, decay=0.0):
    inc = hz(midi) * 2 ** (cents / 1200) * TABLE / RATE
    i0 = int(t0 * RATE)
    if i0 >= len(L) or dur <= 0:
        return
    held = dur * RATE
    a = max(1, int(attack * RATE))
    r = max(1, int(release * RATE))
    end = min(int(held) + r, len(L) - i0)
    gl, gr = gain * math.cos(pan * math.pi / 2), gain * math.sin(pan * math.pi / 2)
    ph = rng.random() * TABLE
    vrate = 2 * math.pi * (4.6 + 0.6 * rng.random()) / RATE
    vstart = int(0.25 * RATE)
    dk = 1.0 / (decay * RATE) if decay else 0.0
    for k in range(end):
        e = k / a if k < a else (1.0 if k < held else 1 - (k - held) / r)
        if dk:
            e *= math.exp(-k * dk)
        ph += inc * (1 + vib * math.sin(vrate * k)) if k > vstart else inc
        v = e * tab[int(ph) & 2047]
        L[i0 + k] += gl * v
        R[i0 + k] += gr * v

def bell(L, R, t0, f0, gain, pan=0.5, length=3.0):
    i0 = int(t0 * RATE)
    n = max(0, min(int(length * RATE), len(L) - i0))
    gl, gr = gain * math.cos(pan * math.pi / 2), gain * math.sin(pan * math.pi / 2)
    for ratio, amp, dec in [(1, 1.0, 1.6), (2.0, .35, 1.0), (3.01, .18, .7), (4.2, .08, .4)]:
        w = 2 * math.pi * f0 * ratio / RATE
        for k in range(n):
            v = amp * math.exp(-k / RATE / dec) * math.sin(w * k)
            L[i0 + k] += gl * v
            R[i0 + k] += gr * v

def pan_of(midi):
    """Low notes a little left, high notes a little right, like a stage."""
    return min(0.8, max(0.2, 0.2 + 0.6 * (midi - 36) / 48))

def pad(L, R, t0, dur, notes, gain, attack=0.12, release=0.3, strum=0.015):
    """A voicing on the strings, the bass note left to the bass, each note
    in two copies a few cents apart so the section sounds wide."""
    for i, m in enumerate(notes[1:]):
        g = gain * (0.92 + 0.16 * rng.random()) * (0.85 if m < 50 else 1.0)
        for cents in (-6.0, 6.0):
            voice(L, R, t0 + i * strum, dur, m, STRINGS, g / 2, pan_of(m), attack, release, 0.004, cents)

def harp(L, R, t0, beats, spb, notes, gain, up):
    """Eighth notes through the voicing and its octave, rising or falling."""
    pool = sorted(notes[1:])
    pool = pool + [m + 12 for m in pool]
    if not up:
        pool.reverse()
    for k in range(int(round(2 * beats))):
        m = pool[k % len(pool)]
        voice(L, R, t0 + k * spb / 2, spb * 0.9, m, HARP, gain * (0.85 + 0.3 * rng.random()), pan_of(m), 0.005, 0.3, 0.0, 0.0, 1.1)

def boom(L, R, t0, beat0, beats, spb, notes, gain):
    """The bluegrass bass: a plucked root on one, the fifth on three (the
    root again when the chord has no fifth), counted from the song's start
    so the alternation survives chord changes."""
    root = notes[0]
    pcs = {m % 12 for m in notes}
    fifth = root + 7 if (root + 7) % 12 in pcs else root
    q = math.ceil(beat0)
    while q < beat0 + beats:
        if q % 2 == 0:
            m = root if (q // 2) % 2 == 0 else fifth
            voice(L, R, t0 + (q - beat0) * spb, spb * 0.7, m, PLUCK, gain, 0.5, 0.005, 0.08, 0.0, 0.0, 0.35)
        q += 1

def chop(L, R, t0, beat0, beats, spb, notes, gain):
    """The mandolin chop on two and four: the voicing's upper notes, in the
    mandolin's register, cut short, with a little pick noise."""
    upper = [m + 12 if m < 55 else m for m in notes[1:]]
    q = math.ceil(beat0)
    while q < beat0 + beats:
        if q % 2 == 1:
            t = t0 + (q - beat0) * spb
            for i, m in enumerate(upper):
                voice(L, R, t + i * 0.004, 0.07, m, MANDOLIN, gain * (0.8 + 0.4 * rng.random()), 0.62, 0.003, 0.05, 0.0, 0.0, 0.09)
            i0 = int(t * RATE)
            for k in range(min(int(0.02 * RATE), len(L) - i0)):
                v = gain * 0.6 * (rng.random() * 2 - 1) * math.exp(-k / (0.004 * RATE))
                L[i0 + k] += 0.6 * v
                R[i0 + k] += 0.8 * v
        q += 1

BLOCK = 16                 # beats: a new key every four bars
KEY_STEPS = [4, 4, 3]      # Giant Steps twice, then a minor third

def key_of(block):
    """Semitones from F for the `block`th four bars, by the nearest way
    (-6..5), so the tune stays in its register."""
    k = sum(KEY_STEPS[i % len(KEY_STEPS)] for i in range(block)) % 12
    return k - 12 if k > 5 else k

def pivot(melody_midi, new_key):
    """The dominant on the last beat before `new_key` (semitones above F):
    its V7, or the tritone substitute when the tune's note would be the
    major seventh, as a sus chord when the note is the eleventh, otherwise
    with two tensions (9 and 13 first, then 9 and #11, then 13 and #11,
    then the altered ones) that leave no minor ninth under the tune."""
    root = (5 + new_key + 7) % 12
    rel = (melody_midi - root) % 12 if melody_midi is not None else 9
    if rel == 11:
        root = (root + 6) % 12
        rel = 5
    if rel == 5:
        pcs = [2, 5, 9, 10]
    else:
        pcs = [4, 10]
        for pair in ((2, 9), (2, 6), (9, 6), (2, 8), (1, 8)):
            if all((rel - x) % 12 != 1 for x in pair):
                pcs += list(pair)
                break
    bass = 36 + root
    upper, m = [], bass + 10
    for pc in sorted(pcs, key=lambda x: (x - 10) % 12):
        while m % 12 != (root + pc) % 12:
            m += 1
        upper.append(m)
    if melody_midi is not None and max(upper) >= melody_midi:
        upper = [m - 12 for m in upper]
    return [bass] + upper

def events(sections):
    """Flatten the plan into (start beat, beats, notes, section) chord
    events and (start beat, beats, midi, lead) melody notes."""
    chords, notes, t = [], [], 0.0
    for name in sections:
        melody, chs, lead, with_harp, bass = SECTIONS[name]
        b = t
        for beats, voicing in chs:
            chords.append((b, beats, voicing, name, b - t))
            b += beats
        if melody:
            b = t
            for m, beats in melody:
                if m is not None:
                    notes.append((b, beats, m, lead))
                b += beats
        t += beats_of(name)
    return chords, notes

def melody_at(notes, beat):
    for b, beats, m, _ in notes:
        if b <= beat < b + beats:
            return m
    return None

def play(L, R, t0, t1, spb, sections):
    """The whole plan from t0, cut at t1. Returns the key it ended in."""
    chords, notes = events(sections)
    key = 0
    for b, beats, voicing, name, rel in chords:
        _, _, _, with_harp, bass = SECTIONS[name]
        block = int(b // BLOCK)
        key = key_of(block)
        block_end = (block + 1) * BLOCK
        pieces = []
        turn = block_end - 1
        if b + beats > turn:
            # The chord stops a beat early; the dominant to the next key takes it.
            if turn > b:
                pieces.append((b, turn - b, [m + key for m in voicing], False))
            nxt = key_of(block + 1)
            m = melody_at(notes, turn)
            pieces.append((turn, 1.0, pivot(m + key if m is not None else None, nxt), True))
        else:
            pieces.append((b, beats, [m + key for m in voicing], False))
        for pb, pbeats, shifted, is_pivot in pieces:
            t = t0 + pb * spb
            if t >= t1:
                return key
            d = min(pbeats * spb, t1 - t)
            pad(L, R, t, d * 0.98, shifted, 0.05 if with_harp else 0.1)
            if with_harp:
                harp(L, R, t, d / spb, spb, shifted, 0.1, up=(int(pb) // 2) % 2 == 0)
            boom(L, R, t, pb, d / spb, spb, shifted, 0.26)
            chop(L, R, t, pb, d / spb, spb, shifted, 0.1)
    for b, beats, m, lead in notes:
        t = t0 + b * spb
        if t >= t1:
            break
        d = min(beats * spb, t1 - t)
        k = key_of(int(b // BLOCK))
        if lead == "strings":
            for cents in (-5.0, 5.0):
                voice(L, R, t, d * 0.98, m + k, STRINGS, 0.07, 0.6, 0.12, 0.3, 0.004, cents)
        else:
            grace = beats < 0.3
            voice(L, R, t, d * (0.98 if grace else 0.95), m + k, PICCOLO, 0.16, 0.58,
                  0.02 if grace else 0.06, 0.02 if grace else 0.2, 0.0 if grace else 0.006)
    return key

def reverb(ch, offset):
    combs = [(int(RATE * d) + offset * 29, fb) for d, fb in ((0.0371, .86), (0.0437, .85), (0.0503, .84), (0.0611, .82))]
    lines = [array("f", [0.0]) * size for size, _ in combs]
    pos = [0] * len(combs)
    n = len(ch)
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
        ch[i] = x + 0.11 * acc

def plan(body):
    """Which sections to play: whole passes of the form, then maybe part of
    one more (the piccolo and the pedal, the strings, or strings and
    piccolo). Of the plans that can fill the body within the tempo range,
    the one nearest the written tempo wins, the shorter on a tie. The keys
    come from the bar count (key_of)."""
    best = None
    for passes in range(0, 9):
        for tail in ([], ["B", "D"], ["A"], ["A", "B"]):
            o = FORM * passes + tail
            if not o:
                continue
            total = sum(beats_of(s) for s in o)
            spb = tempo(body, total)
            if total * spb < body - 1e-6:
                continue
            cost = abs(math.log(spb * BPM / 60))
            if best is None or cost < best[0] - 1e-9 or (abs(cost - best[0]) < 1e-9 and total < best[1]):
                best = (cost, total, o)
    return best[2]

def tempo(body, total):
    """Seconds a beat, so `total` beats fill `body` seconds, within -18 % and
    +20 % of the written tempo."""
    return min(60 / (BPM * 0.82), max(60 / (BPM * 1.2), body / total))

def render(seconds):
    n = int(seconds * RATE)
    L = array("f", [0.0]) * n
    R = array("f", [0.0]) * n

    # Title card: the opening chord swells under a few sparkles.
    pad(L, R, 0.2, INTRO - 0.4, SECTIONS["A"][1][0][1], 0.07, attack=1.3, release=0.9, strum=0.05)
    for k, m in enumerate((77, 80, 84, 87, 91)):
        bell(L, R, 0.35 + 0.22 * k, hz(m), 0.045, 0.55 + 0.05 * k, 2.2)

    body = max(0.0, seconds - INTRO - OUTRO)
    sections = plan(body)
    spb = tempo(body, sum(beats_of(s) for s in sections))
    end = INTRO + body
    key = play(L, R, INTRO, end, spb, sections) if body > 0 else 0

    # Rolled maj13#11 of the last key, three octaves wide, held to the end.
    hold = max(1.2, seconds - end - 0.8)
    spread = [m + key for m in (41, 48, 57, 64, 67, 71, 74)]
    voice(L, R, end + 0.05, hold, spread[0], PLUCK, 0.22, 0.5, 0.01, 0.6, 0.0, 0.0, 2.5)
    for i, m in enumerate(spread[1:]):
        for cents in (-5.0, 5.0):
            voice(L, R, end + 0.1 + i * 0.09, hold - i * 0.09, m, STRINGS, 0.045, pan_of(m), 0.08, 0.8, 0.004, cents)
    bell(L, R, end + 0.75, hz(spread[-1] + 12), 0.035, 0.6, 3.0)

    reverb(L, 0)
    reverb(R, 1)
    peak = max(max(abs(v) for v in L), max(abs(v) for v in R)) or 1
    fade = int(0.6 * RATE)
    for i in range(n):
        e = min(1.0, (n - i) / fade)
        L[i] = math.tanh(1.1 * L[i] / peak) * 0.7 * e
        R[i] = math.tanh(1.1 * R[i] / peak) * 0.7 * e
    return L, R

def write(path, seconds):
    L, R = render(seconds)
    frames = array("h", [0]) * (2 * len(L))
    frames[0::2] = array("h", (int(v * 32767) for v in L))
    frames[1::2] = array("h", (int(v * 32767) for v in R))
    if sys.byteorder == "big":
        frames.byteswap()
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(frames.tobytes())

if __name__ == "__main__":
    write(sys.argv[1], float(sys.argv[2]))

//! Local midnight in one time zone without a time-zone crate (#1038; the
//! binary stays small for the Pi). A zone is a POSIX TZ rule such as
//! `PST8PDT,M3.2.0,M11.1.0`: a standard offset and, optionally, a summer
//! offset with the days it starts and ends. A name such as
//! `America/Vancouver` is looked up in the system's zoneinfo files, whose
//! last line is that rule for the years after their last listed change.

/// The setting's default.
pub const DEFAULT: &str = "America/Vancouver";
/// [`DEFAULT`] when the server has no zoneinfo files: tzdata 2026 has
/// British Columbia on UTC-7 all year from 1 November 2026 (summer time
/// already is UTC-7).
pub const DEFAULT_RULE: &str = "MST7";
/// Where the zoneinfo files are.
const ZONEINFO: &str = "/usr/share/zoneinfo";
const DAY: i64 = 86_400;

/// A time zone, as a POSIX TZ rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    /// Seconds east of UTC outside summer time.
    std: i64,
    dst: Option<Dst>,
}

#[derive(Debug, Clone, PartialEq)]
struct Dst {
    /// Seconds east of UTC in summer time.
    offset: i64,
    start: Rule,
    end: Rule,
}

/// `Mm.w.d/time`: weekday `d` (0 Sunday) of week `w` (5: the last) of
/// month `m`, at `time` seconds after local midnight.
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    month: i64,
    week: i64,
    weekday: i64,
    time: i64,
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.i += 1;
        }
        hit
    }

    /// A zone abbreviation: three or more letters, or `<…>`.
    fn name(&mut self) -> Result<(), String> {
        let from = self.i;
        if self.eat(b'<') {
            while self.peek().is_some_and(|c| c != b'>') {
                self.i += 1;
            }
            return if self.eat(b'>') { Ok(()) } else { Err("an unclosed <".into()) };
        }
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i - from < 3 {
            return Err("a zone name of three letters or more".into());
        }
        Ok(())
    }

    fn number(&mut self) -> Result<i64, String> {
        let from = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.s.get(from..self.i).and_then(|d| std::str::from_utf8(d).ok()).and_then(|n| n.parse().ok()).ok_or_else(|| "a number".into())
    }

    /// `[+-]hh[:mm[:ss]]` in seconds.
    fn hms(&mut self) -> Result<i64, String> {
        let sign = if self.eat(b'-') {
            -1
        } else {
            self.eat(b'+');
            1
        };
        let mut secs = self.number()? * 3600;
        for unit in [60, 1] {
            if !self.eat(b':') {
                break;
            }
            secs += self.number()? * unit;
        }
        Ok(sign * secs)
    }

    fn rule(&mut self) -> Result<Rule, String> {
        if !self.eat(b'M') {
            return Err("only Mm.w.d change rules".into());
        }
        let month = self.number()?;
        let week = if self.eat(b'.') { self.number()? } else { 0 };
        let weekday = if self.eat(b'.') { self.number()? } else { -1 };
        if !(1..=12).contains(&month) || !(1..=5).contains(&week) || !(0..=6).contains(&weekday) {
            return Err("a change rule out of range".into());
        }
        let time = if self.eat(b'/') { self.hms()? } else { 2 * 3600 };
        Ok(Rule { month, week, weekday, time })
    }
}

/// Days since 1970-01-01 of a date (H. Hinnant's algorithm).
pub(super) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// The year of a day counted from 1970-01-01.
fn year_of(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(mp >= 10)
}

impl Rule {
    /// Its local time in `year`, in seconds from 1970 as if local were UTC.
    fn local(&self, year: i64) -> i64 {
        let first = days_from_civil(year, self.month, 1);
        let next = if self.month == 12 { days_from_civil(year + 1, 1, 1) } else { days_from_civil(year, self.month + 1, 1) };
        let weekday_first = (first + 4).rem_euclid(7); // 1970-01-01 was a Thursday
        let mut day = (self.weekday - weekday_first).rem_euclid(7) + (self.week - 1) * 7;
        while first + day >= next {
            day -= 7;
        }
        (first + day) * DAY + self.time
    }
}

impl Zone {
    pub const UTC: Zone = Zone { std: 0, dst: None };

    /// Read a POSIX TZ rule (`UTC0`, `MST7`, `PST8PDT,M3.2.0,M11.1.0`).
    pub fn parse(rule: &str) -> Result<Zone, String> {
        let mut p = Parser { s: rule.trim().as_bytes(), i: 0 };
        let bad = |e: String| format!("{rule:?} is not a time zone rule: {e}");
        p.name().map_err(bad)?;
        let std = -p.hms().map_err(bad)?;
        if p.peek().is_none() {
            return Ok(Zone { std, dst: None });
        }
        p.name().map_err(bad)?;
        let offset = if p.peek().is_some_and(|c| c != b',') { -p.hms().map_err(bad)? } else { std + 3600 };
        if !p.eat(b',') {
            return Err(bad("no change rules".into()));
        }
        let start = p.rule().map_err(bad)?;
        if !p.eat(b',') {
            return Err(bad("no end rule".into()));
        }
        let end = p.rule().map_err(bad)?;
        if p.peek().is_some() {
            return Err(bad("text after the rules".into()));
        }
        Ok(Zone { std, dst: Some(Dst { offset, start, end }) })
    }

    /// A zone by name (`America/Vancouver`, from the zoneinfo files) or by
    /// rule. [`DEFAULT`] works without the files.
    pub fn named(name: &str) -> Result<Zone, String> {
        let name = name.trim();
        if let Ok(z) = Zone::parse(name) {
            return Ok(z);
        }
        if name.is_empty() || name.contains("..") || !name.chars().all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c)) {
            return Err(format!("{name:?} is not a time zone name"));
        }
        match std::fs::read(format!("{ZONEINFO}/{name}")) {
            Ok(bytes) => Zone::parse(&footer(&bytes).ok_or_else(|| format!("{ZONEINFO}/{name} has no rule for the coming years"))?),
            Err(_) if name == DEFAULT => Zone::parse(DEFAULT_RULE),
            Err(e) => Err(format!("no time zone {name} in {ZONEINFO}: {e}")),
        }
    }

    /// Seconds east of UTC at `t`.
    pub fn offset(&self, t: i64) -> i64 {
        let Some(d) = &self.dst else { return self.std };
        let year = year_of((t + self.std).div_euclid(DAY));
        // The start is given in standard time, the end in summer time.
        let start = d.start.local(year) - self.std;
        let end = d.end.local(year) - d.offset;
        let summer = if start < end { start <= t && t < end } else { t >= start || t < end };
        if summer {
            d.offset
        } else {
            self.std
        }
    }

    /// The first local midnight after `now`.
    pub fn next_midnight(&self, now: i64) -> i64 {
        let off = self.offset(now);
        let midnight = ((now + off).div_euclid(DAY) + 1) * DAY;
        let mut t = midnight - off;
        // Settle the offset at midnight itself (a change between now and then).
        for _ in 0..2 {
            t = midnight - self.offset(t);
        }
        if t <= now {
            t += DAY;
        }
        t
    }
}

/// The rule on the last line of a TZif file (version 2 or later).
pub(super) fn footer(bytes: &[u8]) -> Option<String> {
    if !bytes.starts_with(b"TZif") || bytes.get(4).is_none_or(|v| *v == 0) {
        return None;
    }
    let body = bytes.strip_suffix(b"\n")?;
    let from = body.iter().rposition(|c| *c == b'\n')? + 1;
    let rule = std::str::from_utf8(body.get(from..)?).ok()?.trim().to_string();
    (!rule.is_empty()).then_some(rule)
}

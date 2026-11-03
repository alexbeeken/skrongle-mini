//! The price table behind the Generate dialog's cost estimate: what a
//! learner's own key will be billed for a job, from the provider's list
//! prices (dollars per million tokens, stored in micro-dollars).
//! `SKRONGLE_AI_PRICES` adds or overrides entries. Nothing here meters or
//! limits a call: every call is on the learner's own key and account.

use std::collections::HashMap;

/// One micro-dollar is a millionth of a dollar; every amount is stored so.
pub const MICROS_PER_DOLLAR: i64 = 1_000_000;

/// Prices in micro-dollars per million tokens (so `$2.00 / MTok` is 2_000_000).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub input: i64,
    pub output: i64,
    pub cached: i64,
}

/// Anthropic's list prices, checked 2026-10-19 (dollars per million tokens:
/// input, output, cache read). `*` prices any model not
/// listed at the dearest rate (Claude Fable 5.1), so an unknown model is
/// never cheap. The other providers' models (OpenAI's, Google's, and those
/// #24 added: DeepSeek, Mistral, xAI, OpenRouter, an Ollama server) are not
/// listed: their price is unknown, and the Generate dialog shows no cost
/// for them (`listed`). `SKRONGLE_AI_PRICES` adds or overrides entries.
pub const DEFAULT_PRICES: &str = "claude-haiku-5-5=0.10/0.50/0.01,claude-sonnet-5-5=2/10/0.10,claude-opus-5-5=4/20/0.20,*=10/50/1";

#[derive(Clone, Debug)]
pub struct PriceTable {
    prices: HashMap<String, Price>,
}

impl PriceTable {
    /// `model=in/out/cached,…` in dollars per million tokens; `cached`
    /// may be left out (then it is the input price).
    pub fn parse(spec: &str) -> Result<PriceTable, String> {
        let mut t = PriceTable { prices: HashMap::new() };
        t.merge(spec)?;
        Ok(t)
    }

    /// The built-in table, with `SKRONGLE_AI_PRICES` merged in when set.
    pub fn from_env() -> Result<PriceTable, String> {
        let mut t = PriceTable::parse(DEFAULT_PRICES).expect("default prices parse"); // panic-ok: the built-in defaults parse (tests check them)
        if let Some(p) = crate::env("AI_PRICES") {
            t.merge(&p).map_err(|e| format!("SKRONGLE_AI_PRICES: {e}"))?;
        }
        Ok(t)
    }

    pub fn merge(&mut self, spec: &str) -> Result<(), String> {
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (model, nums) = entry.split_once('=').ok_or_else(|| format!("price entry {entry:?} needs model=in/out[/cached]"))?; // panic-ok: a form field name or message, not indexing
            let n: Vec<i64> = nums.split('/').map(|v| dollars_to_micros(v.trim())).collect::<Result<_, _>>()?;
            let price = match n.as_slice() {
                [i, o] => Price { input: *i, output: *o, cached: *i },
                [i, o, c] => Price { input: *i, output: *o, cached: *c },
                _ => return Err(format!("price entry {entry:?} needs model=in/out[/cached]")), // panic-ok: a form field name or message, not indexing
            };
            self.prices.insert(model.trim().to_string(), price);
        }
        Ok(())
    }

    pub fn price(&self, model: &str) -> Price {
        self.prices.get(model).or_else(|| self.prices.get("*")).copied().unwrap_or(Price { input: 0, output: 0, cached: 0 })
    }

    /// `model` has a price of its own, not the `*` rate an unknown model
    /// gets (which is set high on purpose, so too high for an estimate).
    pub fn listed(&self, model: &str) -> bool {
        self.prices.contains_key(model)
    }

    /// The cost of `input` and `output` tokens at `p`, in micro-dollars,
    /// rounded up (an estimate's sum).
    pub fn cost_at(p: Price, input: u64, output: u64) -> i64 {
        let total = input as i128 * p.input as i128 + output as i128 * p.output as i128;
        ((total + 999_999) / 1_000_000) as i64
    }
}

/// "0.10" / "$3" -> micro-dollars.
pub fn dollars_to_micros(v: &str) -> Result<i64, String> {
    let v = v.trim().trim_start_matches('$');
    let d: f64 = v.parse().map_err(|_| format!("{v:?} is not an amount in dollars"))?;
    if !d.is_finite() || d < 0.0 {
        return Err(format!("{v:?} is not an amount in dollars"));
    }
    Ok((d * MICROS_PER_DOLLAR as f64).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_parse_and_cost() {
        let t = PriceTable::parse(DEFAULT_PRICES).unwrap();
        assert_eq!(t.price("claude-sonnet-5-5"), Price { input: 2_000_000, output: 10_000_000, cached: 100_000 });
        // Unknown models cost the dearest rate, never nothing.
        assert_eq!(t.price("something-new").input, 10_000_000);
        assert!(!t.listed("something-new") && t.listed("claude-haiku-5-5"));
        // 1000 in + 500 out on Sonnet: $0.002 + $0.005.
        assert_eq!(PriceTable::cost_at(t.price("claude-sonnet-5-5"), 1000, 500), 7000);
        assert_eq!(PriceTable::cost_at(t.price("claude-haiku-5-5"), 1, 0), 1, "rounded up");
        assert!(PriceTable::parse("bad").is_err() && PriceTable::parse("m=1").is_err());
        assert_eq!(dollars_to_micros("$0.10"), Ok(100_000));
        assert!(dollars_to_micros("-1").is_err());
    }
}

//! Which language a lesson's text reads like (#450, #1199): a cheap guess
//! from function words, no model and no table bigger than a few lines. It
//! tells the languages the app itself speaks apart (en, es, fr, de, pt, it
//! and Chinese by its script), nothing finer: a text in a language it
//! doesn't know gets no guess, and so no finding.

/// The languages the guess knows, as BCP 47 primary subtags.
pub const KNOWN: [&str; 7] = ["en", "es", "fr", "de", "pt", "it", "zh"];

/// Function words that mark each language. Words two of them share ("de",
/// "que") count for both, so they only dilute; the distinctive ones decide.
const WORDS: [(&str, &str); 6] = [
    ("en", "the and is are you this that with for of it in to not your have has they we when what which there here from but can will be on as"),
    ("es", "el la los las un una es son está están y de del que con por para pero como también muy no se al lo mi tu su usted yo hay este esta"),
    ("fr", "le la les un une des du est sont et de que qui avec pour dans pas ne vous nous je tu il elle ils elles ce cette ces mais ou où sur au aux très aussi"),
    ("de", "der die das und ist sind ein eine einen nicht mit für auf von zu den dem des ich du er sie wir ihr auch aber oder wie was wenn dass noch sehr kann"),
    ("pt", "o a os as um uma é são está estão e de do da dos das que com por para mas como também muito não se ao no na nos nas você eu ele ela isso este esta"),
    ("it", "il lo la i gli le un una uno è sono e di del della che con per non ma come anche molto si al alla io lui lei noi voi questo questa"),
];

/// The text needs this many marker words before a guess is made …
const MIN_HITS: usize = 8;
/// … and the winning language this many times the hits of `expected`.
const MARGIN: usize = 3;

/// The primary subtag of a language tag (`en-US` → `en`), lowercase.
pub fn primary(tag: &str) -> String {
    tag.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase()
}

/// The known language `text` reads like, when it is not `expected` by a
/// clear margin. `None` when it reads like `expected`, like nothing the
/// guess knows, or when `expected` is not a known language either.
pub fn reads_like(text: &str, expected: &str) -> Option<&'static str> {
    if !KNOWN.contains(&expected) {
        return None;
    }
    let (cjk, latin) = text.chars().filter(|c| c.is_alphabetic()).fold((0usize, 0usize), |(c, l), ch| if is_cjk(ch) { (c + 1, l) } else { (c, l + 1) });
    // Chinese is told by its script: most letters CJK, or hardly any.
    if cjk >= MIN_HITS && cjk * 2 > latin {
        return (expected != "zh").then_some("zh");
    }
    if expected == "zh" {
        return (latin >= MIN_HITS * 4).then(|| best_latin(text)).flatten();
    }
    let hits = hits(text);
    let expected_hits = hits.iter().find(|(l, _)| *l == expected).map_or(0, |(_, n)| *n);
    let (best, n) = hits.iter().copied().max_by_key(|(_, n)| *n)?;
    (best != expected && n >= MIN_HITS && n >= expected_hits * MARGIN).then_some(best)
}

/// The Latin-script language with the most marker words, when it has enough.
fn best_latin(text: &str) -> Option<&'static str> {
    let (best, n) = hits(text).into_iter().max_by_key(|(_, n)| *n)?;
    (n >= MIN_HITS).then_some(best)
}

/// Marker-word hits per Latin-script language, in `WORDS` order.
fn hits(text: &str) -> Vec<(&'static str, usize)> {
    let lower = text.to_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    WORDS.iter().map(|(lang, words)| (*lang, tokens.iter().filter(|t| words.split(' ').any(|w| w == **t)).count())).collect()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGLISH: &str = "In this lesson you will learn how to say hello. The word is used when you meet someone, and it is the first thing they hear from you. Practise it with a friend.";
    const SPANISH: &str = "En esta lección vas a aprender a saludar. La palabra se usa cuando conoces a alguien, y es lo primero que escucha de ti. Practica con un amigo o una amiga.";

    #[test]
    fn english_explained_in_english_is_fine() {
        assert_eq!(reads_like(ENGLISH, "en"), None);
    }

    #[test]
    fn spanish_text_where_english_was_expected_is_spanish() {
        assert_eq!(reads_like(SPANISH, "en"), Some("es"));
        assert_eq!(reads_like(ENGLISH, "es"), Some("en"));
    }

    #[test]
    fn a_short_text_or_an_unknown_language_gets_no_guess() {
        assert_eq!(reads_like("Hola. ¿Qué tal?", "en"), None);
        assert_eq!(reads_like(SPANISH, "sv"), None);
        assert_eq!(reads_like("Hej och välkommen till den här lektionen om att hälsa på någon du inte känner.", "en"), None);
    }

    #[test]
    fn english_prose_with_spanish_examples_still_reads_like_english() {
        let mixed = format!("{ENGLISH} For example: «El gato está en la casa» and «Los niños son muy altos». {ENGLISH}");
        assert_eq!(reads_like(&mixed, "en"), None);
    }

    #[test]
    fn chinese_is_told_by_its_script() {
        let zh = "这一课教你怎么打招呼。见到别人的时候，先说你好，然后问对方好不好。";
        assert_eq!(reads_like(zh, "en"), Some("zh"));
        assert_eq!(reads_like(zh, "zh"), None);
        assert_eq!(reads_like(ENGLISH, "zh"), Some("en"));
    }

    #[test]
    fn primary_subtag() {
        assert_eq!(primary("en-US"), "en");
        assert_eq!(primary(" PT_br "), "pt");
        assert_eq!(primary(""), "");
    }
}

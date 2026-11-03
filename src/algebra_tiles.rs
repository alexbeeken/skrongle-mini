//! The `algebra-tiles` question type (#1355): a pile of tiles for a
//! quadratic (big squares for x², strips for x, small squares for 1) that the
//! learner lays out as a rectangle, then types its two sides.
//!
//! The tiles are a scratch area: the page never says whether the layout is
//! right. The typed answer is graded by the server. It is right when it is a
//! product of two sides, each with the variable in it (the form check
//! `expression` leaves out on purpose), and the product is equivalent to the
//! tiles. Only positive tiles for now; negative tiles are a later step.

use crate::content::{Lesson, QuizItem, QuizType};
use crate::mathcheck::{self, Expr};
use crate::quiz::Verdict;

/// Most tiles of each kind, so a rectangle still fits on a phone.
pub const MAX_SQUARES: u32 = 3;
pub const MAX_STRIPS: u32 = 12;
pub const MAX_UNITS: u32 = 20;

/// The pile, as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tiles {
    /// The variable's letter, as the tiles are labelled (`x`).
    pub var: char,
    /// x² tiles.
    pub squares: u32,
    /// x tiles.
    pub strips: u32,
    /// 1 tiles.
    pub units: u32,
}

/// Read `tiles` (`x^2 + 5x + 6`) as a pile: a quadratic in one variable with
/// whole, non-negative coefficients and at least one x² tile.
pub fn read(spec: &str) -> Result<Tiles, String> {
    let e = mathcheck::parse(spec.trim()).map_err(|e| format!("tiles '{spec}' cannot be read ({e})"))?;
    let mut vars = Vec::new();
    e.vars(&mut vars);
    let [var] = vars.as_slice() else {
        return Err(format!("tiles '{spec}' must use exactly one variable"));
    };
    let at = |v: f64| e.eval(&|_| v);
    let (c, plus, minus) = (at(0.0), at(1.0), at(-1.0));
    let (a, b) = ((plus + minus) / 2.0 - c, (plus - minus) / 2.0);
    let whole = |n: f64, max: u32| (n.is_finite() && n >= -1e-9 && (n - n.round()).abs() < 1e-9 && n.round() <= max as f64).then(|| n.round() as u32);
    let (Some(squares), Some(strips), Some(units)) = (whole(a, MAX_SQUARES), whole(b, MAX_STRIPS), whole(c, MAX_UNITS)) else {
        return Err(format!(
            "tiles '{spec}' must be a x^2 + b x + c with whole numbers a from 1 to {MAX_SQUARES}, b from 0 to {MAX_STRIPS} and c from 0 to {MAX_UNITS}"
        ));
    };
    let t = Tiles { var: *var, squares, strips, units };
    if squares == 0 || !mathcheck::check_expression(&t.expression(), spec).correct {
        return Err(format!("tiles '{spec}' must be a x^2 + b x + c with at least one {var}^2 tile"));
    }
    Ok(t)
}

impl Tiles {
    /// The pile written out: `x^2 + 5x + 6`.
    pub fn expression(&self) -> String {
        format!("{}{}^2 + {}{} + {}", self.squares, self.var, self.strips, self.var, self.units)
    }
}

/// The tiles of a question to draw, when it is an `algebra-tiles` one.
pub fn of(item: &QuizItem) -> Option<Tiles> {
    (item.kind == QuizType::AlgebraTiles).then(|| read(&item.tiles.join(" ")).ok()).flatten()
}

/// Does the lesson ask an `algebra-tiles` question (so the page loads the board)?
pub fn in_lesson(lesson: &Lesson) -> bool {
    lesson.quiz.iter().any(|q| q.kind == QuizType::AlgebraTiles)
}

/// The factors of a product, flattened: `(x+2)(x+3)` is two, `2(x+1)(x+3)`
/// three, `(x+2)^2` two equal ones.
fn factors<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::Bin('*', a, b) => {
            factors(a, out);
            factors(b, out);
        }
        Expr::Bin('^', a, b) if matches!(**b, Expr::Num(n) if n == 2.0) => {
            out.push(a);
            out.push(a);
        }
        _ => out.push(e),
    }
}

/// Is the answer written as two sides multiplied, each with the variable in it?
pub fn is_two_sides(answer: &str) -> bool {
    let Ok(e) = mathcheck::parse(answer.trim()) else { return false };
    let mut fs = Vec::new();
    factors(&e, &mut fs);
    // A number next to a lone variable is part of that side: 2x(x + 1) is
    // the sides 2x and x + 1. With no lone variable it stays a third factor,
    // so 2(x + 1)(x + 3) is still not two sides.
    if fs.iter().any(|f| matches!(f, Expr::Var(_))) {
        fs.retain(|f| !matches!(f, Expr::Num(_)));
    }
    fs.len() == 2
        && fs.iter().all(|f| {
            let mut v = Vec::new();
            f.vars(&mut v);
            !v.is_empty()
        })
}

/// Course-file check: the tiles can be read, `form` is `product`, and every
/// accepted answer is two sides whose product is the tiles.
pub fn validate(item: &QuizItem) -> Result<(), String> {
    let t = read(&item.tiles.join(" "))?;
    if !item.form.is_empty() && item.form != "product" {
        return Err(format!("form '{}' is not known (the one form is \"product\")", item.form));
    }
    for a in std::iter::once(&item.correct_answer).chain(&item.accepted_answers) {
        mathcheck::validate_expression(a)?;
        if !is_two_sides(a) {
            return Err(format!("'{a}' must be the two sides multiplied, such as (x + 2)(x + 3)"));
        }
        if !mathcheck::check_expression(&t.expression(), a).correct {
            return Err(format!("'{a}' does not multiply out to the tiles '{}'", item.tiles.join(" ")));
        }
    }
    Ok(())
}

/// Grade a typed answer: two sides, whose product is the tiles.
pub fn check(item: &QuizItem, given: &str) -> Verdict {
    let Ok(t) = read(&item.tiles.join(" ")) else { return Verdict { correct: false, note: None } };
    let product = mathcheck::check_expression(&t.expression(), given);
    if product.note.is_some() && !product.correct {
        return Verdict { correct: false, note: product.note };
    }
    if !is_two_sides(given) {
        return Verdict { correct: false, note: Some(crate::t("quiz.check.two_sides")) };
    }
    Verdict { correct: product.correct, note: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(tiles: &str, answer: &str) -> QuizItem {
        serde_json::from_value(serde_json::json!({
            "question_id": "q", "prompt": "p", "type": "algebra-tiles", "tiles": tiles, "form": "product", "correct_answer": answer
        }))
        .unwrap()
    }

    #[test]
    fn reads_the_pile() {
        assert_eq!(read("x^2 + 5x + 6").unwrap(), Tiles { var: 'x', squares: 1, strips: 5, units: 6 });
        assert_eq!(read("2a² + 3a").unwrap(), Tiles { var: 'a', squares: 2, strips: 3, units: 0 });
        assert_eq!(read("(x+1)(x+4)").unwrap(), Tiles { var: 'x', squares: 1, strips: 5, units: 4 });
        for bad in ["x^2 - 5x + 6", "x + 1", "x^3 + 1", "x^2 + y", "4x^2 + 1", "x^2 + 0.5x", "x^2 + 30"] {
            assert!(read(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn the_answer_must_be_two_sides() {
        assert!(is_two_sides("(x + 2)(x + 3)"));
        assert!(is_two_sides("(x+3)*(x+2)"));
        assert!(is_two_sides("x(x + 3)"));
        assert!(is_two_sides("(x + 2)^2"));
        assert!(is_two_sides("2x(x + 1)"));
        assert!(is_two_sides("(2x)(x + 1)"));
        assert!(is_two_sides("(x + 1)(2x)"));
        assert!(!is_two_sides("2x(x + 1)(x + 2)"));
        assert!(!is_two_sides("x^2 + 5x + 6"));
        assert!(!is_two_sides("2(x + 1)(x + 3)"));
        assert!(!is_two_sides("1(x^2 + 5x + 6)"));
    }

    #[test]
    fn grades_the_product_and_the_form() {
        let q = item("x^2 + 5x + 6", "(x + 2)(x + 3)");
        assert!(check(&q, "(x+3)(x+2)").correct);
        assert!(check(&q, "(2 + x)(3 + x)").correct);
        let v = check(&q, "x^2 + 5x + 6");
        assert!(!v.correct && v.note.is_some(), "the expanded form is equal but not two sides");
        let v = check(&q, "(x + 1)(x + 6)");
        assert!(!v.correct && v.note.is_none());
        assert!(!check(&q, "(x + 2").correct);
    }

    #[test]
    fn validates_the_course_file() {
        assert!(validate(&item("x^2 + 5x + 6", "(x + 2)(x + 3)")).is_ok());
        assert!(validate(&item("x^2 + 5x + 6", "(x + 1)(x + 6)")).is_err());
        assert!(validate(&item("x^2 + 5x + 6", "x^2 + 5x + 6")).is_err());
        assert!(validate(&item("2x^2 + 2x", "2x(x + 1)")).is_ok(), "a number on the lone-x side is still two sides");
        assert!(check(&item("2x^2 + 2x", "2x(x + 1)"), "(x + 1)(2x)").correct);
        let mut q = item("x^2 + 5x + 6", "(x + 2)(x + 3)");
        q.form = "sum".into();
        assert!(validate(&q).is_err());
    }
}

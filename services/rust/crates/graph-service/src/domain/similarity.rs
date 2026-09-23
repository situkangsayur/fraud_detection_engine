//! Similarity rules for "mirip" identifiers (rule-dsl §6.5).
//!
//! Fraud rings vary identifiers on purpose: `+6281234567890` / `+6281234567891`, `budi@gmail.com` /
//! `budi@yahoo.com`, `jalan melati 5 rt 1 rw 2` / `jalan melati no 5 rt 01 rw 02`. Similar
//! entities are connected by a scored similarity edge. Traversals follow it only when the
//! caller asks for `include_similar`, and only if the score clears the project's threshold.
//!
//! Phone and email rules are decided here (pure functions). Address similarity uses Postgres
//! `pg_trgm` in the adapter, because it needs the trigram index to find candidates.

/// Similarity verdict: `(score, method)`.
pub type Similarity = (f32, &'static str);

/// Phones are similar when they differ in exactly one digit (same length), or when they share the
/// last 9 digits (same subscriber number written with a different prefix that normalisation could
/// not unify).
pub fn phone_similarity(a: &str, b: &str) -> Option<Similarity> {
    if a == b {
        return None;
    }
    let da: Vec<char> = a.chars().filter(char::is_ascii_digit).collect();
    let db: Vec<char> = b.chars().filter(char::is_ascii_digit).collect();
    if da.len() >= 9 && db.len() >= 9 && da[da.len() - 9..] == db[db.len() - 9..] {
        return Some((0.95, "phone_suffix"));
    }
    if da.len() == db.len() && da.len() >= 8 {
        let diff = da.iter().zip(&db).filter(|(x, y)| x != y).count();
        if diff == 1 {
            return Some((0.9, "phone_edit1"));
        }
    }
    None
}

/// Emails are similar when the normalised local part is identical on different domains.
pub fn email_similarity(a: &str, b: &str) -> Option<Similarity> {
    let (la, da) = a.split_once('@')?;
    let (lb, db) = b.split_once('@')?;
    (la == lb && da != db && la.chars().count() >= 3).then_some((0.9, "email_local"))
}

/// Local part of a normalised email.
pub fn email_local(email: &str) -> Option<&str> {
    email
        .split_once('@')
        .map(|(l, _)| l)
        .filter(|l| l.chars().count() >= 3)
}

/// The last `n` digits of a phone, used as the adapter's candidate prefilter.
pub fn phone_suffix(phone: &str, n: usize) -> Option<String> {
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    (digits.len() >= n).then(|| digits[digits.len() - n..].to_string())
}

/// Escapes `%`, `_` and `\` for use inside a SQL `LIKE` pattern.
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Stores similarity pairs with `entity_a < entity_b` (table CHECK constraint).
pub fn ordered_pair(a: i64, b: i64) -> (i64, i64) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_rules() {
        assert_eq!(phone_similarity("+6281234567890", "+6281234567890"), None);
        assert_eq!(
            phone_similarity("+6281234567890", "+6281234567891"),
            Some((0.9, "phone_edit1"))
        );
        assert_eq!(
            phone_similarity("+6281234567890", "+6591234567890"),
            Some((0.95, "phone_suffix"))
        );
        assert_eq!(phone_similarity("+6281234567890", "+6281299999999"), None);
    }

    #[test]
    fn email_rules() {
        assert_eq!(
            email_similarity("budi@gmail.com", "budi@yahoo.com"),
            Some((0.9, "email_local"))
        );
        assert_eq!(email_similarity("budi@gmail.com", "budi@gmail.com"), None);
        assert_eq!(email_similarity("ab@gmail.com", "ab@yahoo.com"), None);
    }

    #[test]
    fn helpers() {
        assert_eq!(escape_like("a_b%c\\"), "a\\_b\\%c\\\\");
        assert_eq!(ordered_pair(5, 2), (2, 5));
        assert_eq!(phone_suffix("+6281234567890", 9).as_deref(), Some("234567890"));
        assert_eq!(email_local("ab@x.com"), None);
    }
}

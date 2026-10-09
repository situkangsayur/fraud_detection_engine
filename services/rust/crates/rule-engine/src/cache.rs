//! Process-wide compile caches for formulas and regexes.
//!
//! Rules are evaluated thousands of times per second but change rarely, so parsed formulas and compiled regexes
//! are cached by their source text. The caches are bounded: when full they are cleared (simple and safe; the
//! working set of a deployment is small and refills immediately).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;

use crate::formula::{Formula, ParseError};

const MAX_ENTRIES: usize = 4_096;

static FORMULAS: LazyLock<Mutex<HashMap<String, Arc<Formula>>>> = LazyLock::new(Default::default);
static REGEXES: LazyLock<Mutex<HashMap<String, Arc<Regex>>>> = LazyLock::new(Default::default);

fn get_or_insert<T, E>(
    cache: &Mutex<HashMap<String, Arc<T>>>,
    key: &str,
    build: impl FnOnce() -> Result<T, E>,
) -> Result<Arc<T>, E> {
    if let Ok(map) = cache.lock() {
        if let Some(hit) = map.get(key) {
            return Ok(Arc::clone(hit));
        }
    }
    let built = Arc::new(build()?);
    // A poisoned lock only means another thread panicked mid-insert; the cache is an optimisation, so skip it.
    if let Ok(mut map) = cache.lock() {
        if map.len() >= MAX_ENTRIES {
            map.clear();
        }
        map.insert(key.to_string(), Arc::clone(&built));
    }
    Ok(built)
}

/// Parsed formula for `source`, cached.
pub fn formula(source: &str) -> Result<Arc<Formula>, ParseError> {
    get_or_insert(&FORMULAS, source, || Formula::parse(source))
}

/// Compiled regex for `pattern`, cached.
pub fn regex(pattern: &str) -> Result<Arc<Regex>, regex::Error> {
    get_or_insert(&REGEXES, pattern, || Regex::new(pattern))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn caches_return_same_instance() {
        let a = formula("x + 1").unwrap();
        let b = formula("x + 1").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(formula("x +").is_err());
        let r1 = regex("^a+$").unwrap();
        let r2 = regex("^a+$").unwrap();
        assert!(Arc::ptr_eq(&r1, &r2));
        assert!(regex("(").is_err());
    }
}

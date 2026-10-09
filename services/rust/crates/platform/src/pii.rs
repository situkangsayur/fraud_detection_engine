//! PII handling: tenant-scoped hashing, normalisation for graph linking, masking for display.
//!
//! * Card / bank account numbers are **never stored raw**. They become
//!   `HMAC-SHA256(tenant_pepper, digits)` where `tenant_pepper = HMAC-SHA256(PII_PEPPER, tenant_id)`.
//!   The same card links customers within a tenant, but the fingerprint is useless in another
//!   tenant and cannot be reversed without the pepper (architecture.md §5).
//! * Emails, phones and addresses are normalised so that trivially different spellings of the same
//!   value become the same graph entity ("0812-3456-789" and "+62 812 3456 789").

use hmac::{Hmac, Mac};
use once_cell::sync::Lazy;
use regex::Regex;
use sha2::Sha256;

use crate::config::Secret;
use crate::ids::TenantId;

type HmacSha256 = Hmac<Sha256>;

fn hmac_hex(key: &[u8], data: &[u8]) -> String {
    // HMAC accepts keys of any length, so `new_from_slice` cannot fail for Sha256.
    let mut mac = match HmacSha256::new_from_slice(key) {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

/// Per-tenant pepper derived from the platform pepper.
pub fn tenant_pepper(pii_pepper: &Secret, tenant: TenantId) -> Secret {
    Secret::new(hmac_hex(
        pii_pepper.expose().as_bytes(),
        tenant.to_string().as_bytes(),
    ))
}

/// Keeps only ASCII digits.
pub fn digits_only(raw: &str) -> String {
    raw.chars().filter(char::is_ascii_digit).collect()
}

/// Fingerprint of a card/account number (digits only). Returns `None` for inputs without digits.
pub fn hash_digits(pepper: &Secret, raw: &str) -> Option<String> {
    let d = digits_only(raw);
    if d.is_empty() {
        return None;
    }
    Some(hmac_hex(pepper.expose().as_bytes(), d.as_bytes()))
}

/// First `len` digits (BIN/IIN, 6 or 8).
pub fn pan_bin(raw: &str, len: usize) -> Option<String> {
    let d = digits_only(raw);
    (d.len() >= len + 4).then(|| d[..len].to_string())
}

pub fn pan_last4(raw: &str) -> Option<String> {
    let d = digits_only(raw);
    (d.len() >= 4).then(|| d[d.len() - 4..].to_string())
}

/// Luhn checksum validity (used to detect PANs).
pub fn luhn_valid(raw: &str) -> bool {
    let d = digits_only(raw);
    if !(12..=19).contains(&d.len()) {
        return false;
    }
    let sum: u32 = d
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let n = u32::from(b - b'0');
            if i % 2 == 1 {
                let x = n * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                n
            }
        })
        .sum();
    sum % 10 == 0
}

/// Normalises a phone number to E.164. `default_cc` is the calling code without `+` (Indonesia: `62`).
///
/// Handles `08123…`, `8123…`, `628123…`, `+628123…`, `0062…` and separators. Returns `None` when
/// fewer than 7 digits remain.
pub fn normalize_phone(raw: &str, default_cc: &str) -> Option<String> {
    let trimmed = raw.trim();
    let had_plus = trimmed.starts_with('+');
    let mut d = digits_only(trimmed);
    if d.len() < 7 {
        return None;
    }
    if had_plus {
        return Some(format!("+{d}"));
    }
    if let Some(rest) = d.strip_prefix("00") {
        return Some(format!("+{rest}"));
    }
    if d.starts_with(default_cc) && d.len() >= default_cc.len() + 8 {
        return Some(format!("+{d}"));
    }
    if let Some(rest) = d.strip_prefix('0') {
        d = rest.to_string();
    }
    Some(format!("+{default_cc}{d}"))
}

/// Lower-cases and strips Gmail dots / plus-tags (`J.Doe+promo@GMail.com` → `jdoe@gmail.com`).
pub fn normalize_email(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase();
    let (local, domain) = lower.rsplit_once('@')?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return None;
    }
    let domain = if domain == "googlemail.com" {
        "gmail.com"
    } else {
        domain
    };
    let local = local.split('+').next().unwrap_or(local);
    let local = if domain == "gmail.com" {
        local.replace('.', "")
    } else {
        local.to_string()
    };
    if local.is_empty() {
        return None;
    }
    Some(format!("{local}@{domain}"))
}

static ADDR_PUNCT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[.,;:#/\\()\-]+").unwrap_or_else(|_| unreachable_regex()));
static ADDR_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap_or_else(|_| unreachable_regex()));
static ADDR_RTRW: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\brt\s*0*(\d+)\s*rw\s*0*(\d+)\b").unwrap_or_else(|_| unreachable_regex()));

/// Fallback for statically-known regexes (never used in practice; avoids `unwrap` in library code).
fn unreachable_regex() -> Regex {
    #[allow(clippy::unwrap_used)]
    Regex::new("$^").unwrap()
}

/// Normalises an Indonesian address for similarity/linking:
/// lower-case, punctuation → space, common abbreviations expanded, RT/RW unified, spaces collapsed.
pub fn normalize_address(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    // Expand abbreviations that are written with a dot before punctuation is stripped.
    let mut s = format!(" {lower} ");
    for (from, to) in [
        (" jl. ", " jalan "),
        (" jl ", " jalan "),
        (" jln. ", " jalan "),
        (" jln ", " jalan "),
        (" no. ", " nomor "),
        (" no ", " nomor "),
        (" kel. ", " kelurahan "),
        (" kec. ", " kecamatan "),
        (" kab. ", " kabupaten "),
        (" gg. ", " gang "),
        (" gg ", " gang "),
        (" blk. ", " blok "),
        (" perum. ", " perumahan "),
    ] {
        s = s.replace(from, to);
    }
    let s = ADDR_PUNCT.replace_all(&s, " ");
    let s = ADDR_SPACES.replace_all(&s, " ");
    let s = format!(" {} ", s.trim());
    // Abbreviations again, now that punctuation is gone ("jl.sudirman" → "jl sudirman").
    let mut s = s;
    for (from, to) in [
        (" jl ", " jalan "),
        (" jln ", " jalan "),
        (" no ", " nomor "),
        (" gg ", " gang "),
    ] {
        s = s.replace(from, to);
    }
    let s = ADDR_RTRW.replace_all(s.trim(), "rt $1 rw $2");
    let out = ADDR_SPACES.replace_all(&s, " ").trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Canonical textual IP (`::ffff:1.2.3.4` → `1.2.3.4`); invalid input is ignored.
pub fn normalize_ip(raw: &str) -> Option<String> {
    let ip: std::net::IpAddr = raw.trim().parse().ok()?;
    let ip = match ip {
        std::net::IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(std::net::IpAddr::V4)
            .unwrap_or(std::net::IpAddr::V6(v6)),
        v4 => v4,
    };
    Some(ip.to_string())
}

/// `10.1.2.3` → `10.1.2.x`; IPv6 keeps the first 4 groups.
pub fn mask_ip(ip: &str) -> String {
    if ip.contains(':') {
        let head: Vec<&str> = ip.split(':').take(4).collect();
        format!("{}:…", head.join(":"))
    } else {
        match ip.rsplit_once('.') {
            Some((head, _)) => format!("{head}.x"),
            None => "x".into(),
        }
    }
}

/// `4111********1111`
pub fn mask_pan(bin: Option<&str>, last4: Option<&str>) -> String {
    format!("{}********{}", bin.unwrap_or("****"), last4.unwrap_or("****"))
}

/// `j***@gmail.com`
pub fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => {
            let first: String = local.chars().take(1).collect();
            format!("{first}***@{domain}")
        }
        None => "***".into(),
    }
}

/// `+62812****789`
pub fn mask_phone(phone: &str) -> String {
    let chars: Vec<char> = phone.chars().collect();
    if chars.len() <= 7 {
        return "****".into();
    }
    let head: String = chars[..chars.len().min(6)].iter().collect();
    let tail: String = chars[chars.len() - 3..].iter().collect();
    format!("{head}****{tail}")
}

/// Keeps the first `keep` characters, masks the rest (addresses, device ids).
pub fn mask_text(s: &str, keep: usize) -> String {
    let head: String = s.chars().take(keep).collect();
    if s.chars().count() <= keep {
        head
    } else {
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    #[test]
    fn phone_normalisation_variants_collapse() {
        let expected = Some("+628123456789".to_string());
        for raw in [
            "08123456789",
            "0812-3456-789",
            "+62 812 3456 789",
            "628123456789",
            "8123456789",
            "0062 812 3456789",
        ] {
            assert_eq!(normalize_phone(raw, "62"), expected, "{raw}");
        }
        assert_eq!(
            normalize_phone("+1 415 555 0100", "62"),
            Some("+14155550100".into())
        );
        assert_eq!(normalize_phone("123", "62"), None);
    }

    #[test]
    fn email_normalisation() {
        assert_eq!(
            normalize_email(" J.Doe+promo@GMail.com "),
            Some("jdoe@gmail.com".into())
        );
        assert_eq!(
            normalize_email("j.doe+x@googlemail.com"),
            Some("jdoe@gmail.com".into())
        );
        assert_eq!(
            normalize_email("a.b+c@yahoo.co.id"),
            Some("a.b@yahoo.co.id".into())
        );
        assert_eq!(normalize_email("not-an-email"), None);
    }

    #[test]
    fn address_normalisation() {
        assert_eq!(
            normalize_address("Jl. Sudirman No. 5, RT 01/RW 002, Kec. Menteng").as_deref(),
            Some("jalan sudirman nomor 5 rt 1 rw 2 kecamatan menteng")
        );
        assert_eq!(
            normalize_address("JLN SUDIRMAN NO 5 rt1 rw2 kecamatan  menteng").as_deref(),
            Some("jalan sudirman nomor 5 rt 1 rw 2 kecamatan menteng")
        );
        assert_eq!(normalize_address("   "), None);
    }

    #[test]
    fn hashing_is_tenant_scoped_and_digit_based() {
        let pepper = Secret::new("platform-pepper");
        let t1 = tenant_pepper(&pepper, TenantId(Uuid::from_u128(1)));
        let t2 = tenant_pepper(&pepper, TenantId(Uuid::from_u128(2)));
        let a = hash_digits(&t1, "4111 1111 1111 1111");
        assert_eq!(a, hash_digits(&t1, "4111-1111-1111-1111"));
        assert_ne!(a, hash_digits(&t2, "4111111111111111"));
        assert_eq!(a.as_ref().map(String::len), Some(64));
        assert_eq!(hash_digits(&t1, "n/a"), None);
    }

    #[test]
    fn pan_helpers() {
        assert!(luhn_valid("4111 1111 1111 1111"));
        assert!(!luhn_valid("4111 1111 1111 1112"));
        assert_eq!(pan_bin("4111111111111111", 6).as_deref(), Some("411111"));
        assert_eq!(pan_last4("4111111111111111").as_deref(), Some("1111"));
        assert_eq!(mask_pan(Some("411111"), Some("1111")), "411111********1111");
    }

    #[test]
    fn ip_helpers() {
        assert_eq!(normalize_ip(" ::ffff:10.1.2.3 ").as_deref(), Some("10.1.2.3"));
        assert_eq!(normalize_ip("not-an-ip"), None);
        assert_eq!(mask_ip("10.1.2.3"), "10.1.2.x");
    }

    #[test]
    fn masking() {
        assert_eq!(mask_email("john@gmail.com"), "j***@gmail.com");
        assert_eq!(mask_phone("+628123456789"), "+62812****789");
        assert_eq!(mask_text("abcdef", 3), "abc…");
    }
}

//! Adressen, wie Nutzer sie eintippen: `echo://host:port`, `host:port`,
//! `host`, `[::1]:9988` oder eine nackte IPv6-Adresse.

use echo_protocol::Wire;

/// Host und Port aus einer Adresse; ohne Port der Standardport.
#[must_use]
pub fn parse_address(address: &str) -> (String, u16) {
    let rest = strip_scheme(address).trim().trim_end_matches('/');
    if let Some(inner) = rest.strip_prefix('[')
        && let Some((host, tail)) = inner.split_once(']')
    {
        return (host.to_owned(), port_after(tail));
    }

    // Mehr als ein Doppelpunkt ohne Klammern ist eine nackte IPv6-Adresse ohne Port.
    match rest.split_once(':') {
        Some((host, port)) if !port.contains(':') && !host.is_empty() => {
            (host.to_owned(), port_after(&format!(":{port}")))
        }
        _ => (rest.to_owned(), Wire::DEFAULT_PORT),
    }
}

/// Ob die Adresse ausdruecklich den eigenen Server meint.
#[must_use]
pub fn is_echo_address(address: &str) -> bool {
    address
        .get(..Wire::SCHEME.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(Wire::SCHEME))
}

fn strip_scheme(address: &str) -> &str {
    if is_echo_address(address) {
        &address[Wire::SCHEME.len()..]
    } else {
        address
    }
}

fn port_after(tail: &str) -> u16 {
    tail.strip_prefix(':')
        .and_then(|port| port.parse().ok())
        .unwrap_or(Wire::DEFAULT_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_forms_users_type() {
        assert_eq!(
            parse_address("echo://ts.example:1234/"),
            ("ts.example".into(), 1234)
        );
        assert_eq!(
            parse_address("ECHO://ts.example"),
            ("ts.example".into(), 9988)
        );
        assert_eq!(parse_address(" ts.example:77 "), ("ts.example".into(), 77));
        assert_eq!(parse_address("[::1]:5000"), ("::1".into(), 5000));
        assert_eq!(parse_address("[::1]"), ("::1".into(), 9988));
        assert_eq!(parse_address("fe80::1"), ("fe80::1".into(), 9988));
        assert_eq!(parse_address("host:kein"), ("host".into(), 9988));
        assert!(is_echo_address("echo://x"));
        assert!(!is_echo_address("ts3server://x"));
    }
}

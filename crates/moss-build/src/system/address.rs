//! Which kind of network an IP address belongs to, for the fetches that must
//! not reach this machine or its local network on a page's say-so.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The class of an address. Callers refuse the classes they care about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AddressClass {
    /// 127.0.0.0/8, `::1`.
    Loopback,
    /// 169.254.0.0/16, fe80::/10.
    LinkLocal,
    /// 0.0.0.0, `::`.
    Unspecified,
    /// 224.0.0.0/4, ff00::/8.
    Multicast,
    /// 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, fc00::/7.
    Private,
    Public,
}

/// The class of `ip`, after an IPv6 address that carries an IPv4 address is
/// read as that IPv4 address: a socket connects to `::ffff:127.0.0.1` over
/// IPv4, so it is loopback, not a public IPv6 address.
pub(crate) fn classify(ip: IpAddr) -> AddressClass {
    match carried_ipv4(ip) {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

/// The IPv4 address inside an IPv4-mapped (`::ffff:a.b.c.d`), IPv4-compatible
/// (`::a.b.c.d`) or NAT64 well-known-prefix (`64:ff9b::a.b.c.d`) address;
/// any other address unchanged. `::` and `::1` are themselves, not 0.0.0.0
/// and 0.0.0.1.
fn carried_ipv4(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = ip else { return ip };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return v4.into();
    }
    let [s0, s1, 0, 0, 0, 0, hi, lo] = v6.segments() else { return ip };
    let compatible = s0 == 0 && s1 == 0 && (hi, lo) != (0, 0) && (hi, lo) != (0, 1);
    let nat64 = (s0, s1) == (0x64, 0xff9b);
    if compatible || nat64 {
        Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo)).into()
    } else {
        ip
    }
}

fn classify_v4(v4: Ipv4Addr) -> AddressClass {
    if v4.is_loopback() {
        AddressClass::Loopback
    } else if v4.is_link_local() {
        AddressClass::LinkLocal
    } else if v4.is_unspecified() {
        AddressClass::Unspecified
    } else if v4.is_multicast() {
        AddressClass::Multicast
    } else if v4.is_private() {
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

fn classify_v6(v6: Ipv6Addr) -> AddressClass {
    if v6.is_loopback() {
        AddressClass::Loopback
    } else if v6.is_unicast_link_local() {
        AddressClass::LinkLocal
    } else if v6.is_unspecified() {
        AddressClass::Unspecified
    } else if v6.is_multicast() {
        AddressClass::Multicast
    } else if v6.is_unique_local() {
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, AddressClass};

    fn class(s: &str) -> AddressClass {
        classify(s.parse().unwrap())
    }

    #[test]
    fn an_ipv6_address_carrying_ipv4_is_classed_as_that_ipv4_address() {
        for form in ["::ffff:127.0.0.1", "::127.0.0.1", "64:ff9b::127.0.0.1"] {
            assert_eq!(class(form), AddressClass::Loopback, "{form}");
        }
        for form in ["::ffff:169.254.169.254", "::169.254.169.254", "64:ff9b::169.254.169.254"] {
            assert_eq!(class(form), AddressClass::LinkLocal, "{form}");
        }
        for form in ["::ffff:10.0.0.1", "::10.0.0.1", "64:ff9b::192.168.1.1"] {
            assert_eq!(class(form), AddressClass::Private, "{form}");
        }
        for form in ["::ffff:93.184.216.34", "::93.184.216.34", "64:ff9b::93.184.216.34", "64:ff9b:1::127.0.0.1"] {
            assert_eq!(class(form), AddressClass::Public, "{form}");
        }
    }
}

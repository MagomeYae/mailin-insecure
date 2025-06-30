//! DNS utilities for email servers.
//!
//! Currently, DNS based blocklists and reverse DNS lookups are supported.
//! The crate also supports forward confirmed reverse dns checks.
//!
//! Because blocklists are IP4 based, these utilities only support IP4
//! addresses. IP6 addresses are converted to IP4 when possible.
//!
//! # Examples
//! ```no_run
//! use mxdns::{MxDns, FCrDNS};
//!
//! let blocklists = vec!["zen.spamhaus.org.","dnsbl-1.uceprotect.net."];
//! let mxdns = MxDns::new(blocklists).unwrap();
//!
//! // Check if an IP Address is present on blocklists
//! let is_blocked = mxdns.is_blocked([127, 0, 0, 2]).unwrap();
//! assert!(is_blocked);
//!
//! // Reverse lookup a DNS address
//! let rdns = mxdns.reverse_dns([193, 25, 101, 5]).unwrap().unwrap();
//! assert_eq!(rdns, "mail.alienscience.org.");
//!
//! // Check that the ip resolved from the name obtained by the reverse dns matches the ip
//! if let Ok(FCrDNS::Confirmed(_domain)) = mxdns.fcrdns([193, 25, 101, 5]) {
//!    // _domain is Confirmed
//! }
//! ```

#![forbid(unsafe_code)]
#![forbid(missing_docs)]
mod blocklist;
mod err;
mod join_all;

pub use crate::err::{Error, Result};
use crate::{blocklist::BlockList, join_all::join_all};
use dnsclientx::DNSClient;
use log::Level::Debug;
use log::{debug, log_enabled};
use smol::future::FutureExt;
use std::io::ErrorKind;
use std::{fs::File, io::Read, matches, net::IpAddr};

const RESOLV_CONF: &str = "/etc/resolv.conf";

/// Utilities for looking up IP addresses on blocklists and doing reverse DNS
#[derive(Clone)]
pub struct MxDns {
    bootstrap: DNSClient,
    blocklists: Vec<String>,
}

/// The result of a FCrDNS lookup
#[derive(Debug)]
pub enum FCrDNS {
    /// Reverse lookup failed
    NoReverse,
    /// Reverse lookup was successful but could not be forward confirmed
    UnConfirmed(String),
    /// The reverse lookup was forward confirmed
    Confirmed(String),
}

impl FCrDNS {
    /// Is the result a confirmed reverse dns value?
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed(_))
    }
}

impl MxDns {
    /// Create a MxDns using the system provided nameserver config
    pub fn new<S>(blocklists_fqdn: S) -> Result<Self>
    where
        S: IntoIterator,
        S::Item: Into<String>,
    {
        let mut buf = Vec::with_capacity(256);
        let mut file = File::open(RESOLV_CONF)
            .map_err(|e| Error::ResolvConfRead(RESOLV_CONF.to_string(), e))?;
        file.read_to_end(&mut buf)
            .map_err(|e| Error::ResolvConfRead(RESOLV_CONF.to_string(), e))?;
        let conf = resolv_conf::Config::parse(&buf)
            .map_err(|e| Error::ResolvConfParse(RESOLV_CONF.to_string(), e))?;
        let nameservers = conf.get_nameservers_or_local();
        if let Some(ip) = nameservers.first() {
            let ip_addr: IpAddr = ip.into();
            Ok(Self::with_dns(ip_addr, blocklists_fqdn))
        } else {
            Err(Error::NoNameservers(RESOLV_CONF.to_string()))
        }
    }

    /// Create a MxDns that uses the given DNS server for standard queries.
    pub fn with_dns<I, S>(bootstrap_dns: I, blocklists_fqdn: S) -> Self
    where
        I: Into<IpAddr>,
        S: IntoIterator,
        S::Item: Into<String>,
    {
        let ip = bootstrap_dns.into();
        let socket_addr = (ip, 53).into();
        let bootstrap = DNSClient::new(vec![socket_addr]);
        let blocklists: Vec<String> = blocklists_fqdn.into_iter().map(|i| i.into()).collect();
        Self {
            bootstrap,
            blocklists,
        }
    }

    /// Queries blocklists for the given address
    /// Returns a vector where each entry indicates if the address is on the blocklist
    pub fn on_blocklists<A>(&self, addr: A) -> Vec<Result<bool>>
    where
        A: Into<IpAddr>,
    {
        if self.blocklists.is_empty() {
            return vec![];
        }
        let ip: IpAddr = addr.into();

        let ret = smol::block_on({
            let mut all_checks = Vec::new();
            for blocklist in &self.blocklists {
                let one_check = self.check_blocklist(blocklist, ip);
                all_checks.push(one_check.boxed());
            }
            join_all(all_checks)
        });
        if log_enabled!(Debug) {
            for i in ret.iter().enumerate() {
                debug!("{} is blocked by {} = {:?}", ip, self.blocklists[i.0], i.1);
            }
        }
        ret
    }

    async fn check_blocklist(&self, blocklist: &str, ip: IpAddr) -> Result<bool> {
        let resolver = BlockList::lookup_ns(blocklist, &self.bootstrap).await?;
        let blocklist_lookup = BlockList::new(resolver, blocklist);
        blocklist_lookup.is_blocked(ip).await
    }

    /// Returns true if the address is on any of the blocklists
    pub fn is_blocked<A>(&self, addr: A) -> Result<bool>
    where
        A: Into<IpAddr>,
    {
        let mut res = self.on_blocklists(addr);
        if res.is_empty() {
            Ok(false)
        } else if res.iter().all(|r| r.is_err()) {
            res.pop().unwrap_or(Ok(false))
        } else {
            let is_blocked = res.into_iter().any(|r| r.unwrap_or(false));
            Ok(is_blocked)
        }
    }

    /// Does a reverse DNS lookup on the given ip address
    /// Returns Ok(None) if no reverse DNS entry exists.
    pub fn reverse_dns<A>(&self, ip: A) -> Result<Option<String>>
    where
        A: Into<IpAddr>,
    {
        let res = smol::block_on(self.bootstrap.query_ptr(ip.into()));
        match res {
            Ok(fqdn) => Ok(Some(fqdn)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Reverse("reverse_dns".into(), e)),
        }
    }

    /// Does a Forward Confirmed Reverse DNS check on the given ip address
    /// This checks that the reverse lookup on the ip address gives a domain
    /// name that will resolve to the original ip address.
    /// Returns the confirmed reverse DNS domain name.
    pub fn fcrdns<A>(&self, ip: A) -> Result<FCrDNS>
    where
        A: Into<IpAddr>,
    {
        let ipaddr = ip.into();
        let fqdn = match self.reverse_dns(ipaddr)? {
            None => return Ok(FCrDNS::NoReverse),
            Some(s) => s,
        };
        debug!("reverse lookup for {} = {}", ipaddr, fqdn);
        let forward = smol::block_on(self.bootstrap.query_a(&fqdn))
            .map_err(|e| Error::DnsQuery("fcrdns".to_string(), e))?;
        let is_confirmed = forward.contains(&ipaddr);
        if is_confirmed {
            Ok(FCrDNS::Confirmed(fqdn))
        } else {
            Ok(FCrDNS::UnConfirmed(fqdn))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::net::Ipv4Addr;

    const BOOTSTRAP_DNS: IpAddr = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));

    fn blocklists() -> Vec<(&'static str, bool)> {
        // Tuples are (fqdn, has_nameserver)
        vec![
            ("zen.spamhaus.org", true),
            ("bl.spamcop.net", false),
            ("dnsbl-1.uceprotect.net", true),
            ("b.barracuda.central.org", false),
            ("cbl.abuseat.org", true),
        ]
    }

    fn lookup_host(host: &str) -> Result<IpAddr> {
        let socket_addr = (BOOTSTRAP_DNS, 53).into();
        let dns = DNSClient::new(vec![socket_addr]);
        smol::block_on(dns.query_a(host))
            .and_then(|res| {
                res.first()
                    .cloned()
                    .ok_or_else(|| io::Error::other("no dns entries"))
            })
            .map_err(|e| Error::DnsQuery(host.to_string(), e))
    }

    fn build_mx_dns() -> MxDns {
        let blocklists = blocklists()
            .iter()
            .map(|t| t.0)
            .collect::<Vec<&'static str>>();
        MxDns::with_dns(BOOTSTRAP_DNS, blocklists)
    }

    #[test]
    fn empty_blocklists() {
        let empty: Vec<String> = Vec::new();
        let mxdns = MxDns::with_dns(BOOTSTRAP_DNS, empty);
        let blocked = mxdns.is_blocked(Ipv4Addr::new(127, 0, 0, 2)).unwrap();
        assert!(!blocked);
    }

    #[test]
    fn blocklist_addrs() {
        let mxdns = build_mx_dns();
        let blocklists = blocklists();
        for b in blocklists {
            let ns = smol::block_on(mxdns.bootstrap.query_ns(b.0));
            if b.1 {
                assert!(ns.is_ok(), "no NS for {}", b.0);
            } else {
                assert!(
                    matches!(&ns, Ok(v) if v.is_empty()),
                    "unexpected NS result {:?} for {}",
                    ns,
                    b.0
                );
            }
        }
    }

    #[test]
    fn not_blocked() {
        let mxdns = build_mx_dns();
        let blocked = mxdns.is_blocked([127, 0, 0, 1]).unwrap();
        assert!(!blocked);
    }

    #[test]
    fn blocked() {
        let mxdns = build_mx_dns();
        let blocked = mxdns.is_blocked([127, 0, 0, 2]).unwrap();
        assert!(blocked);
    }

    #[test]
    fn reverse_lookup() {
        let alienscience_ip =
            lookup_host("mail.alienscience.org").expect("Cannot lookup mailserver address");
        let mxdns = build_mx_dns();
        let reverse = mxdns.reverse_dns(alienscience_ip).unwrap().unwrap();
        assert_eq!(reverse, "mail.alienscience.org");
    }

    #[test]
    fn fcrdns_ok() {
        let alienscience_ip =
            lookup_host("mail.alienscience.org").expect("Cannot lookup mailserver address");
        let mxdns = build_mx_dns();
        let res = mxdns.fcrdns(alienscience_ip);
        assert!(
            matches!(res, Ok(FCrDNS::Confirmed(_))),
            "Valid mail server failed fcrdns: {:?}",
            res
        );
    }

    #[test]
    fn fcrdns_google_ok() {
        let mxdns = build_mx_dns();
        let res = mxdns.fcrdns([209, 85, 167, 66]);
        assert!(
            matches!(res, Ok(FCrDNS::Confirmed(_))),
            "Valid google server failed fcrdns: {:?}",
            res
        );
    }

    #[test]
    fn fcrdns_fail() {
        let mxdns = build_mx_dns();
        let res = mxdns.fcrdns([127, 0, 0, 2]);
        // 127.0.0.2 -> localhost -> 127.0.0.1
        assert!(
            matches!(res, Ok(FCrDNS::NoReverse) | Ok(FCrDNS::UnConfirmed(_))),
            "Known bad forward confirm failed: {:?}",
            res
        );
    }
}

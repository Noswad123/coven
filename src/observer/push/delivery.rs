use super::*;
use std::net::{IpAddr, SocketAddr};
use web_push::{
    ContentEncoding, SubscriptionInfo, VapidSignatureBuilder, WebPushMessage, WebPushMessageBuilder,
};

pub(super) fn endpoint(value: &str) -> Result<reqwest::Url> {
    if value.len() > 2048
        || !value.is_ascii()
        || value
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace() || b == b'\\')
    {
        return Err(Error("push_invalid_subscription"));
    }
    let url = reqwest::Url::parse(value).map_err(|_| Error("push_invalid_subscription"))?;
    let host = url.host_str().unwrap_or("");
    let vendor = matches!(
        host,
        "fcm.googleapis.com" | "web.push.apple.com" | "push.services.mozilla.com"
    ) || host.ends_with(".push.services.mozilla.com");
    if !vendor
        || host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        || url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path().len() <= 1
        || url.as_str() != value
    {
        return Err(Error("push_invalid_subscription"));
    }
    Ok(url)
}
pub(super) fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || b == 0 || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            // Only globally allocated unicast, excluding special/documentation blocks.
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x2002)
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
pub(super) fn message(
    sub: &Subscription,
    key: &[u8],
    subject: &str,
    alert: &str,
    ttl: u32,
) -> Result<WebPushMessage> {
    let info = SubscriptionInfo::new(
        sub.endpoint.clone(),
        sub.keys.p256dh.clone(),
        sub.keys.auth.clone(),
    );
    let mut signature =
        VapidSignatureBuilder::from_pem(key, &info).map_err(|_| Error("push_crypto_failed"))?;
    signature.add_claim("sub", subject);
    let signature = signature.build().map_err(|_| Error("push_crypto_failed"))?;
    let body = serde_json::to_vec(&json!({"version":1,"alert_id":alert})).unwrap();
    let mut builder = WebPushMessageBuilder::new(&info);
    builder.set_ttl(ttl.min(300));
    builder.set_payload(ContentEncoding::Aes128Gcm, &body);
    builder.set_vapid_signature(signature);
    builder
        .build()
        .map_err(|_| Error("push_invalid_subscription"))
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Outcome {
    Sent,
    Gone,
    Retry,
    Failed,
}
pub(super) trait Sender: Send + Sync {
    fn send(&self, message: WebPushMessage) -> Outcome;
}
pub(super) struct Network;
fn pinned_addresses(ips: impl Iterator<Item = IpAddr>) -> Result<Vec<SocketAddr>> {
    let addresses: Vec<_> = ips.take(17).map(|ip| SocketAddr::new(ip, 443)).collect();
    if addresses.is_empty() || addresses.len() > 16 || addresses.iter().any(|a| !public(a.ip())) {
        return Err(Error("push_unsafe_dns"));
    }
    Ok(addresses)
}
fn client_builder(host: &str, addresses: &[SocketAddr]) -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .pool_max_idle_per_host(0)
        .resolve_to_addrs(host, addresses)
}
impl Sender for Network {
    fn send(&self, message: WebPushMessage) -> Outcome {
        let result = (|| -> Result<Outcome> {
            let url = endpoint(&message.endpoint.to_string())?;
            let host = url.host_str().unwrap();
            let (cfg, mut opts) = hickory_resolver::system_conf::read_system_conf()
                .map_err(|_| Error("push_dns_failed"))?;
            opts.timeout = Duration::from_secs(2);
            opts.attempts = 1;
            opts.cache_size = 0;
            opts.num_concurrent_reqs = 1;
            opts.use_hosts_file = false;
            let cfg = hickory_resolver::config::ResolverConfig::from_parts(
                None,
                Vec::new(),
                cfg.name_servers()
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| Error("push_dns_failed"))?;
            let ips = runtime
                .block_on(async {
                    let resolver = hickory_resolver::TokioAsyncResolver::tokio(cfg, opts);
                    tokio::time::timeout(
                        Duration::from_secs(4),
                        resolver.lookup_ip(format!("{host}.")),
                    )
                    .await
                })
                .map_err(|_| Error("push_dns_failed"))?
                .map_err(|_| Error("push_dns_failed"))?;
            let addresses = match pinned_addresses(ips.iter()) {
                Ok(addresses) => addresses,
                Err(_) => return Ok(Outcome::Failed),
            };
            let client = client_builder(host, &addresses)
                .build()
                .map_err(|_| Error("push_transport_failed"))?;
            let request = web_push::request_builder::build_request::<Vec<u8>>(message);
            let (parts, body) = request.into_parts();
            let mut builder = client.post(url).body(body);
            // web-push uses http 0.2; reqwest uses http 1.x.
            for (name, value) in &parts.headers {
                builder = builder.header(name.as_str(), value.as_bytes());
            }
            let response = builder.send().map_err(|_| Error("push_transport_failed"))?;
            // Never read or log provider-controlled error bodies or redirect locations.
            Ok(match response.status().as_u16() {
                200..=299 => Outcome::Sent,
                404 | 410 => Outcome::Gone,
                429 | 500..=599 => Outcome::Retry,
                _ => Outcome::Failed,
            })
        })();
        result.unwrap_or(Outcome::Retry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[derive(Debug)]
    struct NoDns;
    struct FixtureThread(Option<std::thread::JoinHandle<()>>);
    impl Drop for FixtureThread {
        fn drop(&mut self) {
            if let Some(thread) = self.0.take() {
                let _ = thread.join();
            }
        }
    }
    impl reqwest::dns::Resolve for NoDns {
        fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
            panic!("unpinned DNS fallback");
        }
    }
    fn certificates() -> (
        openssl::x509::X509,
        openssl::x509::X509,
        openssl::pkey::PKey<openssl::pkey::Private>,
    ) {
        use openssl::{
            asn1::Asn1Time,
            bn::BigNum,
            ec::{EcGroup, EcKey},
            hash::MessageDigest,
            nid::Nid,
            pkey::PKey,
            x509::{
                X509, X509NameBuilder,
                extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
            },
        };
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let ca_key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
        let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "Push fixture CA").unwrap();
        let name = name.build();
        let mut ca = X509::builder().unwrap();
        ca.set_version(2).unwrap();
        ca.set_serial_number(&BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        ca.set_subject_name(&name).unwrap();
        ca.set_issuer_name(&name).unwrap();
        ca.set_pubkey(&ca_key).unwrap();
        ca.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        ca.set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        ca.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
            .unwrap();
        ca.append_extension(KeyUsage::new().key_cert_sign().build().unwrap())
            .unwrap();
        ca.sign(&ca_key, MessageDigest::sha256()).unwrap();
        let ca = ca.build();
        let mut leaf = X509::builder().unwrap();
        leaf.set_version(2).unwrap();
        leaf.set_serial_number(&BigNum::from_u32(2).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        leaf.set_subject_name(&name).unwrap();
        leaf.set_issuer_name(&name).unwrap();
        leaf.set_pubkey(&key).unwrap();
        leaf.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        leaf.set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        leaf.append_extension(BasicConstraints::new().critical().build().unwrap())
            .unwrap();
        leaf.append_extension(KeyUsage::new().digital_signature().build().unwrap())
            .unwrap();
        leaf.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
            .unwrap();
        let san = SubjectAlternativeName::new()
            .dns("fixture.invalid")
            .build(&leaf.x509v3_context(Some(&ca), None))
            .unwrap();
        leaf.append_extension(san).unwrap();
        leaf.sign(&ca_key, MessageDigest::sha256()).unwrap();
        (ca, leaf.build(), key)
    }
    #[test]
    fn pinned_tls_preserves_sni_checks_certificates_and_refuses_redirects() {
        // Only the private test builder receives loopback and a fixture trust root.
        // Production always runs vendor/public-DNS admission and system trust.
        let (ca, cert, key) = certificates();
        let mut acceptor =
            openssl::ssl::SslAcceptor::mozilla_intermediate(openssl::ssl::SslMethod::tls())
                .unwrap();
        acceptor.set_certificate(&cert).unwrap();
        acceptor.set_private_key(&key).unwrap();
        let acceptor = acceptor.build();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut accepted = 0;
            let mut successful = 0;
            while accepted < 3 {
                assert!(Instant::now() < deadline);
                let stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                };
                accepted += 1;
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                if let Ok(mut stream) = acceptor.accept(stream) {
                    assert_eq!(
                        stream.ssl().servername(openssl::ssl::NameType::HOST_NAME),
                        Some("fixture.invalid")
                    );
                    let mut bytes = [0; 4096];
                    let n = stream.read(&mut bytes).unwrap();
                    assert!(
                        std::str::from_utf8(&bytes[..n])
                            .unwrap()
                            .to_ascii_lowercase()
                            .contains("host: fixture.invalid")
                    );
                    stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://must-not-resolve.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    successful += 1;
                }
            }
            assert_eq!(successful, 1);
        });
        let mut thread = FixtureThread(Some(thread));
        let client = client_builder("fixture.invalid", &[addr])
            .add_root_certificate(reqwest::Certificate::from_der(&ca.to_der().unwrap()).unwrap())
            .dns_resolver(Arc::new(NoDns))
            .build()
            .unwrap();
        let response = client
            .post(format!("https://fixture.invalid:{}/synthetic", addr.port()))
            .body("inert")
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 302);
        let wrong = client_builder("wrong.invalid", &[addr])
            .add_root_certificate(reqwest::Certificate::from_der(&ca.to_der().unwrap()).unwrap())
            .dns_resolver(Arc::new(NoDns))
            .build()
            .unwrap();
        assert!(
            wrong
                .get(format!("https://wrong.invalid:{}/", addr.port()))
                .send()
                .is_err()
        );
        let untrusted = client_builder("fixture.invalid", &[addr])
            .dns_resolver(Arc::new(NoDns))
            .build()
            .unwrap();
        assert!(
            untrusted
                .get(format!("https://fixture.invalid:{}/", addr.port()))
                .send()
                .is_err()
        );
        thread.0.take().unwrap().join().unwrap();
        let https_only = client_builder("fixture.invalid", &[addr]).build().unwrap();
        assert!(
            https_only
                .get("http://fixture.invalid/synthetic")
                .send()
                .is_err()
        );
    }
    #[test]
    fn mixed_public_private_dns_answers_are_rejected_as_a_set() {
        assert!(
            pinned_addresses(
                ["8.8.8.8".parse().unwrap(), "127.0.0.1".parse().unwrap()].into_iter()
            )
            .is_err()
        );
        assert!(
            pinned_addresses(
                [
                    "8.8.8.8".parse().unwrap(),
                    "::ffff:8.8.8.8".parse().unwrap()
                ]
                .into_iter()
            )
            .is_err()
        );
        assert!(pinned_addresses(std::iter::repeat_n("8.8.8.8".parse().unwrap(), 17)).is_err());
    }
}

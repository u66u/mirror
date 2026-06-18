use std::net::{IpAddr, SocketAddr};

use actix_web::test::TestRequest;
use ipnet::IpNet;
use mirror_backend::http::client_ip;

#[test]
fn ignores_forwarded_header_from_untrusted_peer() -> TestResult {
    let request = TestRequest::default()
        .peer_addr(SocketAddr::from(([203, 0, 113, 10], 443)))
        .insert_header(("x-forwarded-for", "198.51.100.1"))
        .to_http_request();
    let trusted_proxies = trusted_proxies()?;

    assert_eq!(
        client_ip::client_ip(&request, &trusted_proxies),
        Some(IpAddr::from([203, 0, 113, 10]))
    );
    Ok(())
}

#[test]
fn trusts_forwarded_header_only_from_configured_proxy() -> TestResult {
    let request = TestRequest::default()
        .peer_addr(SocketAddr::from(([10, 1, 2, 3], 443)))
        .insert_header(("x-forwarded-for", "198.51.100.1, 10.1.2.3"))
        .to_http_request();
    let trusted_proxies = trusted_proxies()?;

    assert_eq!(
        client_ip::client_ip(&request, &trusted_proxies),
        Some(IpAddr::from([198, 51, 100, 1]))
    );
    Ok(())
}

type TestResult = Result<(), String>;

fn trusted_proxies() -> Result<Vec<IpNet>, String> {
    "10.0.0.0/8"
        .parse()
        .map(|network| vec![network])
        .map_err(|error: ipnet::AddrParseError| error.to_string())
}

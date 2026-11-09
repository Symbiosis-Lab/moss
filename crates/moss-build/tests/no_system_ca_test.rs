//! A Linux host with no system CA store (no `ca-certificates`, as in a minimal
//! container image) must still get an HTTPS client, from the bundled roots,
//! instead of a panic. rustls-native-certs reads `SSL_CERT_FILE` and
//! `SSL_CERT_DIR` in place of the system store, so pointing both at nothing is
//! that host. Its own test binary: the variables are process-wide.
#![cfg(target_os = "linux")]

#[test]
fn a_host_without_system_roots_still_gets_a_client() {
    std::env::set_var("SSL_CERT_FILE", "/nonexistent/ca-certificates.crt");
    std::env::set_var("SSL_CERT_DIR", "/nonexistent/certs");

    moss_build::system::proxy_reqwest::install_crypto_provider();
    let plain = reqwest::Client::builder().build();
    assert!(plain.is_err(), "precondition: with no system roots reqwest's own builder fails");

    assert!(moss_build::system::proxy_reqwest::build_client(|b| b).is_ok());
    // The client every publish uses, built where the panic was.
    let _ = moss_build::seta::client::MossSetaClient::with_url("https://example.invalid");
}

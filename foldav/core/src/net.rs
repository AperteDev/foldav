//! What every HTTPS request trusts: the system's certificate authorities, so a server whose
//! certificate comes from one the user added to the system (a self-hosted Nextcloud's own CA)
//! connects too, as in the browser. Without it ureq trusts only its built-in list and fails
//! with `UnknownIssuer`.

use ureq::tls::{RootCerts, TlsConfig};

/// For every agent's `tls_config`.
pub(crate) fn tls() -> TlsConfig {
    TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .build()
}

Test-only TLS files for `tests/integrations_contract.rs`: a CA (`ca.pem`) and a leaf certificate (`leaf.pem`,
`leaf.key`) for the integration host names, so the mock servers on 127.0.0.1 speak real TLS to the real client. The
CA's private key was deleted after it signed the leaf, so no other certificate can come from it. The app never trusts
this CA: only `HttpsClient::for_contract_tests` does. Made with LibreSSL 3.3.6, P-256, valid until 2126.

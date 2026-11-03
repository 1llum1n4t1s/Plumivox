These certificates and private keys are PUBLIC SYNTHETIC TEST FIXTURES.
They must never protect real data or be trusted by a product installation.
Generated with Git OpenSSL 3.5.9; RSA-2048 / SHA-256.
Validity: 2020-01-01 00:00:00 UTC through 2050-01-01 00:00:00 UTC.
ca-cert.der: test-only CA; its private key is not distributed.
valid-cert.pem: CA-signed server leaf with IP SAN 127.0.0.1.
wrong-cert.pem: same CA, server leaf with DNS SAN other.invalid.
*-key.pem: publicly committed private keys belonging to the synthetic leaves.
The native fixture can add only this fixed CA in a test-fixture build.

Two throwaway RSA keys, made for the tests and for nothing else. Neither is
Kick's, and nothing real is signed with either.

`test-public-key.pem` is the public half of a key pair made for the tests
(`openssl genpkey -algorithm RSA`); the private half was never kept.
`signature.b64` is
`01M38M9K3YRKV8E6Q68C18ZWG4.2026-09-24T02:35:42Z.<body.json>` signed with it
the way Kick signs a webhook (RSA, SHA-256, PKCS#1 v1.5).

`throwaway-test-key.pem` is a different key, a 1024-bit private key committed
on purpose: the webhook tests in `crates/kicksdk/src/webhooks.rs` sign
deliveries with it at run time, to check verification against bodies and
timestamps the fixtures do not have. It is public by being here, so a
signature made with it proves nothing; it is small because only the
arithmetic is under test. It lives here rather than in the crate's source so
that no published package carries a private key. A secret scanner that flags
it is right that it is a private key, and wrong that it is a secret.

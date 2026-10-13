A throwaway key pair made for the tests (`openssl genpkey -algorithm RSA`);
the private half was never kept. `signature.b64` is
`01M38M9K3YRKV8E6Q68C18ZWG4.2026-09-24T02:35:42Z.<body.json>` signed with it
the way Kick signs a webhook (RSA, SHA-256, PKCS#1 v1.5).

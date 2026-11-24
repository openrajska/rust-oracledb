# TLS test fixtures

The fork's TLS tests need a TCPS listener, which the `gvenzl/oracle-free`
image does not have. `listeners.sh` starts two next to the default one, inside
the database container:

| Listener | Port | TLS |
|---|---|---|
| `RAJSKA_TLS` | 2484 | one-way: the server's certificate only |
| `RAJSKA_MTLS` | 2485 | mutual: the client presents a certificate too |

- `server/` is the server's auto-login wallet: a certificate for `oracle`,
  `localhost` and `127.0.0.1`, signed by a throwaway test CA.
- `client/ca/ewallet.pem` holds that CA only (one-way TLS through a private
  CA). `client/mtls/ewallet.pem` holds the client's certificate, its
  unencrypted key and the CA (mTLS).

They are copies of the fixtures in php-oracle's
[`fixtures/tls`](https://github.com/openrajska/php-oracle/tree/main/fixtures/tls),
whose `make.sh` regenerates them. Test fixtures only: never use them for
anything else.

```bash
docker cp .rajska/tls <container>:/tmp/rajska-tls-src
docker exec <container> bash /tmp/rajska-tls-src/listeners.sh
```

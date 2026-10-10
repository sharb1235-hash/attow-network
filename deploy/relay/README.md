# Optional relay deployment

Attow Network's implemented private peer configuration disables relays and
public discovery. No public bootstrap endpoint or hosted relay is required by
the loopback examples or test suite.

A future relay deployment must explicitly supply authenticated endpoint
configuration, tenant/domain admission, transport quotas, operator credentials
and its own tested operational policy. This directory does not contain a
production relay service or an enabled default configuration. Forwarding a
connection never grants a ledger writer lease; see
[the peer trust boundary](../../docs/peer-trust.md).

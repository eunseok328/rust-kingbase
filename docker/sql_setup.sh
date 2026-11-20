#!/bin/bash
set -e

cat > "$PGDATA/server.key" <<-EOKEY
-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAllItXwrj62MkxKVlz2FimJk42WWc3K82Rn2vAl6z38zQxSCj
t9uWwXWTx5YOdGiUcA+JUAruZxqN7vdfphJoYtTrcrpT4rC/FsCMImBxkj1cxdYT
q94SFn9bQBRZk7RUx4Kolt+/h0d3PpNIb4DbyQ8A0MVvNVxLpRRVwc6yQP+NkRMy
gHR+m3P8fxHEtkHCVy7HORbASvN8fRlREMHDL2hkadX0BNM72DDo+DWhPA8GF6WX
tIl1gU6GP6pSbEeMHD3f+uj7f9iSjvkrHrOt2nLUQ9Qnev2nhmU0/dOIweQ17/Fr
lL9jYDUUFNORyjRnlXXUoP5BO/LdEAAqT2A0pwIDAQABAoIBAQCIXu74XUneHuiZ
Wa+eTqwC4mZXmz6OWonzs0vU65NlgksXuv+r6ZO/2GoD1Bcy9jlL3Fxm+DPF56pB
07u7TtHSb3VWdMFrU4tYGcBH45TE5dRHSmo4LlPcgxeGb6/ANwX+pYNKtJvuHyCH
7Vf2iEFcCrdjrumv0BZ0IZmXJGxEV+7mK2Og0bZ/zbmJNaH25muuWj6BKlvLhL0N
S2LlBjKx3HqtppUgUqNFqjLs6IA1u79S5dAomOsxZtnuByaX5WFzpktU2pveZmyF
cl0dwHYZIaxR3ewYeQXGF8ANUmIx3nnxD2JOysPkitaGzeqt6dQZV14tPlDZDKat
Vf0b6BHhAoGBAMWV7rG+7nVXoQ30CIcPGklkST3mVOlrzeBbKP1SeAwoGRbfsdhp
rFMkh5UxTexnOzD4O8HPuJ6NGeWRQfqZT1nnjwHPeJWtiMHT6cnWxlzvxAZ61mio
0jRfb8flhgFKk+G9+Xa6WaYAAwGWdF062EMe2Ym92oKM9ilTPGFVRk1XAoGBAMLD
ETSQd2UqTF/y7wxMPqF3l6d1KBjwpuNuin2IjkXTOfGkDnAU3mSQlr7K1IPX8NPO
gdyMfJoysfRaBuRcNA/o/0l0wyxW4HWtTtPYI0+pRCFtRLsI1MB997QKeaGKb+me
3nBXkOksPSr9oa0Cs27z2cSoBOkpq2N/zzBseHExAoGAOyq3rKBZNehEwTHnb9I0
8+9FA3U6zh9LKjkCIEGW00Uapj/cOMsEIG2a8DEwfW84SWS8OEBkr43fSGBkGo/Y
NDrkFw2ytVee0TQNGTTod6IQ2EPmera7I5XEml5/71kOyZWi40vQVqZAQDR2qgha
BFdzmwywJ1Hg0OUs+pSXlccCgYEAgyOVki80NYolovWQwFcWVOKR2s+oECL6PGlS
FvS714hCm9I7ZnymwlAZMJ6iOaRNJFEIX9i4jZtU95Mm0NzEsXHRc0SLpm9Y8+Oe
EEaYgCsZFOjePpHTr0kiYLgs7fipIkU2wa40hMyk4y2kjzoiV7MaDrCTnevQ205T
0+c1sgECgYBAXKcwdkh9JVSrLXFamsxiOx3MZ0n6J1d28wpdA3y4Y4AAJm4TGgFt
eG/6qHRy6CHdFtJ7a84EMe1jaVLQJYW/VrOC2bWLftkU7qaOnkXHvr4CAHsXQHcx
JhLfvh4ab3KyoK/iimifvcoS5z9gp7IBFKMyh5IeJ9Y75TgcfJ5HMg==
-----END RSA PRIVATE KEY-----
EOKEY
chmod 0600 "$PGDATA/server.key"

cat > "$PGDATA/server.crt" <<-EOCERT
-----BEGIN CERTIFICATE-----
MIID6DCCAtCgAwIBAgIUXzTeUOM3poOVdavBg/7O2zDiVVIwDQYJKoZIhvcNAQEL
BQAwWTELMAkGA1UEBhMCQVUxEzARBgNVBAgTClNvbWUtU3RhdGUxITAfBgNVBAoT
GEludGVybmV0IFdpZGdpdHMgUHR5IEx0ZDESMBAGA1UEAxMJbG9jYWxob3N0MB4X
DTI2MDEwMTAwMDAwMFoXDTM2MDEwMTAwMDAwMFowWTELMAkGA1UEBhMCQVUxEzAR
BgNVBAgTClNvbWUtU3RhdGUxITAfBgNVBAoTGEludGVybmV0IFdpZGdpdHMgUHR5
IEx0ZDESMBAGA1UEAxMJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A
MIIBCgKCAQEAllItXwrj62MkxKVlz2FimJk42WWc3K82Rn2vAl6z38zQxSCjt9uW
wXWTx5YOdGiUcA+JUAruZxqN7vdfphJoYtTrcrpT4rC/FsCMImBxkj1cxdYTq94S
Fn9bQBRZk7RUx4Kolt+/h0d3PpNIb4DbyQ8A0MVvNVxLpRRVwc6yQP+NkRMygHR+
m3P8fxHEtkHCVy7HORbASvN8fRlREMHDL2hkadX0BNM72DDo+DWhPA8GF6WXtIl1
gU6GP6pSbEeMHD3f+uj7f9iSjvkrHrOt2nLUQ9Qnev2nhmU0/dOIweQ17/FrlL9j
YDUUFNORyjRnlXXUoP5BO/LdEAAqT2A0pwIDAQABo4GnMIGkMA8GA1UdEwEB/wQF
MAMBAf8wDgYDVR0PAQH/BAQDAgGmMBMGA1UdJQQMMAoGCCsGAQUFBwMBMCwGA1Ud
EQQlMCOCCWxvY2FsaG9zdIcEfwAAAYcQAAAAAAAAAAAAAAAAAAAAATAdBgNVHQ4E
FgQUEcuoFxzUZ4VV9VPv5frDyIuFA5cwHwYDVR0jBBgwFoAUEcuoFxzUZ4VV9VPv
5frDyIuFA5cwDQYJKoZIhvcNAQELBQADggEBAJUxEzFPaMgLP9AnT+i1VCTWBKHp
uoOeVuQgWvLlWjND9lBZHkrfrVOY20Ez1s1ybZax+fo9fKv0pcOCFuokmSF1VlzC
rmISEkQJ+tsrMjBfWzsr2q1fxMV2XIEikRrGEQ61gC37As4MuDNI6DiMikWhS8Av
lpfTXGQUJJcYcHSaRulq/eF7qV3KM8y0VTavpMPnJvwPQeNWggOaTCC0nFLHNVif
HZbzYgadW8RpF1BlwGK5kljgmfb2S/jtdfMBMGvKd1cZ1eVqnqOEfLMwLcmUe2cF
4WNDqFatOBAaqpn9C0zdC61BtjJbu6l9am273hyGPmLqtVeeneUSCwei+1g=
-----END CERTIFICATE-----
EOCERT

cat >> "$PGDATA/postgresql.conf" <<-EOCONF
port = 5433
ssl = on
ssl_cert_file = 'server.crt'
ssl_key_file = 'server.key'
EOCONF

cat > "$PGDATA/pg_hba.conf" <<-EOCONF
# TYPE  DATABASE        USER            ADDRESS                 METHOD
host    all             pass_user       0.0.0.0/0            password
host    all             md5_user        0.0.0.0/0            md5
host    all             scram_user      0.0.0.0/0            scram-sha-256
host    all             pass_user       ::0/0                password
host    all             md5_user        ::0/0                md5
host    all             scram_user      ::0/0                scram-sha-256

hostssl all             ssl_user        0.0.0.0/0            trust
hostssl all             ssl_user        ::0/0                trust
host    all             ssl_user        0.0.0.0/0            reject
host    all             ssl_user        ::0/0                reject

# IPv4 local connections:
host    all             postgres        0.0.0.0/0            trust
# IPv6 local connections:
host    all             postgres        ::0/0                trust
# Unix socket connections:
local   all             postgres                             trust
EOCONF

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" <<-EOSQL
    CREATE ROLE pass_user PASSWORD 'password' LOGIN;
    CREATE ROLE md5_user PASSWORD 'password' LOGIN;
    SET password_encryption TO 'scram-sha-256';
    CREATE ROLE scram_user PASSWORD 'password' LOGIN;
    CREATE ROLE ssl_user LOGIN;
    CREATE EXTENSION hstore;
    CREATE EXTENSION citext;
    CREATE EXTENSION ltree;
EOSQL

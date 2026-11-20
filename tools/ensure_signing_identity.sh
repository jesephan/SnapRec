#!/bin/bash
# Creates (once) a self-signed code-signing certificate in the login keychain.
# Signing every build with the same identity keeps macOS privacy permissions
# (Screen Recording) valid across rebuilds; ad-hoc signatures change each build.
set -euo pipefail
NAME="${1:-SnapRec Local Signing}"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  exit 0
fi

echo "Creating code-signing certificate \"$NAME\" in your login keychain…"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cat > "$TMP/cert.cnf" <<EOF
[ req ]
distinguished_name = dn
prompt = no
x509_extensions = ext
[ dn ]
CN = $NAME
[ ext ]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
OPENSSL=/usr/bin/openssl   # LibreSSL: produces a PKCS#12 that `security` can import
$OPENSSL req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$TMP/cert.cnf" \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" 2>/dev/null
$OPENSSL pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -name "$NAME" \
  -passout pass:snaprec -out "$TMP/id.p12" 2>/dev/null
security import "$TMP/id.p12" -k "$KEYCHAIN" -P snaprec -T /usr/bin/codesign >/dev/null
echo "Created \"$NAME\"."

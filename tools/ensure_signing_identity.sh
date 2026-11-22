#!/bin/bash
# Makes sure a usable self-signed code-signing identity exists in the login
# keychain, creating it once if needed. Signing every build with the same
# identity keeps macOS privacy permissions (Screen Recording) valid across
# rebuilds; ad-hoc signatures change each build.
#
# Exits 0 only if codesign can actually sign with the identity (a cert
# without its private key, or a key codesign is not allowed to use, fails).
set -euo pipefail
NAME="${1:-SnapRec Local Signing}"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

has_identity() {
  # "Matching identities" includes self-signed (untrusted) ones, which codesign accepts.
  security find-identity -p codesigning "$KEYCHAIN" 2>/dev/null | grep -q "\"$NAME\""
}

can_sign() {
  local tmp
  tmp=$(mktemp -d)
  cp /usr/bin/true "$tmp/probe"
  if codesign --force --sign "$NAME" "$tmp/probe" >/dev/null 2>&1 && codesign --verify "$tmp/probe" >/dev/null 2>&1; then
    rm -rf "$tmp"; return 0
  fi
  rm -rf "$tmp"; return 1
}

if ! has_identity; then
  # A certificate left behind without its private key can't sign; remove it.
  while security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; do
    security delete-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1 || break
  done

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
  security import "$TMP/id.p12" -k "$KEYCHAIN" -P snaprec \
    -T /usr/bin/codesign -T /usr/bin/security >/dev/null
  has_identity || { echo "Signing identity was not created." >&2; exit 1; }
  echo "Created \"$NAME\"."
fi

if ! can_sign; then
  echo "codesign cannot use \"$NAME\" (keychain locked or access denied)." >&2
  echo "Unlock it with:  security unlock-keychain ~/Library/Keychains/login.keychain-db" >&2
  exit 1
fi

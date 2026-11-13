#!/bin/bash
# One-time setup: creates a local code-signing certificate ("MacSweep Local Signing")
# in your login keychain. bundle.sh then signs with it instead of an ad-hoc
# signature, so macOS keeps Full Disk Access across rebuilds.
#
# The certificate is only trusted on this Mac and is used only for code signing.
# Remove it any time in Keychain Access (search "MacSweep Local Signing").
set -euo pipefail

NAME="MacSweep Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
OPENSSL=/usr/bin/openssl   # Apple's LibreSSL; Homebrew OpenSSL 3 makes .p12 files macOS can't import

if security find-identity -p codesigning "$KEYCHAIN" 2>/dev/null | grep -q "$NAME"; then
    echo "✓ '$NAME' already exists. Nothing to do."
    exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

cat > "$TMP/cert.cfg" <<CFG
[ req ]
distinguished_name = dn
x509_extensions    = ext
prompt             = no
[ dn ]
CN = $NAME
[ ext ]
basicConstraints = critical,CA:false
keyUsage         = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CFG

"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$TMP/cert.cfg" \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" 2>/dev/null
"$OPENSSL" pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
    -out "$TMP/id.p12" -passout pass:macsweep -name "$NAME"

security import "$TMP/id.p12" -k "$KEYCHAIN" -P macsweep -T /usr/bin/codesign >/dev/null
echo "macOS will now ask for your password to trust the certificate for code signing…"
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$TMP/cert.pem"

echo "✓ Created '$NAME'."
echo "Next: ./bundle.sh, move the app to /Applications, and grant Full Disk Access one last time."

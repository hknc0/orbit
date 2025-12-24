#!/bin/bash
set -e

CERT_FILE="/app/certs/cert.pem"
KEY_FILE="/app/certs/key.pem"

# Generate self-signed certificates if they don't exist
if [ ! -f "$CERT_FILE" ] || [ ! -f "$KEY_FILE" ]; then
    echo "=== Generating self-signed certificates ==="
    # Run from /app/scripts so ../certs resolves to /app/certs
    mkdir -p /app/scripts
    cd /app/scripts
    /app/gen-dev-cert
    cd /app
    echo "=== Certificates generated ==="
fi

# Output certificate hash for client configuration
if [ -f "$CERT_FILE" ]; then
    echo "=== Certificate Info ==="
    CERT_HASH=$(openssl x509 -in "$CERT_FILE" -outform DER 2>/dev/null | openssl dgst -sha256 -binary | base64)
    echo "Certificate hash (SHA-256): $CERT_HASH"
    echo "Use this hash in client/.env: VITE_CERT_HASH=$CERT_HASH"
    echo "========================="
fi

# Export TLS paths for the server
export TLS_CERT_PATH="$CERT_FILE"
export TLS_KEY_PATH="$KEY_FILE"

# Start the server
echo "=== Starting Orbit Royale Server ==="
exec /app/server

# Orbit Royale Server

WebTransport-based multiplayer game server for Orbit Royale.

## Quick Start (Development)

```bash
# From project root (one-time setup)
make setup    # Generate dev certificates

# Start servers (separate terminals)
make api      # Start API server
make client   # Start client

# Open browser
make chrome   # Chrome with cert bypass
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BIND_ADDRESS` | Server bind address | `0.0.0.0` |
| `PORT` | Server port | `4433` |
| `MAX_ROOMS` | Maximum concurrent game rooms | `100` |
| `TLS_CERT_PATH` | Path to TLS certificate (PEM) | - |
| `TLS_KEY_PATH` | Path to TLS private key (PEM) | - |

### TLS Certificate Handling

#### Development Mode

Run `make setup` once to generate dev certificates:

```bash
make setup
```

This generates:
- `certs/cert.pem` - Self-signed certificate (valid 10 years, localhost only)
- `certs/key.pem` - Private key

The script outputs the certificate hash. Update `client/.env`:
```
VITE_CERT_HASH=<hash from setup output>
```

The `certs/` directory is gitignored. Each developer runs `make setup` once.

#### Production Mode

Set `TLS_CERT_PATH` and `TLS_KEY_PATH` to use CA-signed certificates:

```bash
export TLS_CERT_PATH=/etc/ssl/certs/orbit-royale.pem
export TLS_KEY_PATH=/etc/ssl/private/orbit-royale.key
```

**Requirements:**
- Certificate must be valid for your domain
- Certificate must support TLS 1.3 (required for WebTransport)
- ECDSA or RSA keys supported

**Recommended:** Use Let's Encrypt with certbot or similar ACME client.

## Production Deployment

### Docker

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/orbit-royale-server /usr/local/bin/
EXPOSE 4433/udp
CMD ["orbit-royale-server"]
```

### Kubernetes

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: orbit-royale-tls
type: kubernetes.io/tls
data:
  tls.crt: <base64-encoded-cert>
  tls.key: <base64-encoded-key>
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: orbit-royale
spec:
  replicas: 1
  selector:
    matchLabels:
      app: orbit-royale
  template:
    metadata:
      labels:
        app: orbit-royale
    spec:
      containers:
      - name: server
        image: orbit-royale-server:latest
        ports:
        - containerPort: 4433
          protocol: UDP
        env:
        - name: TLS_CERT_PATH
          value: /tls/tls.crt
        - name: TLS_KEY_PATH
          value: /tls/tls.key
        volumeMounts:
        - name: tls
          mountPath: /tls
          readOnly: true
      volumes:
      - name: tls
        secret:
          secretName: orbit-royale-tls
```

### Load Balancing

WebTransport uses QUIC (UDP), so standard HTTP load balancers won't work. Options:

1. **Direct exposure** - Expose server directly with DNS round-robin
2. **QUIC-aware proxy** - Use Cloudflare, HAProxy 2.6+, or nginx with QUIC support
3. **Cloud UDP LB** - GCP UDP Load Balancer, AWS NLB (UDP mode)

**Important:** QUIC connection migration requires sticky sessions or connection ID routing.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    WebTransport Server                   │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │   Lobby     │  │    Game     │  │   Anti-Cheat    │  │
│  │   Manager   │  │   Rooms     │  │   System        │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │    TLS      │  │    DoS      │  │    Protocol     │  │
│  │   Config    │  │  Protection │  │    Codec        │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Testing

```bash
cargo test              # Run all tests
cargo test --release    # Run with optimizations
```

## License

MIT

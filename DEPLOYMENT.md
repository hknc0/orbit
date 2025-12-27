# Orbit Royale - Deployment Guide

## Executive Summary

Orbit Royale is a **real-time multiplayer browser game** requiring:
- **WebTransport/QUIC** over UDP (port 4433)
- **30 Hz tick rate** (~33ms latency budget)
- **TLS 1.3** mandatory for WebTransport
- **Stateless instances** (no database)
- **~1 CPU, 512MB RAM** per game server

---

## Recommended Stack: Hetzner Cloud + Cloudflare

### Why Hetzner for EU + High Scale

| Aspect | Hetzner Advantage |
|--------|-------------------|
| Location | Falkenstein, Nuremberg, Helsinki - Perfect for EU |
| Cost | €15-30/mo for 500+ players vs $100+/mo elsewhere |
| Bandwidth | 20TB FREE egress/mo |
| Performance | Dedicated vCPUs, NVMe storage |
| Network | Low-latency EU backbone |

---

## Capacity Planning

Each game server instance handles ~150 concurrent players/bots.

| Server | vCPU | RAM | Price | Game Instances | Players |
|--------|------|-----|-------|----------------|---------|
| CX22 | 2 | 4GB | €3.79/mo | 2-3 | ~300-450 |
| CX32 | 4 | 8GB | €7.59/mo | 4-6 | ~600-900 |
| **CX42** | 8 | 16GB | €14.99/mo | 8-12 | ~1200-1800 |
| CX52 | 16 | 32GB | €29.99/mo | 16-24 | ~2400-3600 |

**Recommended: 1x CX42 (€15/mo) + 1x CX22 (€4/mo) for redundancy = €19/mo total**

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Cloudflare (Free Tier)                       │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ DNS + DDoS Protection + Static Assets (Pages)              │  │
│  │ orbit-royale.com → Hetzner origin                          │  │
│  └───────────────────────────┬───────────────────────────────┘  │
└──────────────────────────────┼──────────────────────────────────┘
                               │
        ┌──────────────────────┴──────────────────────┐
        │                                             │
        ▼                                             ▼
┌───────────────────────────┐       ┌───────────────────────────┐
│  Hetzner CX42 (Primary)   │       │  Hetzner CX22 (Backup)    │
│  Falkenstein, Germany     │       │  Helsinki, Finland        │
│  €14.99/mo                │       │  €3.79/mo                 │
│  ─────────────────────    │       │  ─────────────────────    │
│  • 8 vCPU, 16GB RAM       │       │  • 2 vCPU, 4GB RAM        │
│  • 8-12 game instances    │       │  • 2-3 game instances     │
│  • Primary for EU West    │       │  • Backup for EU North    │
│  • Prometheus + Grafana   │       │  • Game servers only      │
└───────────────────────────┘       └───────────────────────────┘

Total: ~€19/mo (~$20/mo) for 500-1500 concurrent players
```

---

## Cost Comparison (500+ Players, EU)

| Provider | Monthly Cost | Notes |
|----------|--------------|-------|
| **Hetzner** | **€19/mo** | CX42 + CX22 |
| Fly.io | ~$50-100/mo | Multiple instances + egress |
| DigitalOcean | ~$48/mo | 2x Premium Droplets |
| AWS | ~$100-200/mo | EC2 + data transfer |
| Vultr | ~$48/mo | 2x Cloud Compute |

**Hetzner saves 60-90% vs alternatives at this scale.**

---

## Quick Start

### 1. Create Hetzner Server

```bash
# Create CX42 server in Falkenstein datacenter via Hetzner Cloud Console
# Select Ubuntu 22.04 LTS
```

### 2. Initial Server Setup

```bash
# SSH into server
ssh root@your-server-ip

# Update system
apt update && apt upgrade -y

# Install Docker
curl -fsSL https://get.docker.com | sh

# Install Docker Compose
apt install docker-compose-plugin -y

# Set up firewall
ufw allow 22/tcp    # SSH
ufw allow 443/tcp   # HTTPS
ufw allow 4433/udp  # WebTransport
ufw allow 4434/udp  # WebTransport (instance 2)
ufw allow 4435/udp  # WebTransport (instance 3)
ufw enable
```

### 3. Clone and Deploy

```bash
# Clone repository
git clone https://github.com/your-repo/orbit-royale.git
cd orbit-royale

# Deploy
docker compose -f docker-compose.prod.yml up -d
```

### 4. Set Up TLS Certificates

```bash
# Install certbot
apt install certbot -y

# Get certificates (stop any service on port 443 first)
certbot certonly --standalone -d orbit.yourdomain.com

# Certificates will be at:
# /etc/letsencrypt/live/orbit.yourdomain.com/fullchain.pem
# /etc/letsencrypt/live/orbit.yourdomain.com/privkey.pem
```

---

## Production Docker Compose

Create `docker-compose.prod.yml`:

```yaml
version: '3.8'

services:
  orbit-api-1:
    build:
      context: .
      dockerfile: docker/api/Dockerfile
    ports:
      - "4433:4433/udp"
      - "9090:9090"
    environment:
      - RUST_LOG=info
      - HOST=0.0.0.0
      - PORT=4433
      - METRICS_PORT=9090
      - BOT_COUNT=150
    volumes:
      - /etc/letsencrypt:/etc/letsencrypt:ro
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
        reservations:
          cpus: '1'
          memory: 1G
    restart: always
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:9090/health"]
      interval: 10s
      timeout: 3s
      retries: 3

  orbit-api-2:
    build:
      context: .
      dockerfile: docker/api/Dockerfile
    ports:
      - "4434:4433/udp"
      - "9091:9090"
    environment:
      - RUST_LOG=info
      - HOST=0.0.0.0
      - PORT=4433
      - METRICS_PORT=9090
      - BOT_COUNT=150
    volumes:
      - /etc/letsencrypt:/etc/letsencrypt:ro
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
        reservations:
          cpus: '1'
          memory: 1G
    restart: always

  orbit-api-3:
    build:
      context: .
      dockerfile: docker/api/Dockerfile
    ports:
      - "4435:4433/udp"
      - "9092:9090"
    environment:
      - RUST_LOG=info
      - HOST=0.0.0.0
      - PORT=4433
      - METRICS_PORT=9090
      - BOT_COUNT=150
    volumes:
      - /etc/letsencrypt:/etc/letsencrypt:ro
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
    restart: always

  prometheus:
    image: prom/prometheus:v2.48.0
    ports:
      - "9093:9090"
    volumes:
      - ./docker/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--storage.tsdb.retention.time=7d'
    restart: always

  grafana:
    image: grafana/grafana:10.2.2
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=${GRAFANA_PASSWORD:-changeme}
      - GF_USERS_ALLOW_SIGN_UP=false
    volumes:
      - ./docker/grafana/provisioning:/etc/grafana/provisioning
      - ./docker/grafana/dashboards:/var/lib/grafana/dashboards
      - grafana-data:/var/lib/grafana
    restart: always

volumes:
  prometheus-data:
  grafana-data:

networks:
  default:
    name: orbit-network
```

---

## Cloudflare Setup

### DNS Records

```
Type  Name              Content              Proxy
─────────────────────────────────────────────────────
A     orbit.game        YOUR_HETZNER_IP      OFF (DNS only)
A     eu1.orbit.game    YOUR_HETZNER_IP      OFF (DNS only)
A     eu2.orbit.game    YOUR_BACKUP_IP       OFF (DNS only)
CNAME www               orbit.pages.dev      ON
```

**Important:** Disable Cloudflare proxy (orange cloud) for game servers - UDP is not supported through the proxy.

### Cloudflare Pages (Static Assets)

```bash
# Build client
cd client
npm run build

# Deploy to Cloudflare Pages
npx wrangler pages deploy dist --project-name=orbit-royale
```

---

## Load Balancing Strategy

Since QUIC connections are stateful, use client-side server selection:

```javascript
// Client-side server selection
const EU_SERVERS = [
  { host: 'eu1.orbit.game', port: 4433 },
  { host: 'eu1.orbit.game', port: 4434 },
  { host: 'eu1.orbit.game', port: 4435 },
  { host: 'eu2.orbit.game', port: 4433 },
];

async function selectServer() {
  const results = await Promise.all(
    EU_SERVERS.map(async (server) => {
      const start = performance.now();
      try {
        // Ping test implementation
        const latency = await pingServer(server);
        return { ...server, latency };
      } catch {
        return { ...server, latency: Infinity };
      }
    })
  );

  return results
    .filter(s => s.latency < Infinity)
    .sort((a, b) => a.latency - b.latency)[0];
}
```

---

## Scaling Path

| Players | Configuration | Monthly Cost |
|---------|---------------|--------------|
| 0-500 | 1x CX42 | €15/mo |
| 500-1500 | 1x CX42 + 1x CX22 | €19/mo |
| 1500-3000 | 2x CX42 | €30/mo |
| 3000-5000 | 1x CX52 + 1x CX42 | €45/mo |
| 5000+ | Multiple CX52 | €60+/mo |

---

## Auto-Scaling: Multi-Galaxy Architecture

### Concept

Instead of scaling a single game world, spawn **multiple galaxies** (parallel universes) where each galaxy is an independent game instance. Players seamlessly join available galaxies or migrate between them.

```
┌─────────────────────────────────────────────────────────────────┐
│                      Galaxy Orchestrator                         │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ • Monitors galaxy health & player counts                    ││
│  │ • Spawns new galaxies when capacity reached                 ││
│  │ • Provides galaxy discovery API for clients                 ││
│  │ • Terminates empty galaxies after cooldown                  ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                               │
       ┌───────────────────────┼───────────────────────┐
       │                       │                       │
       ▼                       ▼                       ▼
┌─────────────┐         ┌─────────────┐         ┌─────────────┐
│  Galaxy α   │         │  Galaxy β   │         │  Galaxy γ   │
│  Port 4433  │         │  Port 4434  │         │  Port 4435  │
│  120/150    │         │  80/150     │         │  45/150     │
│  players    │         │  players    │         │  players    │
└─────────────┘         └─────────────┘         └─────────────┘
       │                       │                       │
       └───────────────────────┴───────────────────────┘
                               │
                        ┌──────┴──────┐
                        │   Client    │
                        │ (connects   │
                        │ to least    │
                        │ crowded)    │
                        └─────────────┘
```

### Scaling Rules

| Condition | Action |
|-----------|--------|
| All galaxies > 80% capacity | Spawn new galaxy |
| Galaxy empty for 5+ minutes | Terminate galaxy |
| Galaxy unhealthy (no heartbeat) | Mark unavailable, respawn |
| Player count spike detected | Pre-spawn galaxies |

### Implementation Options

#### Option A: Simple Orchestrator (Recommended for Start)

A lightweight service that manages galaxy instances via Docker API:

```yaml
# docker-compose.prod.yml
services:
  orchestrator:
    build: ./docker/orchestrator
    ports:
      - "8080:8080"  # Galaxy discovery API
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
    environment:
      - MIN_GALAXIES=1
      - MAX_GALAXIES=10
      - PLAYERS_PER_GALAXY=150
      - SCALE_UP_THRESHOLD=0.8    # 80% capacity
      - SCALE_DOWN_DELAY=300      # 5 min cooldown
      - GALAXY_IMAGE=orbit-api:latest
      - BASE_PORT=4433
```

**Orchestrator API:**
```
GET  /galaxies          → List available galaxies with player counts
GET  /galaxies/best     → Get recommended galaxy (least crowded)
POST /galaxies          → Force spawn new galaxy (admin)
DELETE /galaxies/:id    → Force terminate galaxy (admin)
GET  /health            → Orchestrator health check
```

**Simple Orchestrator Logic (Rust/Node):**
```rust
// Pseudo-code for orchestrator
async fn check_scaling() {
    let galaxies = get_running_galaxies().await;

    // Scale up: all galaxies above threshold
    let all_busy = galaxies.iter()
        .all(|g| g.player_count as f32 / g.capacity as f32 > 0.8);

    if all_busy && galaxies.len() < MAX_GALAXIES {
        spawn_galaxy().await;
    }

    // Scale down: empty galaxies past cooldown
    for galaxy in &galaxies {
        if galaxy.player_count == 0
           && galaxy.empty_since.elapsed() > Duration::from_secs(300)
           && galaxies.len() > MIN_GALAXIES {
            terminate_galaxy(galaxy.id).await;
        }
    }
}
```

#### Option B: Hetzner Cloud API Auto-Scaling

Use Hetzner's API to spin up/down entire servers for large scale:

```bash
# Create new server via Hetzner API
curl -X POST "https://api.hetzner.cloud/v1/servers" \
  -H "Authorization: Bearer $HETZNER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "galaxy-4",
    "server_type": "cx22",
    "image": "docker-ce",
    "location": "fsn1",
    "user_data": "#cloud-init script to start galaxy"
  }'
```

**Hetzner Scaling Tiers:**
```
Tier 1 (0-500 players):     1x CX42 with 3-4 galaxies
Tier 2 (500-2000 players):  1x CX42 + 1x CX22 (auto-spawned)
Tier 3 (2000-5000 players): 2x CX42 (auto-spawned)
Tier 4 (5000+ players):     Multiple CX52 (auto-spawned)
```

#### Option C: Kubernetes (Future Scale)

For massive scale, use Kubernetes with Horizontal Pod Autoscaler:

```yaml
# k8s/galaxy-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: orbit-galaxy
spec:
  replicas: 1
  selector:
    matchLabels:
      app: orbit-galaxy
  template:
    spec:
      containers:
      - name: galaxy
        image: orbit-api:latest
        ports:
        - containerPort: 4433
          protocol: UDP
        resources:
          requests:
            cpu: "500m"
            memory: "512Mi"
          limits:
            cpu: "1000m"
            memory: "1Gi"
---
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: orbit-galaxy-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: orbit-galaxy
  minReplicas: 1
  maxReplicas: 20
  metrics:
  - type: Pods
    pods:
      metric:
        name: orbit_players_total
      target:
        type: AverageValue
        averageValue: "120"  # Scale when avg > 120 players/galaxy
```

### Client-Side Galaxy Selection

Update client to discover and select galaxies:

```typescript
// client/src/net/GalaxySelector.ts
interface Galaxy {
  id: string;
  host: string;
  port: number;
  playerCount: number;
  capacity: number;
  region: string;
  ping?: number;
}

const ORCHESTRATOR_URL = 'https://api.orbit.game';

export async function selectBestGalaxy(): Promise<Galaxy> {
  // 1. Fetch available galaxies
  const response = await fetch(`${ORCHESTRATOR_URL}/galaxies`);
  const galaxies: Galaxy[] = await response.json();

  // 2. Filter by capacity (exclude full galaxies)
  const available = galaxies.filter(g => g.playerCount < g.capacity * 0.95);

  // 3. Ping test each galaxy
  const withPing = await Promise.all(
    available.map(async (galaxy) => ({
      ...galaxy,
      ping: await measurePing(galaxy.host, galaxy.port)
    }))
  );

  // 4. Score: prefer low ping + moderate population
  const scored = withPing.map(g => ({
    ...g,
    score: g.ping + (g.playerCount < 50 ? 50 : 0) // Slight preference for populated
  }));

  // 5. Return best option
  return scored.sort((a, b) => a.score - b.score)[0];
}

// Usage in game init
async function joinGame(playerName: string) {
  const galaxy = await selectBestGalaxy();

  // Show galaxy info to player
  ui.showStatus(`Joining Galaxy ${galaxy.id} (${galaxy.playerCount} players)`);

  // Connect to selected galaxy
  const transport = new WebTransport(`https://${galaxy.host}:${galaxy.port}`);
  await transport.connect();

  // Join the game
  sendJoinRequest(transport, playerName);
}
```

### Galaxy Migration (Optional)

Allow players to switch galaxies mid-session:

```typescript
// Client-side galaxy switching
async function switchGalaxy(targetGalaxyId: string) {
  // 1. Gracefully disconnect from current
  await currentTransport.sendLeave();
  await currentTransport.close();

  // 2. Fetch target galaxy info
  const galaxy = await fetch(`${ORCHESTRATOR_URL}/galaxies/${targetGalaxyId}`);

  // 3. Connect to new galaxy
  const newTransport = new WebTransport(`https://${galaxy.host}:${galaxy.port}`);
  await newTransport.connect();

  // 4. Rejoin with same player name
  sendJoinRequest(newTransport, playerName);

  // 5. Update UI
  ui.showStatus(`Switched to Galaxy ${galaxy.id}`);
}
```

### Galaxy Browser UI

Add a galaxy selection screen:

```
┌────────────────────────────────────────────────────────┐
│                   SELECT GALAXY                        │
├────────────────────────────────────────────────────────┤
│                                                        │
│  ○ Galaxy Alpha    [████████░░] 120/150   12ms  [JOIN] │
│  ○ Galaxy Beta     [█████░░░░░]  75/150   15ms  [JOIN] │
│  ○ Galaxy Gamma    [███░░░░░░░]  45/150   18ms  [JOIN] │
│  ○ Galaxy Delta    [█░░░░░░░░░]  12/150   14ms  [JOIN] │
│                                                        │
│  [AUTO-SELECT BEST]              [REFRESH]             │
│                                                        │
└────────────────────────────────────────────────────────┘
```

### Metrics for Auto-Scaling

Add galaxy-level metrics to Prometheus:

```rust
// api/src/metrics.rs additions
lazy_static! {
    pub static ref GALAXY_ID: String = std::env::var("GALAXY_ID")
        .unwrap_or_else(|_| "default".to_string());

    pub static ref GALAXY_PLAYERS: IntGauge = register_int_gauge!(
        "orbit_galaxy_players",
        "Current player count in this galaxy"
    ).unwrap();

    pub static ref GALAXY_CAPACITY: IntGauge = register_int_gauge!(
        "orbit_galaxy_capacity",
        "Max player capacity for this galaxy"
    ).unwrap();
}
```

Orchestrator scrapes all galaxies:
```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'galaxies'
    static_configs:
      - targets: ['galaxy-1:9090', 'galaxy-2:9090', 'galaxy-3:9090']
    relabel_configs:
      - source_labels: [__address__]
        target_label: galaxy
```

### Recommended Implementation Order

1. **Phase 1**: Static multi-galaxy (manual docker-compose with 2-3 galaxies)
2. **Phase 2**: Simple orchestrator (Docker API-based, single server)
3. **Phase 3**: Hetzner API scaling (multi-server, auto-provision)
4. **Phase 4**: Kubernetes (if 5000+ concurrent players)

---

## Monitoring

### Access Grafana

```
URL: http://your-server-ip:3000
User: admin
Password: (set via GRAFANA_PASSWORD env var)
```

### Key Metrics to Watch

- `orbit_players_total` - Current player count
- `orbit_tick_duration_seconds` - Game loop performance
- `orbit_network_bytes_sent_total` - Bandwidth usage
- `orbit_connections_active` - Active connections

### Alerting (Optional)

Add to Prometheus alerts:
```yaml
groups:
  - name: orbit-alerts
    rules:
      - alert: HighTickDuration
        expr: orbit_tick_duration_seconds > 0.05
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "Game tick taking too long"

      - alert: ServerDown
        expr: up{job="orbit-api"} == 0
        for: 30s
        labels:
          severity: critical
```

---

## Maintenance

### Update Deployment

```bash
cd orbit-royale
git pull
docker compose -f docker-compose.prod.yml build
docker compose -f docker-compose.prod.yml up -d
```

### View Logs

```bash
# All services
docker compose -f docker-compose.prod.yml logs -f

# Specific service
docker compose -f docker-compose.prod.yml logs -f orbit-api-1
```

### Restart Services

```bash
docker compose -f docker-compose.prod.yml restart
```

### Certificate Renewal

```bash
# Auto-renewal (add to crontab)
0 0 1 * * certbot renew --quiet && docker compose -f docker-compose.prod.yml restart
```

---

## Troubleshooting

### Connection Issues

1. Check firewall: `ufw status`
2. Check ports: `netstat -tulpn | grep 4433`
3. Check Docker: `docker compose -f docker-compose.prod.yml ps`

### High Latency

1. Check tick duration in Grafana
2. Reduce BOT_COUNT if CPU-bound
3. Check network with `iperf3`

### Memory Issues

1. Check with `docker stats`
2. Adjust memory limits in compose file
3. Reduce concurrent game instances

---

## Alternative Providers Comparison

| Provider | UDP | Latency | Cost/mo | Best For |
|----------|-----|---------|---------|----------|
| **Hetzner** | ✅ | ✅ EU | €19 | **EU Production** |
| Fly.io | ✅ | ✅ Global | $50-100 | Global reach |
| DigitalOcean | ✅ | ⚠️ | $48 | Simple VPS |
| AWS GameLift | ✅ | ✅ | $100-200 | Enterprise |
| Vultr | ✅ | ✅ | $48 | Bare metal |
| Railway | ❌ | - | - | Not suitable |
| Render | ⚠️ | ⚠️ | - | Not recommended |

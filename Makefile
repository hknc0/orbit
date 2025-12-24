# Orbit Royale - Local Development Makefile

API_PORT := 4433
CLIENT_PORT := 5173

.PHONY: all setup dev api client build clean stop kill chrome help docker-clean clean-all

help:
	@echo "Orbit Royale Development Commands:"
	@echo ""
	@echo "  make setup        - Generate dev certificates (run once)"
	@echo "  make dev          - Start both API and client servers"
	@echo "  make api          - Start only the Rust API server"
	@echo "  make client       - Start only the Vite client"
	@echo "  make build        - Build both API and client"
	@echo "  make stop         - Stop all running servers"
	@echo "  make kill         - Force kill server (SIGKILL)"
	@echo "  make chrome       - Open Chrome with cert bypass"
	@echo "  make test         - Run API tests"
	@echo "  make clean        - Clean local build artifacts"
	@echo "  make docker-clean - Prune Docker build cache"
	@echo "  make clean-all    - Clean everything (local + Docker)"

# Generate dev certificates (one-time setup)
setup:
	@echo "Generating dev certificates..."
	@cd api/scripts && cargo run --release
	@echo ""
	@echo "Update client/.env and this Makefile with the cert hash above."

# Start both servers (API in background, client in foreground)
dev: stop
	@if [ ! -f api/certs/cert.pem ]; then \
		echo "Error: Certificates not found. Run 'make setup' first."; \
		exit 1; \
	fi
	@echo "Starting API server..."
	@cd api && cargo run --release > /dev/null 2>&1 &
	@sleep 2
	@echo "API server running on https://localhost:$(API_PORT)"
	@echo "Starting client..."
	@cd client && npm run dev

# Start only the API server
api: stop-api
	@if [ ! -f api/certs/cert.pem ]; then \
		echo "Error: Certificates not found. Run 'make setup' first."; \
		exit 1; \
	fi
	@echo "Starting API server..."
	cd api && cargo run --release

# Start only the client
client: stop-client
	@echo "Starting client..."
	cd client && npm run dev

# Build everything
build:
	@echo "Building API..."
	cd api && cargo build --release
	@echo "Building client..."
	cd client && npm run build

# Stop all servers
stop: stop-api stop-client
	@echo "All servers stopped"

stop-api:
	@pkill -f "orbit-royale-server" 2>/dev/null || true
	@pkill -f "target/.*api" 2>/dev/null || true

stop-client:
	@pkill -f "vite" 2>/dev/null || true

# Force kill server (SIGKILL)
kill:
	@pkill -9 -f "orbit-royale-server" 2>/dev/null || true
	@pkill -9 -f "target/.*api" 2>/dev/null || true
	@echo "Server killed"

# Open Chrome with certificate bypass (needed for WebTransport to API)
# Uses SPKI hash (different from cert hash used by WebTransport)
chrome:
	@if [ ! -f api/certs/cert.pem ]; then \
		echo "Error: Certificates not found. Run 'make setup' first."; \
		exit 1; \
	fi
	@SPKI_HASH=$$(openssl x509 -in api/certs/cert.pem -pubkey -noout 2>/dev/null | openssl pkey -pubin -outform der 2>/dev/null | openssl dgst -sha256 -binary | base64); \
	echo "Opening Chrome with SPKI hash: $$SPKI_HASH"; \
	/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
		--ignore-certificate-errors-spki-list=$$SPKI_HASH \
		http://localhost:$(CLIENT_PORT) 2>/dev/null &

# Run API tests
test:
	cd api && cargo test

# Clean build artifacts
clean:
	cd api && cargo clean
	cd client && rm -rf node_modules dist

# Clean certificates (will need to run setup again)
clean-certs:
	rm -rf api/certs

# Clean Docker build cache and unused images
docker-clean:
	@echo "Current Docker disk usage:"
	@docker system df
	@echo ""
	@echo "Pruning Docker build cache..."
	docker builder prune -f
	@echo ""
	@echo "After cleanup:"
	@docker system df

# Deep clean - everything (local + Docker)
clean-all: clean docker-clean
	@echo "All build artifacts cleaned"

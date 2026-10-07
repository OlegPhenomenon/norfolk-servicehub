.PHONY: dev dev-server dev-web build test lint seed smoke docker

# Local demo settings for `make dev` / `make seed`.
DEV_ENV = DEMO_MODE=true COOKIE_SECURE=false DATA_DIR=./data

# Backend on :8080 and the Vite dev server (proxying /api to it) side by side.
dev:
	$(MAKE) -j2 dev-server dev-web

dev-server:
	cd server && $(DEV_ENV) cargo run -- serve

dev-web:
	cd web && npm run dev

build:
	cd web && npm ci && npm run build
	cd server && cargo build --release

test:
	cd server && cargo test
	cd web && npm test

lint:
	cd server && cargo clippy --all-targets -- -D warnings
	cd web && npm run lint

# Wipe and re-seed the local demo database.
seed:
	cd server && $(DEV_ENV) cargo run -- seed-demo

# Platform smoke test against a temporary data directory.
smoke:
	scripts/smoke-platform.sh

docker:
	docker compose up --build

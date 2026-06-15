.PHONY: fmt fmt-check check clippy test check-duplicate-fns db-up db-down test-db gate-backend web-typecheck web-lint web-test web-build gate-web gate

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

check:
	cargo check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test

check-duplicate-fns:
	python3 -m unittest discover -s scripts/tests -p 'test_*.py'
	python3 scripts/check_duplicate_rust_fns.py

db-up:
	docker compose -f infra/compose.yaml up -d postgres

db-down:
	docker compose -f infra/compose.yaml down

test-db:
	MIRROR_TEST_DATABASE_URL=$${MIRROR_TEST_DATABASE_URL:-postgres://mirror:mirror@127.0.0.1:54329/mirror_test} cargo test -p mirror-backend --tests -- --ignored --test-threads=1

gate-backend: fmt-check check clippy test check-duplicate-fns

web-typecheck:
	npm --prefix src/web run typecheck

web-lint:
	npm --prefix src/web run lint

web-test:
	npm --prefix src/web test

web-build:
	npm --prefix src/web run build

gate-web: web-typecheck web-lint web-test web-build

gate: gate-backend gate-web

.PHONY: fmt fmt-check check clippy test check-duplicate-fns db-up db-down test-db gate-backend web-typecheck web-lint web-test web-e2e web-build gate-web android-format android-static android-test android-lint android-build android-device-test gate-android gate

ANDROID_JAVA_HOME ?= /usr/lib/jvm/java-17-openjdk
ANDROID_SDK_ROOT ?= $(HOME)/Android/Sdk
ANDROID_GRADLE_USER_HOME ?= $(CURDIR)/.gradle
ANDROID_GRADLE = JAVA_HOME=$(ANDROID_JAVA_HOME) ANDROID_SDK_ROOT=$(ANDROID_SDK_ROOT) GRADLE_USER_HOME=$(ANDROID_GRADLE_USER_HOME) ./src/android/gradlew -p src/android

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

web-e2e:
	npm --prefix src/web run test:e2e

web-build:
	npm --prefix src/web run build

gate-web: web-typecheck web-lint web-test web-e2e web-build

android-format:
	$(ANDROID_GRADLE) ktlintFormat

android-static:
	$(ANDROID_GRADLE) detekt ktlintCheck

android-test:
	$(ANDROID_GRADLE) test

android-lint:
	$(ANDROID_GRADLE) lint

android-build:
	$(ANDROID_GRADLE) assembleDebug assembleDebugAndroidTest

android-device-test:
	$(ANDROID_GRADLE) connectedDebugAndroidTest

gate-android: android-static android-test android-lint android-build

gate: gate-backend gate-web gate-android

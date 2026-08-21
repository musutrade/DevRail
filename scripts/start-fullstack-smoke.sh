#!/usr/bin/env bash
set -euo pipefail

: "${TEST_DATABASE_URL:?cargo flow 必须提供隔离的 TEST_DATABASE_URL}"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
BACKEND_MANIFEST="${REPOSITORY_ROOT}/backend/Cargo.toml"

export DATABASE_URL="${TEST_DATABASE_URL}"
export APP_ENV=test
export AUTO_MIGRATE=false
export PORT=18081
export CORS_ALLOWED_ORIGINS=http://localhost:4300
export LOG_FORMAT=json
export RUST_LOG=warn,sqlx=warn,tower_http=warn
export SERVICE_NAME=arc-admin-fullstack-smoke
export WEBAUTHN_RP_ID=localhost
export WEBAUTHN_RP_ORIGIN=http://localhost:4300
export BOOTSTRAP_ADMIN_USERNAME=fullstack_admin
export BOOTSTRAP_ADMIN_PASSWORD=Fullstack-Smoke-Password-2026!
export BOOTSTRAP_ADMIN_DISPLAY_NAME=全栈测试管理员
export BOOTSTRAP_ADMIN_EMAIL=fullstack-admin@example.test

cargo run --quiet --manifest-path "${BACKEND_MANIFEST}" --bin migrate
cargo run --quiet --manifest-path "${BACKEND_MANIFEST}" --bin bootstrap_admin
exec cargo run --quiet --manifest-path "${BACKEND_MANIFEST}" --bin arc-admin-backend

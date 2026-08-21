#!/usr/bin/env bash
# Trap handlers are invoked indirectly by Bash.
# shellcheck disable=SC2329

set -Eeuo pipefail
set -m

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT_DIR
readonly BACKEND_DIR="${ROOT_DIR}/backend"
readonly FRONTEND_DIR="${ROOT_DIR}/frontend"

backend_pid=''
frontend_pid=''

log() {
  printf '[arc-admin] %s\n' "$*"
}

fail() {
  printf '[arc-admin] Error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "Missing required command: $1"
}

terminate_group() {
  local pid="$1"

  if [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null; then
    kill -TERM -- "-${pid}" 2>/dev/null || kill -TERM "${pid}" 2>/dev/null || true
  fi
}

cleanup() {
  local exit_code=$?

  trap - EXIT INT TERM
  if [[ -n "${backend_pid}" || -n "${frontend_pid}" ]]; then
    log 'Stopping frontend and backend...'
  fi
  terminate_group "${frontend_pid}"
  terminate_group "${backend_pid}"
  [[ -n "${frontend_pid}" ]] && wait "${frontend_pid}" 2>/dev/null || true
  [[ -n "${backend_pid}" ]] && wait "${backend_pid}" 2>/dev/null || true
  exit "${exit_code}"
}

interrupt() {
  exit 130
}

terminate() {
  exit 143
}

trap interrupt INT
trap terminate TERM
trap cleanup EXIT

require_command cargo
require_command npm

[[ -f "${BACKEND_DIR}/.env" ]] ||
  fail 'backend/.env is missing; copy backend/.env.example and configure DATABASE_URL first'
[[ -d "${FRONTEND_DIR}/node_modules" ]] ||
  fail 'frontend dependencies are missing; run npm ci in frontend first'

log 'Starting backend (port is configured in backend/.env)...'
(
  cd "${BACKEND_DIR}"
  exec cargo run
) &
backend_pid=$!

log 'Starting frontend at http://localhost:4200 ...'
(
  cd "${FRONTEND_DIR}"
  exec npm start
) &
frontend_pid=$!

log 'Both services are running. Press Ctrl+C to stop them.'

set +e
wait -n "${backend_pid}" "${frontend_pid}"
exit_code=$?
set -e

if kill -0 "${backend_pid}" 2>/dev/null; then
  log "Frontend exited with status ${exit_code}."
else
  log "Backend exited with status ${exit_code}."
fi

exit "${exit_code}"

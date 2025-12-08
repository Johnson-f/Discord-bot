#!/usr/bin/env bash
set -euo pipefail

# Usage: ./deploy.sh [environment]
# Example: ./deploy.sh production

ENV="${1:-production}"

VPS_IP="95.216.219.131"
VPS_USER="root"
SSH_KEY="$HOME/.ssh/id_ed25519_vps"

DOCKER_IMAGE="johnsonf/discord-bot:latest"
COMPOSE_DIR="/opt/discord-bot"
SERVICE_NAME="discord-bot"
LOCAL_ENV_FILE=".env.${ENV}"
REMOTE_ENV_FILE="${COMPOSE_DIR}/.env.production"

if [[ ! -f "${LOCAL_ENV_FILE}" ]]; then
  echo "[local] missing env file: ${LOCAL_ENV_FILE}"
  echo "Create it from env.production.example"
  exit 1
fi

echo "[local] ensuring remote compose directory exists"
ssh -i "$SSH_KEY" \
  -o BatchMode=yes \
  -o StrictHostKeyChecking=accept-new \
  "${VPS_USER}@${VPS_IP}" "mkdir -p \"${COMPOSE_DIR}\""

echo "[local] uploading env file to ${REMOTE_ENV_FILE}"
scp -i "$SSH_KEY" \
  -o BatchMode=yes \
  -o StrictHostKeyChecking=accept-new \
  "${LOCAL_ENV_FILE}" \
  "${VPS_USER}@${VPS_IP}:${REMOTE_ENV_FILE}"

ssh -i "$SSH_KEY" \
  -o BatchMode=yes \
  -o StrictHostKeyChecking=accept-new \
  "${VPS_USER}@${VPS_IP}" "
    set -euo pipefail
    export ENV=\"${ENV}\"
    export DOCKER_IMAGE=\"${DOCKER_IMAGE}\"
    export SERVICE_NAME=\"${SERVICE_NAME}\"
    cd \"${COMPOSE_DIR}\"
    echo \"[remote] pulling image: \${DOCKER_IMAGE}\"
    docker pull \"\${DOCKER_IMAGE}\"
    echo \"[remote] pulling compose services: \${SERVICE_NAME}\"
    docker compose pull \"\${SERVICE_NAME}\"
    echo \"[remote] deploying compose service: \${SERVICE_NAME}\"
    docker compose up -d \"\${SERVICE_NAME}\"
    echo \"[remote] prune unused images\"
    docker image prune -f >/dev/null
  "

#!/usr/bin/env bash
set -Eeuo pipefail

REPO_URL="${BITTOPOS_REPO_URL:-https://github.com/Bittopos/bittopos-server.git}"
REF="${BITTOPOS_REF:-main}"
SERVICE_USER="bittopos"
INSTALL_DIR="/opt/bittopos"
BIN_DIR="${INSTALL_DIR}/bin"
CONFIG_DIR="/etc/bittopos"
ENV_FILE="${CONFIG_DIR}/bittopos.env"
DATA_DIR="/var/lib/bittopos"
SERVICE_FILE="/etc/systemd/system/bittopos-server.service"
TMP_DIR=""

log() { printf '\n[bittopos] %s\n' "$*"; }
die() { printf '[bittopos] ERROR: %s\n' "$*" >&2; exit 1; }
cleanup() { [[ -z "${TMP_DIR}" ]] || rm -rf -- "${TMP_DIR}"; }
trap cleanup EXIT

[[ "$(id -u)" -eq 0 ]] || die "Ejecuta como root: sudo bash scripts/install.sh"
command -v apt-get >/dev/null 2>&1 || command -v dnf >/dev/null 2>&1 || command -v apk >/dev/null 2>&1 || command -v pacman >/dev/null 2>&1 || die "Distribución no compatible: instala git, Rust/Cargo y SQLite manualmente."
command -v systemctl >/dev/null 2>&1 || die "Este instalador requiere systemd."

install_deps() {
  if command -v apt-get >/dev/null 2>&1; then
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates curl git build-essential pkg-config libsqlite3-dev
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y ca-certificates curl git gcc gcc-c++ make pkgconf-pkg-config sqlite-devel
  elif command -v apk >/dev/null 2>&1; then
    apk add --no-cache ca-certificates curl git build-base pkgconf sqlite-dev
  elif command -v pacman >/dev/null 2>&1; then
    pacman -Sy --needed --noconfirm ca-certificates curl git base-devel pkgconf sqlite
  fi
}

install_deps
export PATH="/root/.cargo/bin:${PATH}"
command -v cargo >/dev/null 2>&1 || die "Cargo no está disponible. Instala Rust estable con rustup antes de ejecutar el instalador."
command -v git >/dev/null 2>&1 || die "Git no está disponible."

if ! id "${SERVICE_USER}" >/dev/null 2>&1; then
  useradd --system --home-dir "${DATA_DIR}" --shell /usr/sbin/nologin "${SERVICE_USER}" 2>/dev/null || adduser -S -H -s /sbin/nologin "${SERVICE_USER}"
fi
install -d -o root -g root -m 0755 "${INSTALL_DIR}" "${BIN_DIR}"
install -d -o root -g "${SERVICE_USER}" -m 0750 "${CONFIG_DIR}"
install -d -o "${SERVICE_USER}" -g "${SERVICE_USER}" -m 0750 "${DATA_DIR}"

if [[ ! -f "${ENV_FILE}" ]]; then
  if [[ -t 0 ]]; then
    read -r -p "Usuario administrador [admin]: " username
    username="${username:-admin}"
    read -r -p "Origen web autorizado por CORS (ej. https://bittopos.com): " web_origin
    [[ -n "${web_origin}" ]] || die "WEB_ORIGIN no puede estar vacío."
    read -r -p "Dirección local de escucha [127.0.0.1:3000]: " bind_addr
    bind_addr="${bind_addr:-127.0.0.1:3000}"
  else
    username="${BITTOPOS_USERNAME:-admin}"
    web_origin="${WEB_ORIGIN:-http://localhost:8787}"
    bind_addr="${BIND_ADDR:-127.0.0.1:3000}"
  fi
  password="${BITTOPOS_PASSWORD:-}"
  if [[ -z "${password}" ]]; then
    if command -v openssl >/dev/null 2>&1; then password="$(openssl rand -hex 32)"
    else password="$(head -c 48 /dev/urandom | od -An -tx1 | tr -d ' \n')"; fi
  fi
  [[ "${#password}" -ge 12 ]] || die "BITTOPOS_PASSWORD debe tener al menos 12 caracteres."
  umask 077
  cat > "${ENV_FILE}" <<EOF
BITTOPOS_USERNAME=${username}
BITTOPOS_PASSWORD=${password}
BITTOPOS_DB_PATH=${DATA_DIR}/bittopos.sqlite3
BIND_ADDR=${bind_addr}
WEB_ORIGIN=${web_origin}
SESSION_COOKIE_SECURE=true
EOF
  chmod 0640 "${ENV_FILE}"
  chown root:"${SERVICE_USER}" "${ENV_FILE}"
  if [[ -t 0 && -z "${BITTOPOS_PASSWORD:-}" ]]; then
    printf '\nContraseña inicial generada y guardada en %s\n' "${ENV_FILE}"
    printf 'Guárdala en un gestor de contraseñas; no se volverá a mostrar.\n'
  fi
else
  log "Se conserva la configuración existente: ${ENV_FILE}"
fi

TMP_DIR="$(mktemp -d)"
log "Descargando ${REPO_URL} (ref: ${REF}) y compilando release."
git clone --depth 1 --branch "${REF}" "${REPO_URL}" "${TMP_DIR}/source"
(
  cd "${TMP_DIR}/source"
  cargo build --release --bin bittopos-server
)
install -o root -g root -m 0755 "${TMP_DIR}/source/target/release/bittopos-server" "${BIN_DIR}/bittopos-server"

install -d -o root -g root -m 0755 /etc/systemd/system
cat > "${SERVICE_FILE}" <<'EOF'
[Unit]
Description=Bittopos HTTP Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=bittopos
Group=bittopos
EnvironmentFile=/etc/bittopos/bittopos.env
WorkingDirectory=/var/lib/bittopos
ExecStart=/opt/bittopos/bin/bittopos-server
Restart=on-failure
RestartSec=3
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/bittopos
RestrictSUIDSGID=true
LockPersonality=true

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable --now bittopos-server
log "Instalación terminada."
systemctl --no-pager --full status bittopos-server || true
printf '\nPrueba local: curl -fsS http://127.0.0.1:3000/health\n'
printf 'Configuración: %s (permisos restringidos)\n' "${ENV_FILE}"
printf 'Actualización posterior: sudo bash scripts/update.sh\n'

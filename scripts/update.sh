#!/usr/bin/env bash
set -Eeuo pipefail

REPO_URL="${BITTOPOS_REPO_URL:-https://github.com/Bittopos/bittopos-server.git}"
REF="${BITTOPOS_REF:-main}"
BIN_DIR="/opt/bittopos/bin"
TMP_DIR=""

die() { printf '[bittopos] ERROR: %s\n' "$*" >&2; exit 1; }
cleanup() { [[ -z "${TMP_DIR}" ]] || rm -rf -- "${TMP_DIR}"; }
trap cleanup EXIT

[[ "$(id -u)" -eq 0 ]] || die "Ejecuta como root: sudo bash scripts/update.sh"
[[ -f /etc/bittopos/bittopos.env ]] || die "No existe /etc/bittopos/bittopos.env. Ejecuta primero el instalador."
[[ -x "${BIN_DIR}/bittopos-server" ]] || die "No se encuentra el binario instalado."
command -v systemctl >/dev/null 2>&1 || die "Este actualizador requiere systemd."
command -v git >/dev/null 2>&1 || die "Git no está instalado."
command -v cargo >/dev/null 2>&1 || die "Cargo no está instalado. Instala Rust estable antes de actualizar."

TMP_DIR="$(mktemp -d)"
printf '[bittopos] Descargando %s (ref: %s)\n' "${REPO_URL}" "${REF}"
git clone --depth 1 --branch "${REF}" "${REPO_URL}" "${TMP_DIR}/source"
(
  cd "${TMP_DIR}/source"
  cargo build --release --bin bittopos-server
)
install -o root -g root -m 0755 "${TMP_DIR}/source/target/release/bittopos-server" "${BIN_DIR}/bittopos-server.new"
mv -f "${BIN_DIR}/bittopos-server.new" "${BIN_DIR}/bittopos-server"
systemctl restart bittopos-server
systemctl is-active --quiet bittopos-server || {
  journalctl -u bittopos-server -n 80 --no-pager >&2 || true
  die "El servicio no quedó activo. La configuración y la base de datos se han conservado."
}
printf '[bittopos] Actualización completada. La configuración y la base SQLite no se modificaron.\n'
printf 'Salud: curl -fsS http://127.0.0.1:3000/health\n'

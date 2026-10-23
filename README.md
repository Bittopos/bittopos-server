# Servidor HTTP Rust

**Licencia:** GNU Affero General Public License v3.0 (AGPL-3.0-only). Consulta el texto completo en [LICENSE](LICENSE).

Este servicio usa `bittopos-core` como autoridad para el esquema y su módulo SQLite para persistir perfiles, proyectos, recursos y estructuras. No duplica el modelo de fichas en otra implementación.

## Requisitos

- Linux de 64 bits compatible con Rust estable.
- Rust estable/Cargo instalado mediante [rustup](https://rustup.rs/), Git y las herramientas de compilación de C.
- SQLite de desarrollo (por ejemplo, `libsqlite3-dev` en Debian/Ubuntu).
- Para el servicio de producción, systemd. El instalador no requiere AWS, un dominio ni Cloudflare.

## Desarrollo local

Desde la raíz del repositorio:

```bash
cp .env.example .env
# Edita .env y configura BITTOPOS_PASSWORD con un secreto propio de al menos 12 caracteres.
set -a
. ./.env
set +a
cargo run --bin bittopos-server
```

- `BITTOPOS_USERNAME`: usuario de desarrollo (por defecto `admin`).
- `BITTOPOS_PASSWORD`: contraseña obligatoria; el servidor no inicia si falta o tiene menos de 12 caracteres.
- `BITTOPOS_DB_PATH`: ruta SQLite; por defecto `./data/bittopos.sqlite3`.
- `BIND_ADDR`: dirección de escucha; por defecto `0.0.0.0:3000`.
- `WEB_ORIGIN`: origen exacto de la SPA autorizado por CORS.
- `SESSION_COOKIE_SECURE`: `true` para HTTPS; `false` solo para desarrollo HTTP local.

## Instalación en Linux con systemd

El instalador instala dependencias del sistema, verifica que Rust/Cargo ya esté instalado, compila el binario release y configura el servicio `bittopos-server` con un usuario de sistema sin privilegios. Está pensado para Ubuntu/Debian y otras distribuciones que dispongan de uno de los gestores de paquetes compatibles en el script.

Instala Rust estable primero si aún no está disponible (consulta [rustup.rs](https://rustup.rs/)). Después, revisa el script antes de ejecutarlo. Desde una copia local del repositorio:

```bash
sudo bash scripts/install.sh
```

Durante la primera instalación solicita el usuario administrador, el origen web permitido por CORS y la dirección de escucha. Por defecto escucha en `127.0.0.1:3000`, adecuado si un proxy inverso o Cloudflare Tunnel se ejecuta en el mismo servidor. Para acceso directo en una red controlada, establece explícitamente la dirección apropiada y configura HTTPS/firewall por separado.

El instalador crea la configuración restringida en `/etc/bittopos/bittopos.env` y los datos persistentes en `/var/lib/bittopos/bittopos.sqlite3`. Si no se proporciona `BITTOPOS_PASSWORD`, genera una contraseña aleatoria y la guarda en el archivo de entorno; limita el acceso a ese archivo y guárdala en un gestor de contraseñas. No publiques ese archivo ni lo añadas al repositorio.

Variables opcionales para instalaciones automatizadas:
- `BITTOPOS_REPO_URL`: URL Git del repositorio (por defecto, el repositorio oficial).
- `BITTOPOS_REF`: rama o tag a instalar (por defecto, `main`).
- `BITTOPOS_USERNAME`, `BITTOPOS_PASSWORD`, `WEB_ORIGIN`, `BIND_ADDR`: valores iniciales cuando no existe todavía el archivo de configuración.

Comprobación y diagnóstico:

```bash
sudo systemctl status bittopos-server
sudo journalctl -u bittopos-server -n 100 --no-pager
curl -fsS http://127.0.0.1:3000/health
```

## Actualización

Desde una copia local actualizada del repositorio:

```bash
sudo bash scripts/update.sh
```

El actualizador compila una nueva versión y reinicia el servicio. No reemplaza el archivo de configuración ni la base SQLite. Antes de actualizaciones de producción, se recomienda realizar una copia de seguridad de `/var/lib/bittopos`. El actualizador requiere Git, Rust/Cargo y systemd instalados.

## API v1

- `GET /health`: estado del proceso, sin autenticación.
- `POST /api/v1/login`: recibe JSON con username y password y crea una cookie HttpOnly.
- `POST /api/v1/logout` y `GET /api/v1/me`: cerrar y consultar sesión.
- `GET|POST /api/v1/profiles`: listar y crear perfiles.
- `GET|POST /api/v1/profiles/{profile_id}/projects`: listar y crear proyectos dentro de un perfil.
- `GET|POST /api/v1/projects/{project_id}/resources`: listar y guardar recursos del proyecto.
- `GET|PUT /api/v1/projects/{project_id}/workspace`: leer y guardar la estructura. PUT requiere `workspace` y `expected_revision` para detectar escrituras concurrentes.

Salvo `/health` y `/api/v1/login`, todas las rutas requieren una sesión válida. Las sesiones duran 24 horas y se invalidan al reiniciar el proceso. Este inicio de sesión es una barrera temporal de desarrollo de un solo administrador; aún no hay cuentas, permisos por proyecto, limitación de intentos ni almacenamiento persistente de sesiones.

## Docker

```bash
docker build -t bittopos-server .
docker run --rm -p 127.0.0.1:3000:3000 \
  -e BITTOPOS_USERNAME=admin \
  -e BITTOPOS_PASSWORD='usa-un-secreto-largo-y-aleatorio' \
  -e WEB_ORIGIN=https://tu-frontend.example \
  -v bittopos-data:/var/lib/bittopos \
  bittopos-server
```

El contenedor corre como usuario no privilegiado. Mantén el volumen `/var/lib/bittopos` para conservar la base entre reinicios. El servidor requiere SQLite nativo en tiempo de ejecución.

## Límites y seguridad

La persistencia es SQLite local. Cloudflare R2/D1 todavía no está conectado: no se debe tratar R2 como un disco POSIX. La SPA debe adoptar las rutas de perfil/proyecto y enviar `credentials: include` para usar la cookie. No expongas el servicio a Internet sin HTTPS, una contraseña fuerte y una revisión de autenticación y protección CSRF. Cloudflare Tunnel y otros proxies son opcionales y deben configurarse por separado.

# Servidor HTTP Rust

**Licencia:** GNU Affero General Public License v3.0 (AGPL-3.0-only). Consulta el texto completo en [`server/LICENSE`](LICENSE).

Este servicio usa bittopos-core como autoridad para el esquema y utiliza su módulo SQLite para persistir perfiles, proyectos, recursos y estructuras. No duplica el modelo de fichas en otra implementación.

## Desarrollo local

Requiere Rust estable y SQLite (libsqlite3-dev en Debian/Ubuntu).

```bash
cp server/.env.example .env
# Edita .env y configura BITTOPOS_PASSWORD con un secreto propio de al menos 12 caracteres.
set -a
. ./.env
set +a
cargo run --bin bittopos-server
```

- BITTOPOS_USERNAME: usuario de desarrollo (por defecto admin).
- BITTOPOS_PASSWORD: contraseña obligatoria. El servidor no inicia si falta o tiene menos de 12 caracteres.
- BITTOPOS_DB_PATH: ruta SQLite; por defecto ./data/bittopos.sqlite3.
- BIND_ADDR: dirección de escucha; por defecto 0.0.0.0:3000.
- WEB_ORIGIN: origen exacto de la SPA autorizado por CORS.
- SESSION_COOKIE_SECURE: true para HTTPS; false solo para desarrollo HTTP local.

## API v1

- GET /health: estado del proceso, sin autenticación.
- POST /api/v1/login: recibe JSON con username y password y crea una cookie HttpOnly.
- POST /api/v1/logout y GET /api/v1/me: cerrar y consultar sesión.
- GET|POST /api/v1/profiles: listar y crear perfiles.
- GET|POST /api/v1/profiles/{profile_id}/projects: listar y crear proyectos dentro de un perfil.
- GET|POST /api/v1/projects/{project_id}/resources: listar y guardar recursos del proyecto.
- GET|PUT /api/v1/projects/{project_id}/workspace: leer y guardar la estructura. PUT requiere workspace y expected_revision para detectar escrituras concurrentes.

Salvo /health y /api/v1/login, todas las rutas requieren una sesión válida. Las sesiones duran 24 horas y se invalidan al reiniciar el proceso. Este inicio de sesión es una barrera temporal de desarrollo de un solo administrador; aún no hay cuentas, permisos por proyecto, limitación de intentos ni almacenamiento persistente de sesiones.

## Docker

```bash
docker build -t bittopos-server .
docker run --rm -p 3000:3000 \
  -e BITTOPOS_USERNAME=admin \
  -e BITTOPOS_PASSWORD='usa-un-secreto-largo-y-aleatorio' \
  -e WEB_ORIGIN=https://tu-frontend.example \
  -v bittopos-data:/var/lib/bittopos \
  bittopos-server
```

El contenedor corre como usuario no privilegiado. Mantén el volumen /var/lib/bittopos para conservar la base entre reinicios. El servidor requiere SQLite nativo en tiempo de ejecución.

## Límites actuales

La persistencia es SQLite local. Cloudflare R2/D1 todavía no está conectado: no se debe tratar R2 como un disco POSIX. La SPA debe adoptar las rutas de perfil/proyecto y enviar credentials: include para usar la cookie. No se debe exponer a Internet sin HTTPS, contraseña fuerte y revisión de la autenticación y la protección CSRF.

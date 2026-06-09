# GGGGGG

Pixel self-service discount-link extraction system remake starter.

This repository contains:

- `docs/pixel-site-spec.md`: public-page feature inventory and API contract notes.
- `docs/openapi.yaml`: OpenAPI draft for the observed API surface.
- `frontend/`: React 19 + Vite frontend.
- `backend/`: Rust Axum backend that proxies upstream Pixel APIs and persists distribution CDK mappings in MongoDB.
- `.github/workflows/build-artifacts.yml`: GitHub Actions workflow that builds and uploads frontend and backend artifacts.

## Local Development

Requirements:

- Node.js 24+
- npm 10+
- Rust 1.89+
- MongoDB 6+

Install frontend dependencies:

```powershell
npm install --prefix frontend
```

Run the Rust API:

```powershell
cargo run --manifest-path backend/Cargo.toml
```

Backend environment variables:

```powershell
$env:MONGODB_URI='mongodb://localhost:27017'
$env:MONGODB_DATABASE='pixel_remake'
$env:UPSTREAM_BASE_URL='https://pixel.yh-mo.xyz'
```

The backend proxies user `/api/...` requests to:

```text
https://pixel.yh-mo.xyz
```

Admin CDK endpoints are local backend endpoints:

- `GET /api/admin/cdks`
- `POST /api/admin/cdks`
- `DELETE /api/admin/cdks/{id}`

When a request body contains `card_key`, the backend checks MongoDB for a matching distribution CDK and forwards the corresponding upstream CDK to `https://pixel.yh-mo.xyz`.

Run the frontend:

```powershell
npm run dev --prefix frontend
```

The Vite dev server proxies `/api` to `http://127.0.0.1:8080`.
For production, the Rust backend can serve `frontend/dist` directly. Set `FRONTEND_DIST_DIR` when the frontend build output is not at `frontend/dist`.

Frontend routes:

- `/`: user task workspace.
- `/admin`: distribution CDK management.

## Build

```powershell
npm run build --prefix frontend
cargo build --manifest-path backend/Cargo.toml --release
```

The GitHub Actions workflow uploads:

- `frontend/dist`
- `backend/target/release/pixel-api`

## Docker Images

The `Docker GHCR` workflow builds one fused Docker image for GitHub Packages / GHCR:

- `ghcr.io/chloemlla/gggggg`

Workflow behavior:

- Pull requests build the image without pushing.
- Pushes to `main` and manual runs push `latest` and `sha-*` tags.

Container variables:

```powershell
$env:BIND_ADDR='0.0.0.0:8080'
$env:FRONTEND_DIST_DIR='/app/public'
$env:MONGODB_URI='mongodb://mongo:27017'
$env:MONGODB_DATABASE='pixel_remake'
$env:UPSTREAM_BASE_URL='https://pixel.yh-mo.xyz'
$env:APP_BASE_URL='https://your-production-domain.example'
$env:CORS_ALLOWED_ORIGINS=''
$env:ADMIN_TOKEN_ENCRYPTION_KEY='replace-with-a-long-random-secret'
$env:CDK_USAGE_LOG_TTL_SECONDS='2592000'
```

The container starts only the Rust backend. It serves the frontend static files and handles `/api/...` from the same origin, so the frontend API base is detected automatically from `window.location.origin`.

`CORS_ALLOWED_ORIGINS` is intentionally blank for same-origin production deployments. Set it to a comma-separated list only when a separate trusted frontend origin must call the API with credentials.

No license file has been added yet because the repository owner has not selected a license.

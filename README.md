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

The `Docker GHCR` workflow builds Docker images for GitHub Packages / GHCR:

- `ghcr.io/chloemlla/gggggg-backend`
- `ghcr.io/chloemlla/gggggg-frontend`

Workflow behavior:

- Pull requests build images without pushing.
- Pushes to `main` and manual runs push `latest` and `sha-*` tags.

Backend container variables:

```powershell
$env:BIND_ADDR='0.0.0.0:8080'
$env:MONGODB_URI='mongodb://mongo:27017'
$env:MONGODB_DATABASE='pixel_remake'
```

The frontend image serves static files through Nginx and proxies `/api/` to `http://backend:8080`.

No license file has been added yet because the repository owner has not selected a license.

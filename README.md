# GGGGGG

Pixel self-service discount-link extraction system remake starter.

This repository contains:

- `docs/pixel-site-spec.md`: public-page feature inventory and API contract notes.
- `docs/openapi.yaml`: OpenAPI draft for the observed API surface.
- `frontend/`: React 19 + Vite frontend.
- `backend/`: Rust Axum backend mock that implements the documented API shape.
- `.github/workflows/build-artifacts.yml`: GitHub Actions workflow that builds and uploads frontend and backend artifacts.

## Local Development

Requirements:

- Node.js 24+
- npm 10+
- Rust 1.89+

Install frontend dependencies:

```powershell
npm install --prefix frontend
```

Run the Rust API:

```powershell
cargo run --manifest-path backend/Cargo.toml
```

Run the frontend:

```powershell
npm run dev --prefix frontend
```

The Vite dev server proxies `/api` to `http://127.0.0.1:8080`.

Demo card key:

```text
demo-card-key
```

## Build

```powershell
npm run build --prefix frontend
cargo build --manifest-path backend/Cargo.toml --release
```

The GitHub Actions workflow uploads:

- `frontend/dist`
- `backend/target/release/pixel-api`

No license file has been added yet because the repository owner has not selected a license.

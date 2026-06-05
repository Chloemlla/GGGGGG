FROM node:24-alpine AS frontend-builder

WORKDIR /app/frontend

COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci

COPY frontend ./
ARG VITE_API_BASE=
ENV VITE_API_BASE=${VITE_API_BASE}
RUN npm run build

FROM rust:1.89-bookworm AS backend-builder

WORKDIR /app/backend

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=backend-builder /app/backend/target/release/pixel-api /app/pixel-api
COPY --from=frontend-builder /app/frontend/dist /app/public

ENV BIND_ADDR=0.0.0.0:8080
ENV FRONTEND_DIST_DIR=/app/public
ENV MONGODB_DATABASE=pixel_remake

EXPOSE 8080

ENTRYPOINT ["/app/pixel-api"]

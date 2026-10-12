# syntax=docker/dockerfile:1
# Multi-stage build for the `social` server (docs/aws/DEPLOYMENT.md §7).

FROM rust:1.90-slim-bookworm AS build
# .cargo/config.toml builds for the host CPU (`target-cpu=native`). An image can run on
# other machines, so pin a target instead: e.g. x86-64-v3, or neoverse-n1 for Graviton.
ARG TARGET_CPU=generic
ENV RUSTFLAGS="-C target-cpu=${TARGET_CPU}"
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p social \
 && cp target/release/social /social \
 && mkdir -p /media

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /social /usr/local/bin/social
# Uploaded images. Created here so a named volume mounted on it is writable by nonroot.
COPY --from=build --chown=nonroot:nonroot /media /var/lib/social/media
ENV SOCIAL_ADDR=0.0.0.0:3000 \
    SOCIAL_MEDIA_DIR=/var/lib/social/media
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/social"]

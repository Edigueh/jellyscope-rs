# Builder: compile the WASM viewer (-> web/pkg) and the native bake binary.
FROM rust:1-bookworm AS builder

# wasm-bindgen CLI must match the pinned wasm-bindgen crate (=0.2.100) exactly,
# or the generated JS bindings mismatch at runtime.
RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-bindgen-cli --version 0.2.100

WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY web ./web

RUN cargo build -p viewer --target wasm32-unknown-unknown --release \
    && wasm-bindgen target/wasm32-unknown-unknown/release/viewer.wasm \
        --out-dir web/pkg --target web --no-typescript \
    && cargo build -p bake --release

# Runtime: Debian/glibc nginx (the bake binary is glibc-linked; alpine/musl
# would not run it). Serves web/ + dist/ from /srv.
FROM nginx:stable AS runtime

COPY --from=builder /build/target/release/bake /usr/local/bin/bake
COPY --from=builder /build/web /srv/web
COPY docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 80
ENTRYPOINT ["/entrypoint.sh"]

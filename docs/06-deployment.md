# Deployment

Container is build + serve. Bake runs once per volume (idempotent via manifest check).

## Image layers

```mermaid
flowchart LR
    subgraph Builder["rust:1-bookworm (builder)"]
        R1["cargo install<br/>wasm-bindgen-cli =0.2.100"]
        R2["rustup target add<br/>wasm32-unknown-unknown"]
        R3["cargo build viewer<br/>-> wasm"]
        R4["wasm-bindgen<br/>-> web/pkg"]
        R5["cargo build bake<br/>-> native glibc"]
        R1 --> R2
        R2 --> R3
        R3 --> R4
        R2 --> R5
    end
    subgraph Runtime["nginx:stable (runtime)"]
        N1["/usr/local/bin/bake"]
        N2["/srv/web"]
        N3["/etc/nginx/conf.d/default.conf"]
        N4["/entrypoint.sh"]
    end
    R5 --> N1
    R4 --> N2
```

Runtime stays Debian glibc because `bake` is glibc-linked; alpine/musl won't run it.

## Runtime sequence

```mermaid
sequenceDiagram
    autonumber
    participant Host
    participant Entry as entrypoint.sh
    participant Bake as bake
    participant Nginx as nginx
    participant Client

    Host->>Entry: run container (bind /data, volume /srv/dist)
    Entry->>Entry: check /srv/dist/manifest.json
    alt FORCE_BAKE=1 or missing manifest
        Entry->>Bake: bake /data /srv/dist
        Bake-->>Entry: dist ready
    else dist present
        Entry-->>Entry: skip
    end
    Entry->>Nginx: exec (daemon off)
    Client->>Nginx: GET /web/* , /dist/*
```

## Volumes + env

| Mount / env | Role |
|---|---|
| `-v ./data:/data` | FITS + CSV datasets input |
| `-v jelly_dist:/srv/dist` | baked outputs (persistent) |
| `FORCE_BAKE=1` | re-bake even when manifest exists |

## Static hosting (no container)

```mermaid
flowchart LR
    A[just build] --> B[dist/ + web/pkg/]
    B --> C[any static server<br/>nginx / CDN / python http.server]
    C --> D[Browser]
```

## GitHub Pages data artifact

The Pages workflow downloads the release asset named exactly `dist.tar.gz`,
normalizes either a root-level archive or an archive containing a top-level
`dist/` directory, copies `web/` beside it, and publishes the combined
directory. A release upload label does not rename an asset: its actual filename
must be `dist.tar.gz`.

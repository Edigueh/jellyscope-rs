_default:
    @just --list --justfile {{source_file()}}

# Build the image (bakes at container start, not here).
build:
    docker compose build

# Start detached; open http://localhost:8000/
up:
    docker compose up -d

# Stop and remove the container.
down:
    docker compose down

# Tail container logs (bake output + nginx access log).
logs:
    docker compose logs -f jellyscope

# Rebuild image and recreate the container.
reload:
    docker compose up -d --build

# Force a re-bake into the persistent dist volume (after changing data/).
rebake:
    docker compose run --rm --entrypoint /usr/local/bin/bake jellyscope /data /srv/dist
    docker compose up -d

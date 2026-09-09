#!/bin/sh
# Bake mounted FITS data into static flux planes, then serve. The bake step is a
# one-shot batch (not a server); nginx serves the result. /srv/dist is a
# persistent volume, so re-bake only when it's empty or FORCE_BAKE=1.
set -e

if [ "$FORCE_BAKE" = 1 ] || [ ! -f /srv/dist/manifest.json ]; then
    if [ -z "$(ls -A /data 2>/dev/null)" ]; then
        echo "warning: /data is empty and no baked dist — mount datasets at /data" >&2
        echo "(bind ./data:/data). starting nginx anyway; the viewer will not load." >&2
    else
        echo "baking /data -> /srv/dist ..."
        bake /data /srv/dist
    fi
else
    echo "dist already baked (found /srv/dist/manifest.json); skipping bake."
    echo "set FORCE_BAKE=1 to re-bake after changing /data."
fi

exec nginx -g 'daemon off;'

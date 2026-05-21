set shell := ["zsh", "-cu"]

# Create a shareable source + reports archive without local toolchain/build caches.
zip out="":
    #!/usr/bin/env zsh
    set -euo pipefail

    root="${PWD:A}"
    repo="${root:t}"
    sha="$(git rev-parse --short HEAD)"
    stamp="$(date +%Y%m%d)"
    archive="{{out}}"
    if [[ -z "$archive" ]]; then
      archive="${root:h}/${repo}-code-and-reports-${sha}-${stamp}.zip"
    fi

    rm -f "$archive"
    (
      cd "$root:h"
      zip -rq "$archive" "$repo" \
        -x "$repo/.git/*" \
        -x "$repo/.cache/*" \
        -x "$repo/report-ui/node_modules/*" \
        -x "$repo/report-ui/dist/*" \
        -x "$repo/report-ui/.vite/*" \
        -x "$repo/target/debug/*" \
        -x "$repo/target/release/*" \
        -x "$repo/target/.rustc_info.json" \
        -x "$repo/target/CACHEDIR.TAG" \
        -x "$repo/foundry/cache/*" \
        -x "$repo/foundry/out/*" \
        -x "$repo/**/*.DS_Store" \
        -x "$repo/.DS_Store" \
        -x "$repo/*.zip"
    )

    ls -lh "$archive"

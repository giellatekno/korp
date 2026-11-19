#!/bin/bash
# TEMPORARY, until task 3 (corpus data in R2) gives the management tool an
# upload command. Packs one language's corpus config and corpora in the layout
# the Worker reads (see src/deploy_back.rs), and puts them into R2 with wrangler.
#
#   worker/stub-data.sh LANG VERSION --local    # wrangler dev's local R2
#   worker/stub-data.sh LANG VERSION --remote   # the account's bucket korp-corpora
#
# Run from tasks/. --remote needs CLOUDFLARE_ACCOUNT_ID, and the bucket
# (npx wrangler r2 bucket create korp-corpora). CWB data is read from
# ../cwb-data (registry/ + data/), the config from ../gtweb2_config (or
# $KORP_DIR/… instead of ../…).
# wrangler r2 object put stops at ~315 MB per object: fine for fkv, not for sme.
set -euo pipefail
lang=$1 version=$2 where=$3
korp=${KORP_DIR:-$(cd .. && pwd)}
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

put() {  # put KEY FILE
    (cd worker && npx wrangler r2 object put "korp-corpora/$1" --file "$2" "$where" >/dev/null)
    echo "put korp-corpora/$1 ($(stat -c %s "$2") bytes)"
}

printf '%s' "$version" > "$out/current"
tar -C "$korp/gtweb2_config/corpus_configs/$lang" -c . | zstd -q -19 -T0 -o "$out/config.tar.zst"
put "configs/$lang/$version.tar.zst" "$out/config.tar.zst"
put "configs/$lang/current" "$out/current"
for yaml in "$korp/gtweb2_config/corpus_configs/$lang/corpora/"*.yaml; do
    corpus=$(basename "$yaml" .yaml)
    # The data directory is the last part of the registry file's HOME
    dir=$(sed -n 's;^HOME .*/\([^/]*\)$;\1;p' "$korp/cwb-data/registry/$corpus")
    tar -C "$korp/cwb-data" -c "registry/$corpus" "data/$dir" | zstd -q -19 -T0 -o "$out/$corpus.tar.zst"
    put "corpora/$corpus/$version.tar.zst" "$out/$corpus.tar.zst"
    put "corpora/$corpus/current" "$out/current"
done

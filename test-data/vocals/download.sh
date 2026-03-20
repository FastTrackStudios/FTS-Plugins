#!/usr/bin/env bash
# Download CC-BY vocal stems from ccMixter for integration testing.
#
# All tracks are Creative Commons Attribution licensed.
# Attribution:
#   - "Persephone" by snowflake (CC-BY) — https://ccmixter.org/files/snowflake/22364
#   - "Ophelia's Song (Vocals)" by musetta (CC-BY) — https://ccmixter.org/files/musetta/15601
#   - "Harmony" by snowflake (CC-BY) — https://ccmixter.org/files/snowflake/26759
#
# These are short vocal acapellas used to test the vocal rider DSP.

set -euo pipefail
cd "$(dirname "$0")"

TRACKS=(
    "snowflake_-_Persephone.mp3|https://ccmixter.org/content/snowflake/snowflake_-_Persephone.mp3"
    "musetta_-_Ophelias_Song_Vocals.mp3|https://ccmixter.org/content/musetta/musetta_-_Ophelia_s_Song_(Vocals).mp3"
    "snowflake_-_Harmony.mp3|https://ccmixter.org/content/snowflake/snowflake_-_Harmony_2.mp3"
)

for entry in "${TRACKS[@]}"; do
    name="${entry%%|*}"
    url="${entry#*|}"

    if [ -f "$name" ]; then
        echo "Already downloaded: $name"
        continue
    fi

    echo "Downloading: $name"
    curl -sL --insecure --max-time 120 \
        -H 'User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36' \
        -H 'Referer: https://ccmixter.org/' \
        "$url" -o "$name"
    size=$(wc -c < "$name")
    if [ "$size" -lt 1000 ]; then
        echo "  ERROR: Download too small ($size bytes), removing"
        rm -f "$name"
        exit 1
    fi
    echo "  Done ($size bytes)"
done

echo ""
echo "All vocal test files downloaded."
echo "License: Creative Commons Attribution (CC-BY)"

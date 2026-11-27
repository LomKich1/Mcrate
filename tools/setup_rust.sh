#!/bin/sh
# Ставит Rust в песочницу Claude (Linux). На ПК не нужен.
# Запуск: sh tools/setup_rust.sh && . /home/claude/env.sh
set -e
ROOT=/home/claude
ENV=$ROOT/mamba/envs/rs
if [ ! -f "$ROOT/env.sh" ]; then
    mkdir -p "$ROOT/mm"
    curl -fsSL https://conda.anaconda.org/conda-forge/linux-64/micromamba-2.10.0-0.tar.bz2 |
        tar -xj -C "$ROOT/mm" bin/micromamba
    "$ROOT/mm/bin/micromamba" create -y -q -r "$ROOT/mamba" -n rs -c conda-forge \
        rust=1.100 gcc_linux-64 pkg-config openssl alsa-lib libudev
    for c in cc gcc; do ln -sf x86_64-conda-linux-gnu-gcc "$ENV/bin/$c"; done
    cat > "$ROOT/env.sh" <<EOF
export PATH="$ENV/bin:\$PATH"
export OPENSSL_DIR="$ENV"
export PKG_CONFIG_PATH="$ENV/lib/pkgconfig"
EOF
fi
echo "Rust готов"

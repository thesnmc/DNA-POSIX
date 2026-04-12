#!/bin/bash
set -e

echo "[*] Initializing V8 Enterprise Biological Lab..."

# 1. Build the Rust Engine
echo "[*] Compiling core FUSE bridge..."
cd dna_vfs_core
cargo build --release
cd ..

# 2. Setup the Infrastructure
echo "[*] Securing Volatile RAM Vaults..."
mkdir -p dna_vfs/.dna_cache/{physical_pool,vault_b,journal}
mkdir -p dna_vfs/{bio_drive,.bio_trash}

# 3. Mount tmpfs (ignores if already mounted)
if ! mountpoint -q dna_vfs/.dna_cache/physical_pool; then
    sudo mount -t tmpfs -o size=1G tmpfs dna_vfs/.dna_cache/physical_pool
fi

if ! mountpoint -q dna_vfs/.dna_cache/vault_b; then
    sudo mount -t tmpfs -o size=1G tmpfs dna_vfs/.dna_cache/vault_b
fi

# 4. Boot the Engine
echo "[+] Infrastructure Locked. Booting Matrix Command Center..."
./dna_vfs_core/target/release/dna_vfs_core ./dna_vfs/bio_drive

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

# 3. Mount tmpfs (Auto-scales to half of system RAM)
SYS_RAM=$(free -g | awk '/^Mem:/{print $2}')
VAULT_SIZE=$((SYS_RAM / 2))

if [ "$VAULT_SIZE" -lt 1 ]; then
    VAULT_SIZE=1 # Fallback to 1GB for tiny VMs
fi

echo "[*] Detected ${SYS_RAM}GB RAM. Allocating ${VAULT_SIZE}GB per Biological Vault..."

if ! mountpoint -q dna_vfs/.dna_cache/physical_pool; then
    sudo mount -t tmpfs -o size=${VAULT_SIZE}G tmpfs dna_vfs/.dna_cache/physical_pool
fi

if ! mountpoint -q dna_vfs/.dna_cache/vault_b; then
    sudo mount -t tmpfs -o size=${VAULT_SIZE}G tmpfs dna_vfs/.dna_cache/vault_b
fi

# 4. Boot the Engine
echo "[+] Infrastructure Locked. Booting Matrix Command Center..."
./dna_vfs_core/target/release/dna_vfs_core ./dna_vfs/bio_drive

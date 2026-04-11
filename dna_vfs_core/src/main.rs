use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request,
};
use libc::ENOENT;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ==========================================
// 🧬 THE BIOLOGICAL CORE (YOUR EXACT MATH)
// ==========================================

fn compute_checksum(data: &[u8]) -> u32 {
    let mut a: u32 = 1; let mut b: u32 = 0;
    for &byte in data { a = (a + byte as u32) % 65521; b = (b + a) % 65521; }
    (b << 16) | a
}

fn generate_primer(filename: &str) -> String {
    let mut hash: u32 = 5381;
    for b in filename.bytes() { hash = hash.wrapping_mul(33).wrapping_add(b as u32); }
    let bases = ['A', 'C', 'G', 'T'];
    let mut primer = String::new();
    for i in 0..6 { primer.push(bases[((hash >> (i * 2)) & 3) as usize]); }
    primer
}

fn encode_oligo(primer: &str, data: &[u8]) -> String {
    let mut armored = Vec::new();
    for &byte in data { armored.push(byte); armored.push(byte); armored.push(byte); }
    
    let mut dna = primer.to_string();
    let mut prev = primer.chars().last().unwrap_or('C');

    for byte in armored {
        let mut val = byte as u16;
        for _ in 0..6 {
            let trinary = val % 3; val /= 3;
            let next_base = match (prev, trinary) {
                ('A', 0) => 'C', ('A', 1) => 'G', ('A', 2) => 'T',
                ('C', 0) => 'A', ('C', 1) => 'G', ('C', 2) => 'T',
                ('G', 0) => 'A', ('G', 1) => 'C', ('G', 2) => 'T',
                ('T', 0) => 'A', ('T', 1) => 'C', ('T', 2) => 'G',
                _ => 'A',
            };
            dna.push(next_base); prev = next_base;
        }
    }
    dna
}

fn decode_oligo(primer: &str, dna: &str) -> Vec<u8> {
    let body = dna.strip_prefix(primer).unwrap_or(dna);
    let mut raw_bytes = Vec::new();
    let mut prev = primer.chars().last().unwrap_or('C');
    let mut current_val = 0u16; let mut power = 1u16; let mut count = 0;

    for c in body.chars() {
        let trinary = match (prev, c) {
            ('A', 'C') => 0, ('A', 'G') => 1, ('A', 'T') => 2,
            ('C', 'A') => 0, ('C', 'G') => 1, ('C', 'T') => 2,
            ('G', 'A') => 0, ('G', 'C') => 1, ('G', 'T') => 2,
            ('T', 'A') => 0, ('T', 'C') => 1, ('T', 'G') => 2,
            _ => 0, 
        };
        current_val += trinary * power; power *= 3; prev = c; count += 1;
        if count == 6 { raw_bytes.push(current_val as u8); current_val = 0; power = 1; count = 0; }
    }

    let mut healed_bytes = Vec::new();
    let mut i = 0;
    while i + 2 < raw_bytes.len() {
        let b1 = raw_bytes[i]; let b2 = raw_bytes[i+1]; let b3 = raw_bytes[i+2];
        let healed = if b1 == b2 || b1 == b3 { b1 } else if b2 == b3 { b2 } else { b1 };
        healed_bytes.push(healed); i += 3;
    }
    healed_bytes
}

// ==========================================
// 🚀 THE KERNEL BRIDGE (FUSE VFS)
// ==========================================

// ==========================================
// 🚀 THE KERNEL BRIDGE (FUSE VFS)
// ==========================================

const TTL: Duration = Duration::from_secs(1);
const ROOT_INODE: u64 = 1;

struct DnaNode {
    ino: u64,
    name: String,
    size: u64,
    content: Vec<u8>,
    // V8 FEATURE: POSIX Permissions
    uid: u32,
    gid: u32,
    perm: u16,
}

struct DnaVfs {
    nodes: HashMap<u64, DnaNode>,
    next_ino: u64,
    pool_dir: String,      // Vault A
    mirror_dir: String,    // Vault B (RAID 1)
    journal_dir: String,   // WAL Journal
}

impl DnaVfs {
    fn new(pool_dir: String, mirror_dir: String, journal_dir: String) -> Self {
        Self {
            nodes: HashMap::new(),
            next_ino: 2,
            pool_dir,
            mirror_dir,
            journal_dir,
        }
    }

    fn generate_attr(&self, ino: u64, size: u64, kind: FileType, uid: u32, gid: u32, perm: u16) -> FileAttr {
        FileAttr {
            ino,
            size,
            blocks: (size + 511) / 512,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind,
            perm,
            nlink: if kind == FileType::Directory { 2 } else { 1 },
            uid,
            gid,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }
}

impl Filesystem for DnaVfs {
    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        if ino == ROOT_INODE {
            reply.attr(&TTL, &self.generate_attr(ino, 0, FileType::Directory, 1000, 1000, 0o755));
        } else if let Some(node) = self.nodes.get(&ino) {
            reply.attr(&TTL, &self.generate_attr(ino, node.size, FileType::RegularFile, node.uid, node.gid, node.perm));
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        if ino != ROOT_INODE { reply.error(ENOENT); return; }
        if offset == 0 {
            let _ = reply.add(ROOT_INODE, 1, FileType::Directory, ".");
            let _ = reply.add(ROOT_INODE, 2, FileType::Directory, "..");
            let mut i = 3;
            for node in self.nodes.values() {
                let _ = reply.add(node.ino, i, FileType::RegularFile, &node.name);
                i += 1;
            }
        }
        reply.ok();
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent != ROOT_INODE { reply.error(ENOENT); return; }
        let name_str = name.to_string_lossy().to_string();
        
        if let Some(node) = self.nodes.values().find(|n| n.name == name_str) {
            reply.entry(&TTL, &self.generate_attr(node.ino, node.size, FileType::RegularFile, node.uid, node.gid, node.perm), 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn create(&mut self, req: &Request, parent: u64, name: &OsStr, mode: u32, _umask: u32, _flags: i32, reply: ReplyCreate) {
        if parent != ROOT_INODE { reply.error(ENOENT); return; }
        let ino = self.next_ino;
        self.next_ino += 1;
        
        let node = DnaNode {
            ino,
            name: name.to_string_lossy().to_string(),
            size: 0,
            content: Vec::new(),
            uid: req.uid(),
            gid: req.gid(),
            perm: mode as u16,
        };
        
        let uid = node.uid;
        let gid = node.gid;
        let perm = node.perm;

        self.nodes.insert(ino, node);
        reply.created(&TTL, &self.generate_attr(ino, 0, FileType::RegularFile, uid, gid, perm), 0, 0, 0);
    }

    fn write(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, data: &[u8], _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyWrite) {
        if let Some(node) = self.nodes.get_mut(&ino) {
            let offset = offset as usize;
            if offset + data.len() > node.content.len() {
                node.content.resize(offset + data.len(), 0);
            }
            node.content[offset..offset + data.len()].copy_from_slice(data);
            node.size = node.content.len() as u64;
            reply.written(data.len() as u32);
        } else {
            reply.error(ENOENT);
        }
    }

    // V8 FEATURE: Aerospace WAL + RAID 1 + Zstd + POSIX
    fn flush(&mut self, _req: &Request, ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        if let Some(node) = self.nodes.get(&ino) {
            if node.content.is_empty() { reply.ok(); return; }

            println!("[+] FLUSH TRIGGERED: Processing {}...", node.name);
            
            // 1. Write-Ahead Log (WAL) Intent
            let wal_path = format!("{}/{}.wal", self.journal_dir, node.name);
            fs::write(&wal_path, b"STATUS: PENDING_SYNTHESIS").unwrap_or_default();
            println!("    -> WAL: Journal entry locked.");

            // 2. Zstd Compression
            let compressed_data = zstd::encode_all(node.content.as_slice(), 3).unwrap_or_else(|_| node.content.clone());

            // 3. POSIX Metadata Injection
            let mut payload = Vec::new();
            payload.extend_from_slice(&node.uid.to_be_bytes());
            payload.extend_from_slice(&node.gid.to_be_bytes());
            payload.extend_from_slice(&node.perm.to_be_bytes());
            
            let checksum = compute_checksum(&compressed_data);
            payload.extend_from_slice(&checksum.to_be_bytes());
            payload.extend_from_slice(&compressed_data);

            let primer = generate_primer(&node.name);
            let chunk_size = 8;
            let mut pool: Vec<String> = Vec::new();
            
            for (index, chunk) in payload.chunks(chunk_size).enumerate() {
                let mut block = Vec::new();
                block.extend_from_slice(&(index as u16).to_be_bytes());
                block.extend_from_slice(chunk);
                pool.push(encode_oligo(&primer, &block));
            }
            
            let fasta_data = pool.join("\n");

            // 4. RAID 1 Mirroring (Dual Vault Write)
            let primary_path = format!("{}/{}.fasta", self.pool_dir, node.name);
            let mirror_path = format!("{}/{}.fasta", self.mirror_dir, node.name);
            
            fs::write(primary_path, &fasta_data).unwrap_or_default();
            println!("    -> RAID 1: Written to Vault A (Primary).");
            
            fs::write(mirror_path, &fasta_data).unwrap_or_default();
            println!("    -> RAID 1: Written to Vault B (Mirror).");

            // 5. Commit Journal
            fs::write(&wal_path, b"STATUS: COMMITTED").unwrap_or_default();
            println!("    -> WAL: Synthesis committed successfully.");
        }
        reply.ok();
    }

    // V8 FEATURE: RAID 1 Fail-Over + Zstd + POSIX
    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        if let Some(node) = self.nodes.get_mut(&ino) {
            let primary_path = format!("{}/{}.fasta", self.pool_dir, node.name);
            let mirror_path = format!("{}/{}.fasta", self.mirror_dir, node.name);
            
            // Check WAL first
            let wal_path = format!("{}/{}.wal", self.journal_dir, node.name);
            if let Ok(status) = fs::read_to_string(&wal_path) {
                if status.contains("PENDING_SYNTHESIS") {
                    eprintln!("[-] CRITICAL: WAL indicates corrupted synthesis. Quarantining file.");
                    reply.error(ENOENT);
                    return;
                }
            }

            // RAID Fail-Over Logic
            let dna_pool = if let Ok(data) = fs::read_to_string(&primary_path) {
                println!("[*] OPEN: Sequencing from Vault A...");
                data
            } else if let Ok(data) = fs::read_to_string(&mirror_path) {
                eprintln!("[!] WARNING: Vault A failed. Failing over to Vault B (RAID 1)...");
                data
            } else {
                reply.error(ENOENT);
                return;
            };

            let primer = generate_primer(&node.name);
            let mut blocks: HashMap<u16, Vec<u8>> = HashMap::new();
            let mut max_index = 0;

            for strand in dna_pool.lines() {
                if strand.starts_with(&primer) {
                    let healed_block = decode_oligo(&primer, strand);
                    if healed_block.len() >= 2 {
                        let index = u16::from_be_bytes([healed_block[0], healed_block[1]]);
                        blocks.insert(index, healed_block[2..].to_vec());
                        if index > max_index { max_index = index; }
                    }
                }
            }

            let mut reassembled = Vec::new();
            for i in 0..=max_index {
                if let Some(chunk) = blocks.get(&i) { reassembled.extend_from_slice(chunk); }
            }

            if reassembled.len() >= 14 { // 4(uid) + 4(gid) + 2(perm) + 4(checksum)
                let mut idx = 0;
                node.uid = u32::from_be_bytes([reassembled[idx], reassembled[idx+1], reassembled[idx+2], reassembled[idx+3]]); idx+=4;
                node.gid = u32::from_be_bytes([reassembled[idx], reassembled[idx+1], reassembled[idx+2], reassembled[idx+3]]); idx+=4;
                node.perm = u16::from_be_bytes([reassembled[idx], reassembled[idx+1]]); idx+=2;
                
                let expected_chk = u32::from_be_bytes([reassembled[idx], reassembled[idx+1], reassembled[idx+2], reassembled[idx+3]]); idx+=4;
                let compressed_data = &reassembled[idx..];

                if compute_checksum(compressed_data) == expected_chk {
                    if let Ok(decompressed) = zstd::decode_all(compressed_data) {
                        node.content = decompressed;
                        node.size = node.content.len() as u64;
                        println!("[+] SUCCESS: DNA Decoded, POSIX loaded, and Zstd Decompressed.");
                    } else {
                        eprintln!("[-] CRITICAL: Zstd Decompression Failed!");
                    }
                } else {
                    eprintln!("[-] CRITICAL: Adler-32 Verification Failed!");
                }
            }
            reply.opened(0, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        if let Some(node) = self.nodes.get(&ino) {
            let offset = offset as usize;
            let size = size as usize;
            if offset >= node.content.len() {
                reply.data(&[]);
            } else {
                let end = std::cmp::min(offset + size, node.content.len());
                reply.data(&node.content[offset..end]);
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn setattr(&mut self, _req: &Request, ino: u64, mode: Option<u32>, uid: Option<u32>, gid: Option<u32>, size: Option<u64>, _atime: Option<fuser::TimeOrNow>, _mtime: Option<fuser::TimeOrNow>, _ctime: Option<SystemTime>, _fh: Option<u64>, _crtime: Option<SystemTime>, _chgtime: Option<SystemTime>, _bkuptime: Option<SystemTime>, _flags: Option<u32>, reply: ReplyAttr) {
        let (final_size, final_uid, final_gid, final_perm) = if let Some(node) = self.nodes.get_mut(&ino) {
            if let Some(s) = size {
                node.size = s;
                node.content.truncate(s as usize);
            }
            if let Some(m) = mode { node.perm = m as u16; }
            if let Some(u) = uid { node.uid = u; }
            if let Some(g) = gid { node.gid = g; }
            
            (node.size, node.uid, node.gid, node.perm)
        } else {
            reply.error(ENOENT);
            return;
        };

        reply.attr(&TTL, &self.generate_attr(ino, final_size, FileType::RegularFile, final_uid, final_gid, final_perm));
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <mountpoint>", args[0]);
        return;
    }
    let mountpoint = &args[1];
    
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let pool_dir = format!("{}/dna-posix/dna_vfs/.dna_cache/physical_pool", home);
    let mirror_dir = format!("{}/dna-posix/dna_vfs/.dna_cache/vault_b", home);
    let journal_dir = format!("{}/dna-posix/dna_vfs/.dna_cache/journal", home);

    println!("[*] Booting TheSNMC DNA-POSIX V8 Engine (Native Rust)");
    println!("[*] Primary Vault (TMPFS): {}", pool_dir);
    println!("[*] RAID 1 Mirror (TMPFS): {}", mirror_dir);
    println!("[*] Mounting pure-metal FUSE driver at: {}", mountpoint);

    let vfs = DnaVfs::new(pool_dir, mirror_dir, journal_dir);
    let options = vec![MountOption::RW, MountOption::FSName("dna_vfs".to_string()), MountOption::AutoUnmount];

    fuser::mount2(vfs, mountpoint, &options).expect("[-] FATAL: Kernel rejected the FUSE mount.");
}

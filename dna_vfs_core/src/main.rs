use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request,
};
use libc::ENOENT;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ==========================================
// 📊 SHARED MEMORY (NEURAL LINK FOR TUI)
// ==========================================

struct EngineMetrics {
    raw_bytes: u64,
    compressed_bytes: u64,
    reads: u64,
    writes: u64,
    logs: Vec<String>,
}

impl EngineMetrics {
    fn log(&mut self, msg: String) {
        self.logs.push(msg);
        if self.logs.len() > 15 { self.logs.remove(0); } // Keep last 15 logs on screen
    }
}

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

const TTL: Duration = Duration::from_secs(1);
const ROOT_INODE: u64 = 1;

struct DnaNode {
    ino: u64,
    name: String,
    size: u64,
    content: Vec<u8>,
    uid: u32,
    gid: u32,
    perm: u16,
}

struct DnaVfs {
    nodes: HashMap<u64, DnaNode>,
    next_ino: u64,
    pool_dir: String,      
    mirror_dir: String,    
    journal_dir: String,
    trash_dir: String, // V8 FEATURE: Biological Garbage Collection
    metrics: Arc<Mutex<EngineMetrics>>, // Neural Link to TUI
}

impl DnaVfs {
    fn new(pool_dir: String, mirror_dir: String, journal_dir: String, trash_dir: String, metrics: Arc<Mutex<EngineMetrics>>) -> Self {
        Self {
            nodes: HashMap::new(),
            next_ino: 2,
            pool_dir,
            mirror_dir,
            journal_dir,
            trash_dir,
            metrics,
        }
    }

    fn generate_attr(&self, ino: u64, size: u64, kind: FileType, uid: u32, gid: u32, perm: u16) -> FileAttr {
        FileAttr {
            ino, size, blocks: (size + 511) / 512,
            atime: UNIX_EPOCH, mtime: UNIX_EPOCH, ctime: UNIX_EPOCH, crtime: UNIX_EPOCH,
            kind, perm, nlink: if kind == FileType::Directory { 2 } else { 1 },
            uid, gid, rdev: 0, flags: 0, blksize: 512,
        }
    }
}

impl Filesystem for DnaVfs {
    // V8 FEATURE: 1.0 Petabyte Kernel Illusion
    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        // 1 PB = 274,877,906,944 blocks of 4096 bytes
        let blocks = 274_877_906_944;
        reply.statfs(blocks, blocks, blocks, 0, 1_000_000_000, 4096, 255, 4096);
    }

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
            ino, name: name.to_string_lossy().to_string(), size: 0, content: Vec::new(),
            uid: req.uid(), gid: req.gid(), perm: mode as u16,
        };
        let uid = node.uid; let gid = node.gid; let perm = node.perm;
        self.nodes.insert(ino, node);
        reply.created(&TTL, &self.generate_attr(ino, 0, FileType::RegularFile, uid, gid, perm), 0, 0, 0);
    }

    // V8 FEATURE: Biological Garbage Collection
    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        if parent != ROOT_INODE { reply.error(ENOENT); return; }
        let name_str = name.to_string_lossy().to_string();

        let ino_to_remove = self.nodes.values().find(|n| n.name == name_str).map(|n| n.ino);

        if let Some(ino) = ino_to_remove {
            self.nodes.remove(&ino);
            
            // Move fasta files to the quarantine zone instead of destroying them
            let primary_path = format!("{}/{}.fasta", self.pool_dir, name_str);
            let mirror_path = format!("{}/{}.fasta", self.mirror_dir, name_str);
            let trash_primary = format!("{}/{}_vaultA.fasta", self.trash_dir, name_str);
            let trash_mirror = format!("{}/{}_vaultB.fasta", self.trash_dir, name_str);
            
            let _ = fs::rename(&primary_path, &trash_primary);
            let _ = fs::rename(&mirror_path, &trash_mirror);

            if let Ok(mut m) = self.metrics.lock() { 
                m.log(format!("[🗑️ GC] {} quarantined to .bio_trash", name_str)); 
            }
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
    }

    fn write(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, data: &[u8], _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyWrite) {
        if let Some(node) = self.nodes.get_mut(&ino) {
            let offset = offset as usize;
            if offset + data.len() > node.content.len() { node.content.resize(offset + data.len(), 0); }
            node.content[offset..offset + data.len()].copy_from_slice(data);
            node.size = node.content.len() as u64;
            
            if let Ok(mut m) = self.metrics.lock() { m.writes += 1; }
            reply.written(data.len() as u32);
        } else {
            reply.error(ENOENT);
        }
    }

    fn flush(&mut self, _req: &Request, ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        if let Some(node) = self.nodes.get(&ino) {
            if node.content.is_empty() { reply.ok(); return; }

            if let Ok(mut m) = self.metrics.lock() { 
                m.log(format!("[FLUSH] Processing {}...", node.name)); 
            }
            
            let wal_path = format!("{}/{}.wal", self.journal_dir, node.name);
            fs::write(&wal_path, b"STATUS: PENDING_SYNTHESIS").unwrap_or_default();
            
            if let Ok(mut m) = self.metrics.lock() { m.log(format!("  -> WAL: Journal entry locked.")); }

            let compressed_data = zstd::encode_all(node.content.as_slice(), 3).unwrap_or_else(|_| node.content.clone());
            
            if let Ok(mut m) = self.metrics.lock() {
                m.raw_bytes += node.content.len() as u64;
                m.compressed_bytes += compressed_data.len() as u64;
            }

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
            let primary_path = format!("{}/{}.fasta", self.pool_dir, node.name);
            let mirror_path = format!("{}/{}.fasta", self.mirror_dir, node.name);
            
            fs::write(primary_path, &fasta_data).unwrap_or_default();
            fs::write(mirror_path, &fasta_data).unwrap_or_default();
            fs::write(&wal_path, b"STATUS: COMMITTED").unwrap_or_default();
            
            if let Ok(mut m) = self.metrics.lock() { 
                m.log(format!("  -> RAID 1: Written to Vault A & Vault B.")); 
                m.log(format!("  -> WAL: Synthesis committed successfully.")); 
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        if let Some(node) = self.nodes.get_mut(&ino) {
            let primary_path = format!("{}/{}.fasta", self.pool_dir, node.name);
            let mirror_path = format!("{}/{}.fasta", self.mirror_dir, node.name);
            let wal_path = format!("{}/{}.wal", self.journal_dir, node.name);
            
            if let Ok(status) = fs::read_to_string(&wal_path) {
                if status.contains("PENDING_SYNTHESIS") {
                    if let Ok(mut m) = self.metrics.lock() { m.log(format!("[-] CRITICAL: WAL corrupted. Quarantining file.")); }
                    reply.error(ENOENT); return;
                }
            }

            let dna_pool = if let Ok(data) = fs::read_to_string(&primary_path) {
                if let Ok(mut m) = self.metrics.lock() { m.log(format!("[*] OPEN: Sequencing from Vault A...")); }
                data
            } else if let Ok(data) = fs::read_to_string(&mirror_path) {
                if let Ok(mut m) = self.metrics.lock() { m.log(format!("[!] WARNING: Vault A failed. Failing over to Vault B...")); }
                data
            } else {
                reply.error(ENOENT); return;
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

            if reassembled.len() >= 14 { 
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
                        if let Ok(mut m) = self.metrics.lock() { m.log(format!("[+] SUCCESS: DNA Decoded, POSIX loaded & Decompressed.")); }
                    }
                }
            }
            if let Ok(mut m) = self.metrics.lock() { m.reads += 1; }
            reply.opened(0, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        if let Some(node) = self.nodes.get(&ino) {
            let offset = offset as usize; let size = size as usize;
            if offset >= node.content.len() { reply.data(&[]); } else {
                let end = std::cmp::min(offset + size, node.content.len());
                reply.data(&node.content[offset..end]);
            }
        } else { reply.error(ENOENT); }
    }

    fn setattr(&mut self, _req: &Request, ino: u64, mode: Option<u32>, uid: Option<u32>, gid: Option<u32>, size: Option<u64>, _atime: Option<fuser::TimeOrNow>, _mtime: Option<fuser::TimeOrNow>, _ctime: Option<SystemTime>, _fh: Option<u64>, _crtime: Option<SystemTime>, _chgtime: Option<SystemTime>, _bkuptime: Option<SystemTime>, _flags: Option<u32>, reply: ReplyAttr) {
        let (final_size, final_uid, final_gid, final_perm) = if let Some(node) = self.nodes.get_mut(&ino) {
            if let Some(s) = size { node.size = s; node.content.truncate(s as usize); }
            if let Some(m) = mode { node.perm = m as u16; }
            if let Some(u) = uid { node.uid = u; }
            if let Some(g) = gid { node.gid = g; }
            (node.size, node.uid, node.gid, node.perm)
        } else { reply.error(ENOENT); return; };

        reply.attr(&TTL, &self.generate_attr(ino, final_size, FileType::RegularFile, final_uid, final_gid, final_perm));
    }
}

// ==========================================
// 🖥️ THE OBSERVABILITY DASHBOARD (TUI)
// ==========================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <mountpoint>", args[0]);
        return Ok(());
    }
    let mountpoint = args[1].clone();
    
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let pool_dir = format!("{}/dna-posix/dna_vfs/.dna_cache/physical_pool", home);
    let mirror_dir = format!("{}/dna-posix/dna_vfs/.dna_cache/vault_b", home);
    let journal_dir = format!("{}/dna-posix/dna_vfs/.dna_cache/journal", home);
    let trash_dir = format!("{}/dna-posix/dna_vfs/.bio_trash", home);

    // Initialize Shared Memory
    let metrics = Arc::new(Mutex::new(EngineMetrics {
        raw_bytes: 0, compressed_bytes: 0, reads: 0, writes: 0,
        logs: vec!["[*] System Booting...".to_string(), "[*] Securing RAM Disks...".to_string(), "[*] Engine Online.".to_string()],
    }));

    let vfs = DnaVfs::new(pool_dir, mirror_dir, journal_dir, trash_dir, Arc::clone(&metrics));
    let options = vec![MountOption::RW, MountOption::FSName("dna_vfs".to_string()), MountOption::AutoUnmount];

    // Spawning FUSE to Background Thread
    let _session = fuser::spawn_mount2(vfs, &mountpoint, &options).expect("[-] FATAL: Kernel rejected mount.");

    // TUI Initialization
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)].as_ref())
                .split(f.size());

            let m = metrics.lock().unwrap();

            // TOP: Title
            let title = Paragraph::new(format!("  TheSNMC DNA-POSIX V8 Enterprise Core  |  Live Mount: {}  ", mountpoint))
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(title, chunks[0]);

            // MIDDLE: Dashboard Split
            let middle_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
                .split(chunks[1]);

            // MIDDLE-LEFT: Live Logs
            let log_items: Vec<ListItem> = m.logs.iter().map(|l| ListItem::new(l.as_str())).collect();
            let logs = List::new(log_items).block(Block::default().title(" System Events (FUSE Intercepts) ").borders(Borders::ALL));
            f.render_widget(logs, middle_chunks[0]);

            // MIDDLE-RIGHT: Analytics
            let ratio = if m.raw_bytes > 0 { 100.0 - ((m.compressed_bytes as f64 / m.raw_bytes as f64) * 100.0) } else { 0.0 };
            let stats = format!(
                "\n\n  STORAGE ECONOMICS\n  ------------------\n  Raw Payload:    {} bytes\n  DNA Pool Size:  {} bytes\n  Zstd Saved:     {:.2}%\n\n  ENGINE ACTIVITY\n  ------------------\n  Kernel Writes:  {}\n  Kernel Reads:   {}",
                m.raw_bytes, m.compressed_bytes, ratio, m.writes, m.reads
            );
            let analytics = Paragraph::new(stats).block(Block::default().title(" Live Analytics ").borders(Borders::ALL));
            f.render_widget(analytics, middle_chunks[1]);

            // BOTTOM: Controls
            let footer = Paragraph::new(" Press 'q' to gracefully unmount and exit engine. ")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        })?;

        // Handle Keyboard Exit
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code { break; }
            }
        }
    }

    // TUI Teardown
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    
    println!("[+] Gracefully unmounted FUSE bridge and destroyed UI session.");
    Ok(())
}

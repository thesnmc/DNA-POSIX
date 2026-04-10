use std::env;
use std::fs;
use std::collections::HashMap;

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

// Applies TMR and Goldman Encoding to a byte slice
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

// Decodes Goldman, heals via TMR, and returns raw bytes
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

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 5 { return; }
    let mode = &args[1]; let input_path = &args[2]; let output_path = &args[3]; let filename = &args[4];
    let primer = generate_primer(filename);

    if mode == "encode" {
        let data = fs::read(input_path).unwrap_or_default();
        let checksum = compute_checksum(&data);
        let mut payload = Vec::new();
        payload.extend_from_slice(&checksum.to_be_bytes());
        payload.extend_from_slice(&data);

        // V8 FEATURE: The Block Allocator
        // Slice into 8-byte chunks to keep oligo length under 200 base pairs
        let chunk_size = 8;
        let mut pool: Vec<String> = Vec::new();
        
        for (index, chunk) in payload.chunks(chunk_size).enumerate() {
            let mut block = Vec::new();
            block.extend_from_slice(&(index as u16).to_be_bytes()); // Attach block address
            block.extend_from_slice(chunk);
            let oligo = encode_oligo(&primer, &block);
            pool.push(oligo);
        }
        
        // Output as separate lines, simulating thousands of floating strands
        fs::write(output_path, pool.join("\n")).expect("Write failed");

    } else if mode == "decode" {
        let dna_pool = fs::read_to_string(input_path).unwrap_or_default();
        let mut blocks: HashMap<u16, Vec<u8>> = HashMap::new();
        let mut max_index = 0;

        // V8 FEATURE: Primer Fishing & Liquid Reassembly
        for strand in dna_pool.lines() {
            // Only grab strands that match our file's unique primer barcode
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

        if reassembled.len() >= 4 {
            let mut chk = [0u8; 4]; chk.copy_from_slice(&reassembled[0..4]);
            let expected = u32::from_be_bytes(chk);
            let actual = &reassembled[4..];
            if compute_checksum(actual) == expected {
                fs::write(output_path, actual).unwrap_or_default();
            } else {
                eprintln!("[!] CRITICAL: Reassembled DNA failed Adler-32 verification.");
                fs::write(output_path, "").unwrap_or_default();
            }
        } else { fs::write(output_path, "").unwrap_or_default(); }
    }
}

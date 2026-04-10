#!/usr/bin/env python3
import os, sys, errno, subprocess, json, threading, shutil
from fuse import FUSE, Operations, LoggingMixIn, FuseOSError
from cryptography.fernet import Fernet

RUST_ENGINE = os.path.expanduser("~/dna_vfs_core/target/release/dna_vfs_core")
CACHE_DIR = os.path.expanduser("~/dna_vfs/.dna_cache")
TRASH_DIR = os.path.expanduser("~/dna_vfs/.bio_trash")

KEY_PATH = os.path.expanduser("~/dna_vfs/master.key")
if not os.path.exists(KEY_PATH):
    with open(KEY_PATH, 'wb') as f: f.write(Fernet.generate_key())
with open(KEY_PATH, 'rb') as f: cipher = Fernet(f.read())

def call_rust(mode, in_path, out_path, filename):
    subprocess.run([RUST_ENGINE, mode, in_path, out_path, filename], capture_output=True)

class DNADrive(LoggingMixIn, Operations):
    def __init__(self, physical_root):
        self.root = os.path.realpath(physical_root)
        self.index_fasta = os.path.join(self.root, "__MASTER_INDEX__.fasta")
        self.index_json = os.path.join(self.root, "__MASTER_INDEX__.json")
        self.files = {}
        self.mutex = threading.Lock()
        if not os.path.exists(CACHE_DIR): os.makedirs(CACHE_DIR)
        self._load_index()

    def statfs(self, path):
        blocks = (1024**5) // 4096 
        return {'f_bsize': 4096, 'f_blocks': blocks, 'f_bavail': blocks, 'f_bfree': blocks}

    def _load_index(self):
        if not os.path.exists(self.index_fasta): return
        call_rust('decode', self.index_fasta, self.index_json, '__MASTER_INDEX__')
        try:
            if os.path.exists(self.index_json) and os.path.getsize(self.index_json) > 0:
                with open(self.index_json, 'rb') as f: dec_data = cipher.decrypt(f.read())
                self.files = json.loads(dec_data.decode()).get('files', {})
        except: self.files = {}
        finally:
            if os.path.exists(self.index_json): os.remove(self.index_json)

    def _save_index(self):
        with self.mutex:
            enc_data = cipher.encrypt(json.dumps({'files': self.files}).encode())
            with open(self.index_json, 'wb') as f:
                f.write(enc_data)
                f.flush(); os.fsync(f.fileno())
            call_rust('encode', self.index_json, self.index_fasta, '__MASTER_INDEX__')
            if os.path.exists(self.index_json): os.remove(self.index_json)

    def getattr(self, path, fh=None):
        if path == '/': return {'st_mode': 0o40755, 'st_nlink': 2}
        name = path.strip('/')
        if name not in self.files: raise FuseOSError(errno.ENOENT)
        return {'st_mode': 0o100644, 'st_size': self.files[name]['size'], 'st_nlink': 1}

    def readdir(self, path, fh): return ['.', '..'] + list(self.files.keys())

    def create(self, path, mode, fi=None):
        name = path.strip('/')
        with self.mutex: self.files[name] = {'size': 0}
        open(os.path.join(CACHE_DIR, name), 'w').close()
        self._save_index()
        return 0

    def write(self, path, buf, offset, fh):
        name = path.strip('/')
        cache_p = os.path.join(CACHE_DIR, name)
        phys_p = os.path.join(self.root, name + ".fasta")
        with open(cache_p, 'r+b' if os.path.exists(cache_p) else 'wb') as f:
            f.seek(offset); f.write(buf)
        with self.mutex: self.files[name]['size'] = os.path.getsize(cache_p)
        self._save_index()
        threading.Thread(target=self._async_sync, args=(cache_p, phys_p, name)).start()
        return len(buf)

    def _async_sync(self, cache_p, phys_p, name):
        enc_p = cache_p + ".enc"
        with open(cache_p, 'rb') as f: data = f.read()
        with open(enc_p, 'wb') as f: f.write(cipher.encrypt(data))
        call_rust('encode', enc_p, phys_p, name)
        if os.path.exists(enc_p): os.remove(enc_p)

    def read(self, path, length, offset, fh):
        name = path.strip('/')
        cache_p = os.path.join(CACHE_DIR, name)
        
        if offset == 0:
            phys_p = os.path.join(self.root, name + ".fasta")
            enc_p = cache_p + ".enc"
            call_rust('decode', phys_p, enc_p, name)
            
            if os.path.exists(enc_p) and os.path.getsize(enc_p) > 0:
                with open(enc_p, 'rb') as f: dec_data = cipher.decrypt(f.read())
                with open(cache_p, 'wb') as f: f.write(dec_data)
                os.remove(enc_p)
            elif os.path.exists(cache_p):
                os.remove(cache_p) 
                
        if not os.path.exists(cache_p):
            raise FuseOSError(errno.EIO) 

        with open(cache_p, 'rb') as f:
            f.seek(offset); return f.read(length)

    def unlink(self, path):
        name = path.strip('/')
        if name in self.files:
            with self.mutex: del self.files[name]
            self._save_index()
            phys_p = os.path.join(self.root, name + ".fasta")
            if os.path.exists(phys_p):
                shutil.move(phys_p, os.path.join(TRASH_DIR, name + ".fasta"))
            if os.path.exists(os.path.join(CACHE_DIR, name)):
                os.remove(os.path.join(CACHE_DIR, name))

if __name__ == '__main__':
    if len(sys.argv) < 3: sys.exit(1)
    FUSE(DNADrive(sys.argv[1]), sys.argv[2], nothreads=False, foreground=False)

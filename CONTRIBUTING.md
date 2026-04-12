# Contributing to DNA-POSIX V8

Welcome to the SNMC DNA-POSIX project! We are building the deep-tech infrastructure required to natively bridge the Linux kernel with synthetic biology. 

As an open-source project, we actively welcome pull requests, bug reports, and architectural discussions.

## 🛠️ Current Development Priorities (Help Needed)
If you are looking to contribute, these are our highest priority targets:

1. **The Physical API Socket:** The core engine dynamically scales via an `Arc<Mutex>` SSOT variable. We need developers to help write hardware-specific background threads that ping physical DNA synthesizers (via USB/Network) to update this capacity variable dynamically.
2. **MacOS Compatibility:** The FUSE bridge is currently optimized for Linux (`libfuse`). We are seeking macOS developers to help test and port the FUSE bindings for `macFUSE`.
3. **Dashboard Enhancements:** UI/UX improvements for the Ratatui TUI Matrix Dashboard.

## 🚀 How to Submit a Pull Request
1. Fork the repository.
2. Clone your fork locally and run the `./launch_lab.sh` script to verify your environment.
3. Create a new branch: `git checkout -b feature/your-feature-name`.
4. Make your changes (ensure `cargo fmt` and `cargo clippy` pass).
5. Push to your fork and submit a PR against our `main` branch.

All code must maintain the Zero-Trust security posture (no plaintext leaks in memory) and include tests for the FUSE lifecycle where applicable.

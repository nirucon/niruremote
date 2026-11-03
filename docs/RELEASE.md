# Release checklist

1. Update `Cargo.toml`, `README.md`, `ROADMAP.md` and `docs/ACCEPTANCE.md`.
2. Run:

   ```sh
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ./tests/smoke-local.sh
   cargo build --release --locked
   ```

3. Commit and push `main`.
4. Tag and publish the raw binary plus checksums:

   ```sh
   cp target/release/niruremote niruremote
   sha256sum niruremote > SHA256SUMS
   gh release create v0.1.6 niruremote SHA256SUMS \
     --title "NIRUREMOTE 0.1.6" \
     --notes "Linux/X11 host, SSH tunnel, Tailscale discovery, Windows/macOS clients."
   ```

   `install.sh` downloads exactly:
   `https://github.com/nirucon/niruremote/releases/download/v0.1.6/niruremote`
   and `SHA256SUMS`.

5. Compute the source tag checksum used by NIRUX extras:

   ```sh
   curl -fsSL -o /tmp/niruremote-v0.1.6.tar.gz \
     https://github.com/nirucon/niruremote/archive/refs/tags/v0.1.6.tar.gz
   sha256sum /tmp/niruremote-v0.1.6.tar.gz
   ```

6. Add the extras row in the NIRUX `nirux` script:

   ```text
   niruremote niruremote 0.1.6 <checksum>
   ```

   and the matching row in `nirux/docs/EXTRAS.md`.

7. Verify on a NIRUX machine:

   ```sh
   nirux extras select niruremote
   nirux extras install niruremote
   nirux extras status
   niruremote --version
   ```

8. Run the bare-metal acceptance in `docs/ACCEPTANCE.md` before announcing
   the release as tested.

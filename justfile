default:
	@just --list

build:
	cargo build --workspace

install:
	cargo build --release --workspace
	mkdir -p "$HOME/.local/bin"
	install -m 755 target/release/mm "$HOME/.local/bin/mm"

# Run the CLI locally; pass extra args after `--`, e.g. `just preview -- --help`
run *args:
	cargo run -p matchmaker-cli -F experimental -- {{args}}

# Build static x86_64 binary for Linux (musl)
build-x86:
	cargo zigbuild --release --target x86_64-unknown-linux-musl

# Build static ARM64 binary for Linux (musl)
build-arm:
	cargo zigbuild --release --target aarch64-unknown-linux-musl

# Build macOS Apple Silicon binary (aarch64)
build-mac-arm:
	cargo zigbuild --release --target aarch64-apple-darwin

# Build macOS Intel binary (x86_64)
build-mac-x86:
	cargo zigbuild --release --target x86_64-apple-darwin

# Build Windows binary (x86_64)
build-win:
	cargo zigbuild --release --target x86_64-pc-windows-gnu

# Package release archives for all architectures into dist/
dist version:
	mkdir -p dist
	@echo "Building Linux x86_64 (musl)..."
	cargo zigbuild --release --target x86_64-unknown-linux-musl
	tar -czf dist/mm-{{version}}-x86_64-unknown-linux-musl.tar.gz -C target/x86_64-unknown-linux-musl/release mm
	@echo "Building Linux ARM64 (musl)..."
	cargo zigbuild --release --target aarch64-unknown-linux-musl
	tar -czf dist/mm-{{version}}-aarch64-unknown-linux-musl.tar.gz -C target/aarch64-unknown-linux-musl/release mm
	@echo "Building macOS Apple Silicon..."
	cargo zigbuild --release --target aarch64-apple-darwin
	tar -czf dist/mm-{{version}}-aarch64-apple-darwin.tar.gz -C target/aarch64-apple-darwin/release mm
	@echo "Building Windows x86_64..."
	cargo zigbuild --release --target x86_64-pc-windows-gnu
	zip -q -j dist/mm-{{version}}-x86_64-pc-windows-gnu.zip target/x86_64-pc-windows-gnu/release/mm.exe
	@echo "Generating SHA256 checksums..."
	cd dist && sha256sum mm-{{version}}-* > SHA256SUMS.txt
	@echo "Artifacts generated in dist/:"
	@ls -lh dist/



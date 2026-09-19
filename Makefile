# SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
#
# SPDX-License-Identifier: EUPL-1.2

all: build-all


# Build WebAssembly binaries with WASI

wasi_os = linux
wasi_arch = x86_64
wasi_version = 32
wasi_version_full = $(wasi_version).0
wasi_sdk = wasi-sdk-${wasi_version_full}-${wasi_arch}-${wasi_os}
wasi_dir = $(abspath target/wasi-sdk/$(wasi_sdk))
wasi_cflags = "-Wno-implicit-function-declaration"
wasi_cc = "$(wasi_dir)/bin/clang"
wasi_ld = "$(wasi_dir)/bin/wasm-ld"

# Memory:
# -  Initial heap: 100 MB
# -  Maximum memory: 1 GB
# -  Stack: 15 MB
wasi_rustflags =  -L $(wasi_dir)/share/wasi-sysroot/lib/wasm32-wasip1-threads
wasi_rustflags += -C linker=$(wasi_ld)
wasi_rustflags += -C link-args=--import-memory
wasi_rustflags += -C link-args=--shared-memory
wasi_rustflags += -C link-args=--initial-heap=104857600
wasi_rustflags += -C link-args=--max-memory=1073741824
wasi_rustflags += -C link-args=-zstack-size=15728640

wasmtime_opts := --wasm "threads=y,shared-memory=y" --wasi "threads=y"
wasmtime_ls := target/wasm32-wasip1-threads/debug/t32ls.wasm

.PHONY: wasm-build-debug
wasm-build-debug:
	RUSTFLAGS="$(wasi_rustflags)" CC=$(wasi_cc) CFLAGS=$(wasi_cflags) cargo build --verbose --target=wasm32-wasip1-threads --bin t32ls

.PHONY: wasm-build-release
wasm-build-release:
	RUSTFLAGS="$(wasi_rustflags)" CC=$(wasi_cc) CFLAGS=$(wasi_cflags) cargo build --verbose --target=wasm32-wasip1-threads --profile=release --bin t32ls

.PHONY: wasm-test
wasm-test:
	./ls/tests/wasm.rs | wasmtime $(wasmtime_opts) -- $(wasmtime_ls) --clientProcessId=0 --trace=verbose

.PHONY: wasm-install
wasm-install:
	mkdir --parents target/wasi-sdk
	wget https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${wasi_version}/$(wasi_sdk).tar.gz
	tar xvf $(wasi_sdk).tar.gz --directory=target/wasi-sdk


# Build pipeline for data assets

dir ?= ltb
ugd_asset ?= data/ugd.dat

$(ugd_asset): ugd-build-debug
	cargo run --package t32-language-server-user-guide-data -- --output=$@ ${dir}

.PHONY: ugd-build-debug
ugd-build-debug:
	cargo build --package t32-language-server-user-guide-data --profile=dev --bin t32ls-ugd

.PHONY: ugd-build-release
ugd-build-release:
	cargo build --package t32-language-server-user-guide-data --profile=release --bin t32ls-ugd

.PHONY: ugd-build
ugd-build: ugd-build-debug ugd-build-release

.PHONY: t32ls-build-debug
t32ls-build-debug: $(ugd_asset)
	cargo build --package t32-language-server --profile=dev --bin t32ls


# Build language server binary

.PHONY: t32ls-build-release
t32ls-build-release: $(ugd_asset)
	cargo build --package t32-language-server --profile=release --bin t32ls

.PHONY: t32ls-build
t32ls-build: t32ls-build-debug t32ls-build-release

.PHONY: build-debug
build-all: t32ls-build-debug ugd-build-debug

.PHONY: build-release
build-all: t32ls-build-release ugd-build-release

.PHONY: build-all
build-all: build-debug build-release wasm-build

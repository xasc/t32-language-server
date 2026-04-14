# SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
#
# SPDX-License-Identifier: EUPL-1.2

wasi_os = linux
wasi_arch = x86_64
wasi_version = 32
wasi_version_full = $(wasi_version).0
wasi_sdk = wasi-sdk-${wasi_version_full}-${wasi_arch}-${wasi_os}
wasi_dir = $(abspath target/wasi-sdk/$(wasi_sdk))
wasi_rustflags = "-L $(wasi_dir)/share/wasi-sysroot/lib/wasm32-wasip1-threads"
wasi_cflags = "-Wno-implicit-function-declaration"
wasi_cc = "$(wasi_dir)/bin/clang"

wasmtime_opts := --wasm "threads=y,shared-memory=y" --wasi "threads=y"
wasmtime_ls := target/wasm32-wasip1-threads/debug/t32-language-server.wasm


.PHONY: wasm-build
wasm-build:
	RUSTFLAGS=$(wasi_rustflags) CC=$(wasi_cc) CFLAGS=$(wasi_cflags) cargo build --target=wasm32-wasip1-threads

.PHONY: wasm-test
wasm-test:
	./ls/tests/wasm.rs | wasmtime $(wasmtime_opts) -- $(wasmtime_ls) --clientProcessId=0 --trace=verbose

.PHONY: wasm-install
wasm-install:
	mkdir --parents target/wasi-sdk
	wget https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${wasi_version}/$(wasi_sdk).tar.gz
	tar xvf $(wasi_sdk).tar.gz --directory=target/wasi-sdk

.PHONY: default

default: pull-request-ci

install-dependencies:
	yum update -y
	yum install cmake3 clang-devel zstd libzstd-devel -y
	(ln -s /usr/bin/cmake3 /usr/bin/cmake) || echo "cmake already installed, nothing to do"

install-bpftool-arm:
	curl -fSL "https://github.com/libbpf/bpftool/releases/download/v7.2.0/bpftool-v7.2.0-arm64.tar.gz" -o bpftool.tar.gz \
		&& tar --extract --file bpftool.tar.gz --directory /usr/local/bin \
		&& chmod +x /usr/local/bin/bpftool \
		&& source ~/.bashrc

pipeline-build-arm64v8: install-dependencies install-bpftool-arm
	cargo build --release \
		&& ./package.sh arm64v8/rezolus.tar.gz target/release/rezolus \

pipeline-build:
	echo "NOOP for pipeline-build. Invoke arch-specific Makefile targets instead."

pipeline-synth:
	echo "NOOP for pipeline-synth because we have no infrastructure."

pull-request-ci:
	cargo fmt -- --check \
		&& cargo clippy --all-targets --all-features -- -D warnings -W clippy::unwrap_used -W clippy::todo -W clippy::panic_in_result_fn -W clippy::expect_used \
		&& cargo build --release --features bpf 


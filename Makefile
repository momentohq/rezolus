.PHONY: default

default: pull-request-ci

pipeline-build-amd64:
	cargo build --release --features bpf \
		&& ./package.sh amd64/rezolus.tar.gz target/release/rezolus \

pipeline-build-arm64v8:
	/sbin/bpftool btf dump file /sys/kernel/btf/vmlinux format c > src/common/bpf/vmlinux.h \
		&& cargo build --release --features bpf \
		&& ./package.sh arm64v8/rezolus.tar.gz target/release/rezolus \

pipeline-build:
	echo "NOOP for pipeline-build. Invoke arch-specific Makefile targets instead."

pipeline-synth:
	echo "NOOP for pipeline-synth because we have no infrastructure."

pull-request-ci:
	cargo fmt -- --check \
		&& cargo clippy --all-targets --all-features -- -D warnings -W clippy::unwrap_used -W clippy::todo -W clippy::panic_in_result_fn -W clippy::expect_used \
		&& cargo build --release --features bpf 


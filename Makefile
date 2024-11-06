.PHONY: default

default: pull-request-ci

install-dependencies:
	yum update -y
	yum install clang-devel -y

compile-binaries: install-dependencies
	cargo --version \
	&& cargo build --locked --release

pipeline-build-arm64v8: compile-binaries
	./package.sh arm64v8/rezolus.tar.gz target/release/rezolus

pipeline-build:
	echo "NOOP for pipeline-build. Invoke arch-specific Makefile targets instead."

pipeline-synth:
	echo "NOOP for pipeline-synth because we have no infrastructure."

pull-request-ci:
	cargo fmt -- --check \
		&& cargo clippy --all-targets --all-features -- -D warnings -W clippy::unwrap_used -W clippy::todo -W clippy::panic_in_result_fn -W clippy::expect_used \
		&& cargo build --release


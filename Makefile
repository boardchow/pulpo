.PHONY: install uninstall run test stress

install:
	cargo install --path . --locked

uninstall:
	cargo uninstall pulpo

run:
	cargo run --release

test:
	cargo test

stress:
	cargo test --release --test stress -- --ignored --nocapture

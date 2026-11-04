.PHONY: install uninstall run test

install:
	cargo install --path . --locked

uninstall:
	cargo uninstall pulpo

run:
	cargo run --release

test:
	cargo test

.PHONY: install uninstall run test stress docker

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

docker:
	cargo test sets_up_any_compose_project -- --ignored

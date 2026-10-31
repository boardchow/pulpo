# Pulpo — a VS Code replacement in one Ghostty window.
#   make install    build and install pulpo into ~/.cargo/bin (on your PATH with rustup)
#   make uninstall  remove it
#   make run        build and run from the checkout
#   make test       unit tests and the server end-to-end test

.PHONY: install uninstall run test

install:
	cargo install --path . --locked

uninstall:
	cargo uninstall pulpo

run:
	cargo run --release

test:
	cargo test

check:
	cargo fmt --all
	cargo clippy --all-features --tests -- -Dwarnings
	cargo test --all-features

default: check

check: check-sube check-scales check-libwallet

check-sube:
	@just -f crates/sube/justfile check lint

check-scales:
	@just -f crates/scales/justfile check lint

check-libwallet:
	@just -f crates/libwallet/justfile check lint

test: test-scales test-sube

test-scales:
	@just -f crates/scales/justfile test

test-sube:
	@just -f crates/sube/justfile test

ci:
	@just -f crates/scales/justfile check-targets test
	@just -f crates/sube/justfile ci

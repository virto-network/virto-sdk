default: check

check: check-sube check-scales check-libwallet

check-sube:
	@just -f lib/sube/justfile check lint

check-scales:
	@just -f lib/scales/justfile check lint

check-libwallet:
	@just -f lib/libwallet/justfile check lint

test: test-scales test-sube

test-scales:
	@just -f lib/scales/justfile test

test-sube:
	@just -f lib/sube/justfile test

ci:
	@just -f lib/scales/justfile check-targets test
	@just -f lib/sube/justfile ci

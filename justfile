default: check

check: check-sube check-scales check-libwallet

check-sube:
	@just -f sube/justfile check lint

check-scales:
	@just -f scales/justfile check lint

check-libwallet:
	@just -f libwallet/justfile check lint

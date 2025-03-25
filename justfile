default: check

check: check-sube check-scales check-libwallet check-sdk

check-sube:
	@just -f lib/sube/justfile check lint

check-scales:
	@just -f lib/scales/justfile check lint

check-libwallet:
	@just -f lib/libwallet/justfile check lint

check-sdk:
	@just -f sdk/js/justfile check

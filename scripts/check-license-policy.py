#!/usr/bin/env python3
"""Keep incompatible license families out of deny.toml's allowlist."""

from pathlib import Path
import sys
import tomllib


DENIED_LICENSES = {
    "AGPL-1.0-only",
    "AGPL-1.0-or-later",
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "BUSL-1.1",
    "Proprietary",
    "Commercial",
}


def main() -> int:
    config_path = Path(__file__).resolve().parents[1] / "deny.toml"
    with config_path.open("rb") as config_file:
        config = tomllib.load(config_file)

    allowed = set(config.get("licenses", {}).get("allow", []))
    violations = sorted(
        license_id
        for license_id in allowed
        if license_id in DENIED_LICENSES
        or license_id.startswith(("AGPL-", "BUSL-", "LicenseRef-"))
    )

    if violations:
        print("Disallowed license identifiers found in deny.toml allowlist:")
        for license_id in violations:
            print(f"  - {license_id}")
        return 1

    print("License policy check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

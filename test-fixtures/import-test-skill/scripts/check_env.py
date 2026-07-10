#!/usr/bin/env python3
"""Import test helper — confirms scripts/ was imported."""

import platform
import sys

print("[import-test-skill] check_env.py ran OK")
print(f"python={sys.version.split()[0]} platform={platform.system()}")

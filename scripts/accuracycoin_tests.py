#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""AccuracyCoin's tests as the reference reads them from the pinned .asm, as JSON.

Run on the reference's interpreter; famicom-accuracycoin reads the list.

    accuracycoin_tests.py OUT.json
"""

import json
import sys
from pathlib import Path

from famicom_python import testrom

HOME = Path(testrom.__file__).resolve().parents[2] / "tests" / "test_roms" / "AccuracyCoin"
tests = testrom.accuracycoin_tests((HOME / "AccuracyCoin.asm").read_text())
Path(sys.argv[1]).write_text(json.dumps([t.__dict__ for t in tests], indent=0))
print(f"{len(tests)} tests")

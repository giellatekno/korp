#!/usr/bin/env python3

"""
Small script to change the pointed-to-data paths inside the regsitry files
to match the new setup on gtweb-02. Inside the container image, the CWB files
are mounted as /corpora/registry/ and /corpora/data/

The script may need adjusting for the various places it is run. The paths are
different on local machines, as opposed to servers.
"""

# the lines we want to change are like this:
# HOME /corpora/gt_cwb/data/xxx_category_yyyymmdd

import re
from pathlib import Path

cwd = Path(".").resolve()

for path in Path("registry").iterdir():
    contents = path.read_text()
    name = path.name
    contents = re.sub("HOME .*", f"HOME /corpora/data/{name}", contents)
    contents = re.sub("INFO .*", f"INFO /corpora/data/{name}/.info", contents)
    path.write_text(contents)

print("done")

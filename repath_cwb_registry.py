#!/usr/bin/env python3

"""
Update paths in CWB registry files.

If the CWB registry and data files are moved from the directory path where they
were first created, the paths inside the registry files will be wrong. This
is also relevant when the paths are mounted in a container, as the mount point
in a container is often different from where they are stored outside.
"""

import argparse
from pathlib import Path

cwd = Path(".").resolve()


def parse_args():
    parser = argparse.ArgumentParser(
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description=(
            "Update paths in CWB registry files."
            "\n\nUse --data-root to explicitly set the new data root if the data files are not located in "
            "\"<registry_path>/../data\"."
        ),
    )
    parser.add_argument("registry_path", help="Path to the registry directory")
    parser.add_argument("--data-root", help="New data path. Defaults to \"<registry_path>/../data\"")
    parser.add_argument("--no-keep", action="store_true", help="Do not keep the old paths as comments in the registry files")
    args = parser.parse_args()
    return args


def main():
    args = parse_args()
    reg_path = Path(args.registry_path)
    data_path = args.data_root
    data_path = Path(data_path)
    keep = not args.no_keep

    if data_path is None:
        data_path = reg_path.parent.joinpath("data")

    for path in Path(reg_path).iterdir():
        contents = path.read_text()
        corpus_name = path.name
        updated_contents = []
        for line in contents.splitlines():
            if line.startswith("HOME "):
                if keep:
                    updated_contents.append("#" + line)
                updated_path = data_path / corpus_name
                updated_contents.append(f"HOME {updated_path}")
            elif line.startswith("INFO "):
                if keep:
                    updated_contents.append("#" + line)
                updated_path = data_path / corpus_name / ".info"
                updated_contents.append(f"INFO {updated_path}")
            else:
                updated_contents.append(line)

        updated_contents = "\n".join(updated_contents)
        path.write_text(updated_contents)

    print("done")


if __name__ == "__main__":
    raise SystemExit(main())

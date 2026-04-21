#!/usr/bin/env python3

"""Common tasks for Korp.

tasks.py <front|back> <smi|nsu|other> <build|push|runlocal|bap>

There is no special handling of "all" languages, to do the same tasks for
every lang. Instead, sh scripting, e.g.:

for lang in sma sme smj smn sms koi kpv mdf mhr mrj myv udm fao fit fkv olo vep vro; do
    uv run tasks.py build front $lang;
done
"""

import argparse
import os
from dataclasses import dataclass
from pathlib import Path

ACR = "gtlabcontainerregistry.azurecr.io"
INSTANCES = ["smi", "nsu", "other"]
LANGS = [
    "sma", "sme", "smj", "smn", "sms", "koi",
    "kpv", "mdf", "mhr", "mrj", "myv", "udm",
    "fao", "fit", "fkv", "olo", "vep", "vro",
]
CMDS = ["build", "build-cwb", "push", "sync-settings", "run", "bap"]


def port_of(frontorback, lang):
    port = 1390
    port += len(LANGS) if frontorback == "front" else 0
    port += LANGS.index(lang)
    return port


CONTAINERFILE_BACKEND = """
FROM docker.io/library/debian:trixie AS builder

# anders: we have to install headers for pypi mysqlclient to be able to build
# https://github.com/PyMySQL/mysqlclient/tree/main#linux

# TODO: Memcached server ? MariaDB Server?

RUN set -eux && \
    apt-get update && \
    apt-get install --no-install-recommends -y \
        git curl build-essential pkg-config \
        python3 python3-venv python3-pip python3-dev \
        default-libmysqlclient-dev libglib2.0-0

WORKDIR /korp

RUN set -eux && \
    git clone --depth 1 https://github.com/spraakbanken/korp-backend.git

RUN python3 -m venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"

# fix for bug that recently appeared when starting gunicorn:
# "pkg_resources.DistributionNotFound: The 'zope.event' distribution was not found and is required by the application"
RUN sed --in-place 's/gevent.*/gevent~=25.5/' /korp/korp-backend/requirements.txt

RUN pip install --disable-pip-version-check --requirement /korp/korp-backend/requirements.txt
RUN pip install gunicorn



FROM docker.io/library/debian:trixie

RUN set -eux; \
    apt-get update; \
    apt-get install --no-install-recommends -y python3 libmariadb3 libglib2.0-0

# the created virtual environment, with all requirements installed
COPY --from=builder /opt/venv /opt/venv

# the code repository, as cloned from git
COPY --from=builder /korp/korp-backend /korp/korp-backend

# the CWB binaries we need
COPY --from=cwb /tmp/cwb-{CWB_VERSION}-linux-x86_64.tar.gz /cwb/cwb-{CWB_VERSION}-linux-x86_64.tar.gz
RUN tar --directory /cwb -zxvf /cwb/cwb-{CWB_VERSION}-linux-x86_64.tar.gz
RUN cp -r /cwb/cwb-{CWB_VERSION}-linux-x86_64/bin/* /usr/bin
RUN cp -r /cwb/cwb-{CWB_VERSION}-linux-x86_64/lib/libcl.so /usr/lib/libcl.so
#COPY --from=builder /usr/bin/cqp /usr/bin/cqp
#COPY --from=builder /usr/bin/cwb-scan-corpus /usr/bin/cwb-scan-corpus
#COPY --from=builder /usr/lib/libcl.so /usr/lib/libcl.so

# Some korp or cwb process needs "gawk" exactly, "awk" isn't good enough for them.
RUN ln -s /usr/bin/awk /usr/bin/gawk

# This essentially activates the virutal environment
ENV PATH="/opt/venv/bin:$PATH"
#ENV PYTHONPATH="/opt/venv/lib"

WORKDIR /korp/korp-backend
CMD ["gunicorn", "--worker-class", "gevent", "--bind", "0.0.0.0:1234", "--workers", "4", "--max-requests", "250", "--limit-request-line", "0", "run:create_app()" ]
"""


def run_cmd(cmd, *args, **kwargs):
    from subprocess import run

    if isinstance(cmd, str):
        from shlex import split as split_cmd
        cmd = split_cmd(cmd)

    if not isinstance(cmd, list):
        raise TypeError("argument 'cmd' must be a list or a string")

    print(" ".join(cmd))

    try:
        return run(cmd, **kwargs)
    except KeyboardInterrupt:
        pass


def build_cwb():
    cmd = "podman build -t cwb -f containerfiles/Containerfile.cwb"
    run_cmd(cmd, encoding="utf-8")
    # tag with version number that we just built
    proc = run_cmd("podman run --rm cwb cat /tmp/VERSION", capture_output=True)
    cwb_version = proc.stdout.decode("utf-8").strip()
    run_cmd(f"podman image tag cwb cwb:{cwb_version}")


def build_frontend_base():
    print("Building korp-frontend-base image")
    cwd = os.getcwd()
    tag = "korp-frontend-base"
    containerfile = "containerfiles/Containerfile.frontend-base"
    run_cmd(f"podman build -t {tag} -f {containerfile} {cwd}")


def build_front(lang):
    assert isinstance(lang, str)
    build_frontend_base()
    cwd = os.getcwd()
    tag = f"korp-frontend-{lang}"
    containerfile = "containerfiles/Containerfile.frontend"

    cmd = (
        "podman build "
        f"-t {tag} "
        f"--build-arg=instance={lang} "
        f"-f {containerfile} {cwd}"
    )
    run_cmd(cmd, encoding="utf-8")


def build_back():
    import json
    from functools import cmp_to_key
    cmd = "podman image list --format json --filter=reference=cwb"
    proc = run_cmd(cmd, capture_output=True)
    stdout = proc.stdout.decode("utf-8")
    try:
        images = json.loads(stdout)
    except Exception as e:
        print(f"error json-parsing output from cmd: {cmd}")
        print(e)
        raise
    if not images:
        print("Need built cwb image. Hint: run the 'build-cwb' command")
        return

    versions = []
    for name in images[0]["Names"]:
        if not name.startswith("localhost/cwb:"):
            continue
        version = name.removeprefix("localhost/cwb:")
        if version == "latest":
            continue
        versions.append(version)

    def compare_semver(a: str, b: str):
        a = tuple(int(x) for x in a.split("."))
        b = tuple(int(x) for x in b.split("."))
        if a < b:
            return -1
        elif a == b:
            return 0
        else:
            return 1

    versions.sort(key=cmp_to_key(compare_semver))
    cwb_version = versions[-1]
    cmd = f"podman build -t korp-backend -f - {os.getcwd()}"
    run_cmd(cmd, input=CONTAINERFILE_BACKEND.format(CWB_VERSION=cwb_version), encoding="utf-8")


def run_front(lang, backend):
    if backend is None:
        backend = f"http://127.0.0.1:{port_of('back', lang)}"

    if not (backend.startswith("https://") or backend.startswith("http://")):
        print("Error: backend must start with http:// or https://")
        return 1

    run_cmd(
        "podman run --rm "
        f"--name korp-frontend-{lang} "
        f"-e BACKEND={backend} "
        f"-p {port_of('front', lang)}:80 "
        f"korp-frontend-{lang}"
    )


def check_korp_backend_config(config_py, corpus_config, cwbfiles):
    p = Path(config_py)
    if not p.is_file():
        raise Exception(f"Error: {config_py} is not a file")

    p = Path(corpus_config)
    if not p.is_dir():
        raise Exception(f"Error: {corpus_config} is not a directory")
    need = set(["corpora", "attributes", "modes"])
    msg = f"Error: corpus_config directory '{corpus_config}'"
    for f in p.iterdir():
        if not f.is_dir():
            raise Exception(f"{msg} contains an entry which is not a directory: {f}")
        try:
            need.remove(f.name)
        except KeyError:
            raise Exception(f"{msg} contains an unexpected directory: {f.name}")
    if need:
        raise Exception(f"{msg} missing the following directories: {need}")

    p = Path(cwbfiles)
    if not p.is_dir():
        raise Exception(f"cwbfiles path '{cwbfiles}' is not a directory")
    need = set(["data", "registry"])
    for f in p.iterdir():
        if f.is_dir() and f.name in need:
            need.remove(f.name)
    if need:
        raise Exception(f"cwbfiles directory missing directories: {need}")


def run_back(lang, cwbfiles, corpus_config=None, custom_run_cmd=None):
    assert lang in LANGS
    assert isinstance(cwbfiles, str)
    cwd = os.getcwd()
    if corpus_config is None:
        corpus_config = f"{cwd}/gtweb2_config/corpus_configs/{lang}"

    config_py = f"{cwd}/gtweb2_config/config.py"
    check_korp_backend_config(config_py, corpus_config, cwbfiles)

    args = (
        f"--name korp-backend-{lang} "
        "--rm "
        "--replace "
        f"-p {port_of('back', lang)}:1234 "
        f"-v {config_py}:/korp/korp-backend/instance/config.py "
        f"-v {corpus_config}:/corpora/corpus_config "
        f"-v {cwbfiles}:/corpora"
    )
    if custom_run_cmd is None:
        custom_run_cmd = ""
    else:
        args = f"{args} -it"
    run_cmd(f"podman run {args} korp-backend {custom_run_cmd}")


def push_front(lang):
    run_cmd(f"podman tag korp-frontend-{lang} {ACR}/korp-frontend-{lang}")
    run_cmd(f"podman push {ACR}/korp-frontend-{lang}")


def push_back():
    run_cmd(f"podman tag korp-backend {ACR}/korp-backend")
    run_cmd(f"podman push {ACR}/korp-backend")


def bap_front(lang):
    build_front(lang)
    push_front(lang)


def bap_back():
    build_back()
    push_back()


def sync_settings():
    run_cmd(
        "rsync "
        "-rv "  # r = recursively, v = verbose
        "--stats "  # show stats at the end
        "gtweb2_config/ "  # copy everything in the directory
        "gtweb-02.uit.no:/home/services/korp/config/"  # to this directory
    )


@dataclass
class Args:
    cmd: str  # Literal["build", "run"]
    frontorback: str  # Literal["front", "back"]
    lang: str
    cwbfiles: str
    backend: str
    korp_config: Path
    # for giving a custom CMD to the run command when running a container
    custom_run_cmd: str


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("args", nargs="*")
    parser.add_argument("--cwbfiles")
    parser.add_argument("--backend")
    parser.add_argument(
        "--custom-run-cmd",
        help="override the Containerfile CMD (only for 'run' command)",
    )
    parser.add_argument(
        "--korp-config",
        help=(
            "directory where the backend korp config dir is (used "
            "with run back)"
        ),
    )

    args = parser.parse_args()

    cmd = None
    frontorback = None
    lang = None

    for arg in args.args:
        if arg in LANGS:
            if lang is not None:
                parser.error("multiple languages given, only give one")
            lang = arg
        elif arg in ["front", "back"]:
            if frontorback is not None:
                parser.error("can't give both front and back, choose one")
            frontorback = arg
        elif arg in CMDS:
            if cmd is not None:
                parser.error("can only give one command")
            cmd = arg

    return Args(
        cmd=cmd,
        frontorback=frontorback,
        lang=lang,
        cwbfiles=args.cwbfiles,
        backend=args.backend,
        korp_config=args.korp_config,
        custom_run_cmd=args.custom_run_cmd,
    )


if __name__ == "__main__":
    match parse_args():
        case Args("build-cwb"):
            build_cwb()
        case Args("build", "front", lang=None) as args:
            print("error: build front: missing argument: lang")
            print("  give one of:", ", ".join(LANGS))
        case Args("build", "front", lang):
            build_front(lang)
        case Args("build", "back") as args:
            build_back()
        case Args("run", "front", lang=None) as args:
            print("error: run front: missing 3rd argument: lang")
            print(f"  give one of: {', '.join(LANGS)}")
        case Args("run", "front", lang, _cwbfiles, backend) as args:
            run_front(lang, backend)
        case Args("run", "back", cwbfiles=None) as args:
            print("Need to specify --cwbfiles <path to built cwb files>")
            print("anders: locally I use /home/anders/misc/cwb/corpora/gt_cwb/registry/")
        case Args("run", "back", lang, cwbfiles, custom_run_cmd=custom_run_cmd) as args:
            run_back(lang, cwbfiles, custom_run_cmd=custom_run_cmd)
        case Args("run", "back", lang, cwbfiles, _backend, korp_config) as args:
            run_back(lang, cwbfiles, korp_config)
        case Args("push", "back"):
            push_back()
        case Args("push", "front", lang=None):
            print("error: push front: missing argument: lang")
            print(f"  give one of: {', '.join(LANGS)}")
        case Args("push", "front", lang):
            push_front(lang)
        case Args("bap", "front", lang=None):
            print("error: bap front: missing argument: lang")
            print(f"  give one of: {', '.join(LANGS)}")
        case Args("run" | "build", frontorback) as args:
            print("error: front or back?")
        case Args("sync-settings"):
            sync_settings()
        case Args(cmd):
            print("error: no cmd found in arguments")
            print(f"give one of: {', '.join(CMDS)}")

#!/usr/bin/env python3

"""Common tasks for Korp.

tasks.py <front|back> <smi|nsu|other> <build|push|runlocal|bap>
"""

import argparse
import os
from dataclasses import dataclass

ACR = "gtlabcontainerregistry.azurecr.io"
INSTANCES = ["smi", "nsu", "other"]
LANGS = [
    "sma", "sme", "smj", "smn", "sms", "koi",
    "kpv", "mdf", "mhr", "mrj", "myv", "udm",
    "fao", "fit", "fkv", "olo", "vep", "vro",
]
CMDS = ["build", "push", "sync-settings", "run", "bap"]


def port_of(frontorback, lang):
    port = 1390
    port += len(LANGS) if frontorback == "front" else 0
    port += LANGS.index(lang)
    return port


DOCKERFILE_FRONTEND = """
FROM docker.io/library/debian:bookworm AS builder

ARG instance

RUN <<EOF
    set -eux
    apt-get update
    apt-get install -y --no-install-recommends git nginx npm
    npm install --global yarn
    git clone --branch master --depth 1 https://github.com/spraakbanken/korp-frontend.git /korp/korp-frontend
EOF

WORKDIR /korp

COPY ./gtweb2_config/front/config-${instance}.yaml /korp/korp-frontend/app/config.yml
RUN mkdir -p /korp/korp-frontend/app/modes

# yarn build failed on this file not being present, with "invalid syntax",
# because some file did a require() on this, which some earlier part of the
# build process replaced with some non-js text about the file not being
# found.
RUN touch /korp/korp-frontend/app/modes/default_mode.js

# Extra translation files for the frontend
COPY ./gtweb2_config/translations/* /korp/korp-frontend/app/translations

RUN <<EOF
    set -eux
    cd /korp/korp-frontend
    yarn
    yarn build
EOF


FROM docker.io/library/nginx

#COPY ./korp-nginx-frontend.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /korp/korp-frontend/dist /usr/share/nginx/html/

RUN grep -l "korp_backend_url:[ ]\\?\\"[^\\"]\\+\\"" /usr/share/nginx/html/*.js > /js_file

RUN <<EOF
if [ $(wc -l </js_file) -ne 1 ]; then
    echo "IMAGE BUILD ERROR: Cannot find .js file with defintion of korp_backend_url, cannot continue."
    exit 1;
fi
EOF

RUN echo '#!/bin/bash' > /entry.sh
RUN echo 'if [ ! -v BACKEND ]; then' >>/entry.sh
RUN echo '  echo "Fatal: env var BACKEND is not set"' >>/entry.sh
RUN echo 'fi' >>/entry.sh
RUN echo 'sed -i "s,korp_backend_url:[ ]\\?\\"[^\\"]\\+\\",korp_backend_url: \\"${BACKEND}\\","' "$(cat /js_file)" >>/entry.sh
RUN echo 'exec "$@"' >>/entry.sh
RUN chmod +x /entry.sh

ENTRYPOINT [ "/entry.sh" ]
CMD ["nginx", "-g", "daemon off;"]
"""


DOCKERFILE_BACKEND = """
FROM docker.io/library/debian:bookworm AS builder

# anders: we have to install headers for pypi mysqlclient to be able to build
# https://github.com/PyMySQL/mysqlclient/tree/main#linux

RUN set -eux && \
    apt-get update && \
    apt-get install --no-install-recommends -y \
        git curl build-essential \
        python3 python3-venv python3-pip python3-dev \
        default-libmysqlclient-dev libglib2.0-0 libpcre3

# CWB
RUN set -eux && \
    curl --location --silent --show-error --create-dirs --output-dir /cwb --remote-name https://downloads.sourceforge.net/project/cwb/cwb/cwb-3.5/deb/cwb_3.5.0-1_amd64.deb && \
    dpkg -i $(find /cwb -name "*.deb")

# Memcached server ? MariaDB Server?

WORKDIR /korp

RUN set -eux && \
    git clone --depth 1 https://github.com/spraakbanken/korp-backend.git
    #git clone --branch giellatekno --single-branch --depth 1 https://github.com/giellatekno/korp-backend.git /korp/korp-backend

RUN python3 -m venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"
RUN pip install --disable-pip-version-check --requirement /korp/korp-backend/requirements.txt
RUN pip install gunicorn



FROM docker.io/library/debian:bookworm

RUN set -eux; \
    apt-get update; \
    apt-get install --no-install-recommends -y python3 libmariadb3 libglib2.0-0 libpcre3

# the created virtual environment, with all requirements installed
COPY --from=builder /opt/venv /opt/venv

# the code repository, as cloned from git
COPY --from=builder /korp/korp-backend /korp/korp-backend

# the CWB binaries we need
COPY --from=builder /usr/bin/cqp /usr/bin/cqp
COPY --from=builder /usr/bin/cwb-scan-corpus /usr/bin/cwb-scan-corpus
COPY --from=builder /usr/lib/libcl.so /usr/lib/libcl.so

# This essentially activates the virutal environment
ENV PATH="/opt/venv/bin:$PATH"

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
        run(cmd, **kwargs)
    except KeyboardInterrupt:
        pass


def build_front(lang):
    assert isinstance(lang, str)

    cmd = (
        "podman build "
        f"-t korp-frontend-{lang} "
        f"--build-arg=instance={lang} "
        f"-f - {os.getcwd()}"
    )
    run_cmd(cmd, input=DOCKERFILE_FRONTEND, encoding="utf-8")


def build_back():
    cmd = f"podman build -t korp-backend -f - {os.getcwd()}"
    run_cmd(cmd, input=DOCKERFILE_BACKEND, encoding="utf-8")


def run_front(lang, backend):
    if backend is None:
        backend = f"http://localhost:{port_of('back', lang)}"

    run_cmd(
        "podman run --rm "
        f"--name korp-frontend-{lang} "
        f"-e BACKEND={backend} "
        f"-p {port_of('front', lang)}:80 "
        f"korp-frontend-{lang}"
    )


def run_back(lang, cwbfiles):
    assert lang in LANGS
    assert isinstance(cwbfiles, str)
    print(lang, cwbfiles)
    cwd = os.getcwd()
    args = (
        f"--name korp-backend-{lang} "
        "--rm "
        "--replace "
        f"-p {port_of('back', lang)}:1234 "
        f"-v {cwd}/gtweb2_config/config.py:/korp/korp-backend/instance/config.py "
        f"-v {cwd}/gtweb2_config/corpus_configs/{lang}:/corpora/corpus_config "
        f"-v {cwbfiles}:/corpora/gt_cwb"
    )
    run_cmd(f"podman run {args} korp-backend")


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
        "--stats "
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


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("args", nargs="*")
    parser.add_argument("--cwbfiles")
    parser.add_argument("--backend")

    args = parser.parse_args()

    cmd = None
    frontorback = None
    lang = None

    for arg in args.args:
        if arg in LANGS:
            lang = arg
        elif arg in ["front", "back"]:
            frontorback = arg
        elif arg in CMDS:
            cmd = arg

    return Args(
        cmd=cmd,
        frontorback=frontorback,
        lang=lang,
        cwbfiles=args.cwbfiles,
        backend=args.backend,
    )


if __name__ == "__main__":
    match parse_args():
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
        case Args("run", "back", lang, cwbfiles) as args:
            run_back(lang, cwbfiles)
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

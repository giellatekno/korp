#!/usr/bin/env python3

"""Common tasks for Korp.

tasks.py <front|back> <smi|nsu|other> <build|push|runlocal|bap>
"""

import argparse
import os
import sys
#sys.path.insert(0, "/home/anders/temp/python-playground/decorator_argparse/")
#from decorator_argparse import Argparser, Param, handler, subcommands  # noqa: E402


ACR = "gtlabcontainerregistry.azurecr.io"
INSTANCES = ["smi", "nsu", "other"]
CMDS = ["build", "push", "run", "bap"]

DOCKERFILE_FRONTEND = """
FROM docker.io/library/debian:bookworm AS builder

ARG instance
ARG backend=https://gtweb.uit.no/korp-backend-${instance}

RUN set -eux && \
    apt-get update && \
    apt-get install -y --no-install-recommends git nginx npm && \
    npm install --global yarn

WORKDIR /korp

RUN set -eux && \
    git clone --branch master --depth 1 https://github.com/spraakbanken/korp-frontend.git /korp/korp-frontend

COPY ./gtweb2_korp_settings/front/config-${instance}.yaml /korp/korp-frontend/app/config.yml
RUN sed --in-place "s,^korp_backend_url:.*$,korp_backend_url: ${backend}," /korp/korp-frontend/app/config.yml
RUN mkdir -p /korp/korp-frontend/app/modes

# yarn build failed on this file not being present, with "invalid syntax",
# because some file did a require() on this, which some earlier part of the
# build process replaced with some non-js text about the file not being
# found.
RUN touch /korp/korp-frontend/app/modes/default_mode.js

# Extra translation files for the frontend
COPY ./gtweb2_korp_settings/translations/* /korp/korp-frontend/app/translations

RUN set -eux && \
    cd /korp/korp-frontend && \
    yarn && \
    yarn build


FROM docker.io/library/nginx

#COPY ./korp-nginx-frontend.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /korp/korp-frontend/dist /usr/share/nginx/html/
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

    try:
        run(cmd, **kwargs)
    except KeyboardInterrupt:
        pass


def build(frontorback, instance, backend):
    if frontorback == "front":
        input = DOCKERFILE_FRONTEND
        args = (
            f"-t korp-frontend-{instance} "
            f"--build-arg=instance={instance} "
        )
        if backend is not None:
            args += f"--build-arg=backend={backend} "
    elif frontorback == "back":
        input = DOCKERFILE_BACKEND
        args = "-t korp-backend"

    cwd = os.getcwd()
    cmd = f"podman build {args} -f - {cwd}"
    run_cmd(cmd, input=input, encoding="utf-8")


def run(frontorback, instance, cwbfiles):
    cwd = os.getcwd()
    if frontorback == "back":
        if cwbfiles is None:
            sys.exit("Need to specify --cwbfiles (where the built cwb files are located)")
        image = "korp-backend"
        args = (
            f"--name korp-backend-{instance} "
            "--rm "
            "--replace "
            "-p 1390:1234 "
            f"-v {cwd}/config.py:/korp/korp-backend/instance/config.py "
            f"-v {cwd}/gtweb2_korp_settings/corpus_configs/{instance}:/corpora/gt_cwb/corpus_config "
            f"-v {cwbfiles}:/corpora"
        )
    elif frontorback == "front":
        image = f"korp-frontend-{instance}"
        args = (
            "--name korp-frontend-smi "
            "--rm "
            "--replace "
            "-p 1395:80 "
        )

    run_cmd(f"podman run {args} {image}")


def push_front(instance):
    print("push frontend", instance)
    return
    if instance is None:
        instance = INSTANCES
    for inst in instance:
        run(f"podman tag korp-frontend-{inst} {ACR}/korp-frontend-{inst}")
        run(f"podman push {ACR}/korp-frontend-{inst}")


def push_back(instance):
    print("push backend", instance)
    return
    run(f"podman tag korp-backend {ACR}/korp-backend")
    run(f"podman push {ACR}/korp-backend")


def bap_front(instance):
    print("bap frontend")


def bap_back(instance):
    build("back", instance)
    push_back(instance)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("args", nargs="*")
    parser.add_argument("--cwbfiles")
    parser.add_argument("--backend")

    args = parser.parse_args()

    instance = None
    frontorback = None
    cmd = None

    for arg in args.args:
        if arg in INSTANCES:
            instance = arg
        elif arg in ["front", "back"]:
            frontorback = arg
        elif arg in CMDS:
            cmd = arg

    if instance is None:
        sys.exit(f"need an instance, one of {', '.join(INSTANCES)}")
    if frontorback is None:
        sys.exit("need to know 'front' or 'back'")
    if cmd is None:
        sys.exit(f"need a cmd, one of {', '.join(CMDS)}")

    return instance, frontorback, cmd, args.cwbfiles, args.backend


if __name__ == "__main__":
    instance, frontorback, cmd, cwbfiles, backend = parse_args()

    if cmd == "build":
        build(frontorback, instance, backend)
    elif cmd == "run":
        run(frontorback, instance, cwbfiles)
    else:
        print(f"TODO run command: {cmd}")

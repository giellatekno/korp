# Our setup

This repository contains the configuration files for our setup of Korp, and
also gathers the upstream repositories as submodules. This readme explains
our setup.

```
github.com/giellatekno/korp  (this repository)
  - readme/documentation
  - our settings
    - web-facing nginx reverse proxy configuration (nginx_reverse_proxy_locations.conf)
    - backend/instance/config.py where?
    - corpus_config for each instance (smi, nsu, other)
  - submodule: korp-frontend (github.com/giellatekno/korp-frontend)
      fork of spraakbanken/korp-frontend
      follows upstream, except for the 'giellatekno' branch, which tries to
         follow dev, but with our changes intact
  - submodule: korp-backend (github.com/giellatekno/korp-backend)
      fork of spraakbanken/korp-backend
      follows upstream
```


# Top-down overview

## Reverse proxy (web-facing web-server)

The `gtweb.uit.no` server runs nginx. It acts as a reverse proxy for our Korp
instances. We have 3 instances of Korp: sami languages (*smi*), non-sami uralic
languages (abbreviated *nsu*, **n**on-**s**ami **u**ralic), and one for the...
others (*other*). Each of these require their own `korp-frontend`, and
`korp-backend`. Hence we have 6 routes in our web-facing nginx server. The
configuration for the web-facing nginx is in `nginx_reverse_proxy_locations.conf`.

We use Korp's *"modes"* to represent languages. One language per mode.
Because each "mode" in Korp is presented as one text entry in the top right
navigation bar, and we want to limit how how many there are, and prevent the
dropdown selector from showing, we limit ourselves to some 5, 6 or 7 modes, and
because of this, we run three instances of Korp.


## The services

Both services are built as containers, in our case using `podman`, but it should
be interchangible with `docker`. The summary is as follows:

```
korp-frontend-smi - static nginx, exposed at port 1341, run with KORP_BACKEND_URL set to /backend/korp-smi
    podman run -d -p 1341:80 -e "KORP_BACKEND_URL=https://gtweb.uit.no/backend/korp-smi" --name korp-frontend-smi korp-frontend

korp-backend-smi
    type: gunicorn korp-backend, exposed at port 1342
    command: podman run --privileged -d -p 1342:1234 -v /corpora/gt_cwb:/corpora/gt_cwb -v /home/anders/korp/corpus_configs/smi:/opt/korp-backend/corpus_config -v /home/anders/korp/config.py:/korp/korp-backend/instance/config.py --security-opt label=disable --name korp-backend-smi korp-backend

korp-frontend-nsu - static nginx, exposed at port 1343, run with KORP_BACKEND_URL set to /backend/korp-nsu
    podman run -d -p 1343:80 -e "KORP_BACKEND_URL=https://gtweb.uit.no/backend/korp-nsu" --name korp-frontend-nsu korp-frontend

korp-frontend-other - static nginx, exposed at port 1345, run with KORP_BACKEND_URL set to /backend/korp-other
    podman run -d -p 1345:80 -e "KORP_BACKEND_URL=https://gtweb.uit.no/backend/korp-other" --name korp-frontend-other korp-frontend
```

### Building and deplying

*These are the steps as of now. They may change. Maybe we'll use dedicated image builders,
maybe we'll use a self-hosted or some registry, and push and pull images from there instead
of manually saving, scping and loading. Maybe we'll use compose to orchistrate this. Maybe there
will be scripts that automate some of this... Possibilities are endless...*

1. Build the images locally, tag them as `korp-frontend` or `korp-backend`.
2. Save the image to a .tar file locally, e.g.: `podman save -o korp-frontend.tar korp-frontend`
3. scp this file to the server
4. [on server] Use podman to load this image, e.g.: `podman load -i korp-fronted.tar`
    1. NOTE: MAY HAVE TO REMOVE THE OLD ONE FIRST(??)
5. [on server] Stop and remove the old container: `podman stop korp-frontend-smi; podman rm korp-frontend-smi`
6. [on server] Create and start the new one, using `podman run` commands above.
7. [on server] Replace the systemd unit file. It should be named e.g. `/etc/systemd/user/korp-frontend-smi.service`.


### korp-frontend

`korp-frontend` is a *SPA* (**S**ingle-**P**age **A**pplication), built as a
completely static web site. The resulting built folder (named "*dist*") is
hosted with nginx.

#### Backend configuration

Run the container with the environment variable `KORP_BACKEND_URL` to set where
the frontend tries to reach the backend. (**Note:** This is a change from the
upstream source repository, as the upstream just has this information statically
defined at build time).

#### Build options

The `Dockerfile` is split in a builder, and a slimmer image that copies and
uses only the necessary build artifacts. If desired, it's also possible to
build the final image by copying the locally built *dist* folder into the
image. See the comments in the `Dockerfile` in the repository.


### korp-backend

#### Building for deployment

The image is built with the command: `podman build -t korp-backend -f Dockerfile`
(be sure to be in the `korp-backend` directory).

A builder step installs and builds the requirements, which are then copied over
to the final image. Python requirements are c modules, which needs headers to be built,
as well as CWB binaries that are installed.

#### Running the built image on the server

All required configuration files and paths are mapped into the container at
runtime, using *bind mounts* (argument `-v OUTSIDE_PATH:INSIDE_PATH` passed to
`podman run`, where `OUTSIDE_PATH` means a path on the system that runs the
container, and `INSIDE_PATH` means where this will be visible inside the container).
These are:

- `-v /home/anders/korp/config.py:/korp/korp-backend/instance/config.py`.
  This is the "root" configuration file for the backend. It defines where the
  other paths are. (**Note**: local path will change!)
- `-v /corpora/gt_cwb:/corpora/gt_cwb`. This entire folder is mapped in to the
  same path in the container. It's where the CWB files are. We map it to this
  specific path, because that's where the `config.py` tells Korp to look.
- `-v /home/anders/korp/corpus_configs/INSTANCE:/opt/korp-backend/corpus_config`
  (replace `INSTANCE` with `smi`, `nsu` or `other`). This is where Korp reads
  about the corpuses, and how to present them, what options are set, etc.
  We run three instances of Korp, and each gets their own `corpus_config`.
  We map it into `/opt/korp-backend/corpus_config` inside the container, because
  our `config.py` file (mapped earlier) defines that this is where to look.

In addition to the *bind mounts* for configuration, we have to set the port
binding, using `-p OUTSIDE_PORT:1234`, where we replace `OUTSIDE_PORT` with
`1341` for `smi`, `1343` for `nsu` and `1345` for `other`.

Finally, there are some flags we must give for the container to function
properly, namely `--privileged` and `--security-opt label=disable`.

The full command is therefore:

```bash
podman run --privileged -d -p OUTSIDE_PORT:1234 -v /corpora/gt_cwb:/corpora/gt_cwb -v /home/anders/korp/corpus_configs/INSTANCE:/opt/korp-backend/corpus_config -v /home/anders/korp/config.py:/korp/korp-backend/instance/config.py --security-opt label=disable --name korp-backend-INSTANCE korp-backend
```


### The server

On the server, we set up systemd units that runs each container for us. We first
create the container, using the `podman run` commands, with a given `--name`,
so that we can can then use `podman generate systemd NAME > /etc/systemd/user/NAME.service`
to generate the systemd unit file for that service.


## Updating corpora

The usual `CorpusTools` approach (steps 1-4), followed by more "server-administrative" tasks as the final steps (steps 5-8):

1. import (`add_files_to_corpus`) - adds the files under some specified category, writes corresponding meta-file (xsl)
    1. fill in meta data
2. extract content from sources (`convert2xml`)
3. analyse using analyser (`analyse_corpus`) (requires analyser for that language built with correct ./configure params)
4. make korp-mono ready (`korp_mono`)  - extracts from analysis, converts the format to CWB-input format, and concatenates all files under a subcategory to one file (e.g. "sme_admin", admin is the category, so "sme_admin" will be one "corpus" (as defined by CWB))
5. pack it up (`pack_cwbmono_files`) (which really just makes a tarball)
6. send it to the server (scp) (note: HARD TO AUTOMATE)
7. [on server] unpack and update registry-files paths (`unpack_cwbmono_files`)
8. Make sure to update the `corpus_config`. Change dates of corpora files, etc. (needs more explanation)
9. [on server] Restart the Korp instance for it to take effect (`podman restart korp-backend-<group>`, `group` being `smi` (sami), `nsu` (non-sami uralic), `other` [??])
    1. Subject to change if deployed differently (but it should ideally just be to restart some pod somewhere, or a similarly simple command)


## Keeping CWB and corpus_config in sync

There is no system for this. Anything that is in the corpus_config must be
present in the CWB files. CWB files that are not referenced in the corpus_config
are ignored.


## Other

### Difficulty in running locally for testing and development

The frontend is simple, and should just be cloning, `yarn` to install dependencies,
and a `yarn dev` to start the development server. The problem is that it needs
a backend, otherwise it will just show an error screen.

The backend is a bit more work. It needs a locally setup `corpus_config`, `CWB`
tools installed, as well as the `config.py` itself. It's possible to just copy
over some of the corpora that one wants to test against, but it's a little
bit of work.

Another issue is testing something against the full dataset. The entire dataset
of just the CWB files themselves are 5.9 GB.



## Database

We need it up and running. Probably just a mariadb container, running with
a bind mount to a database folder on the host.

This must be reachable from the backend. We could use a podman network, or just expose a port
locally and connect to that through the host.

We need to create the tables, the schema for them are in the documentation, so
that's fine. However, grepping through the source reveals 0 "INSERT" statements,
meaning we must fill the database ourselves. How do we do that?
-- I assume it's a one-time thing, but we still have to do it.

Later problems: How do we make sure that there aren't problems if we remove,
add, or rename (update) some corpora?

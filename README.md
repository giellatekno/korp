# Our setup

This repository gathers the upstream repositories as submodules, and is for
keeping the settings for our setup.


## Overview

```
github.com/giellatekno/korp  (this repository)
  - readme/documentation
  - our settings
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

Because each "mode" in Korp is presented as one text entry in the top right
navigation bar, and we want to limit how how many there are, and prevent the
dropdown selector from showing, we limit ourselves to some 6 or 7 modes.

We use "modes" to represent languages.

## korp-frontend

Can build locally, or using the Dockerfile. The final step is to make a static
nginx image.

The Dockerfile is twofold, the first image is a builder, and installs
npm, yarn, etc, so it can be built into the static html, css and js folder.

Is run in a container using `podman`. The container runs nginx. It is statically
defined in the js code where to contact the backend. Therefore, a change has
been made so that when the container runs, a shellscript changes the code
before nginx is run.

Run with the environment variable KORP_BACKEND_URL set to the backend-url.


## korp-backend

A builder step installs and builds the requirements, which are then copied over
to the final image. Python requirements are c modules, which needs headers to be built,
as well as CWB binaries that are installed.

`podman build -t korp-backend -f Dockerfile`

The backend expects to find the CWB corpus folder (the one containing data/ and
registry/ folders), in `/corpus` - because the setting is set like that in the default config.

CWB_REGISTRY - the CWB registry files (the registry files must have the data paths also correct accordingly)
CORPUS_CONFIG_DIR - the folder with the yaml files

Q: if I have -v /corpus:/corpus, can I then also have -v /some/other/folder:/corpus/corpus_config

# CORPUS_CONFIG_DIR = /korp/korp-backend/corpus_config   - Korp sin config, må definere korpus
# CWB_REGISTRY = /corpus/registry  - hvor Korp forventer å finne de korpusene som er definert i CORPUS_CONFIG_DIR
# ...disse kan vel begge to mappes inn.


## Updating corpora

The usual `CorpusTools` approach (steps 1-4), followed by more "server-administrative" tasks as the final steps (steps 5-8):

1) import (`add_files_to_corpus`) - adds the files under some specified category, writes corresponding meta-file (xsl)
1.2) fill in meta data
2) extract content from sources (`convert2xml`)
3) analyse using analyser (`analyse_corpus`)
    (requires analyser for that language built with correct ./configure params)
4) make korp-mono ready (`korp_mono`)  - extracts from analysis, converts the format to CWB-input format, and concatenates all files under a subcategory to one file (e.g. "sme_admin", admin is the category, so "sme_admin" will be one "corpus" (as defined by CWB))
5) pack it up (`pack_cwbmono_files`) (which really just makes a tarball)
6) send it to the server (scp) (note: HARD TO AUTOMATE)
7) [on server] unpack and update registry-files paths (`unpack_cwbmono_files`)
8) [on server] Restart the Korp instance for it to take effect (`podman restart korp-backend-<group>`, `group` being `smi` (sami), `nsu` (non-sami uralic), `other` [??])
8.1) Subject to change if deployed differently (but it should ideally just be to restart some pod somewhere, or a similarly simple command)

Thoughts:

If adding corpus (a new lang, or a new category inside a lang),
or changing some "attributes", then the corresponding `corpus_config` must
also be changed - where do they reside?


## Keeping CWB and corpus_config in sync

There is no system for this. Anything that is in the corpus_config must be
present in the CWB files. CWB files that are not referenced in the corpus_config
are ignored.

## The CORPUS_CONFIG

Korp (the python app) needs configuration, it is stored in a folder, usually
named `corpus_config`.


# Putting it all together

On the incoming server, gtweb.uit.no, we have nginx. We need 6 services. 2 per
group, and we have 3 groups.

podman containers:

```
korp-frontend-smi - static nginx, exposed at port 1341, run with KORP_BACKEND_URL set to /backend/korp-smi
    podman run -d -p 1341:80 -e "KORP_BACKEND_URL=https://gtweb.uit.no/backend/korp-smi" --name korp-frontend-smi korp-frontend

korp-frontend-nsu - static nginx, exposed at port 1343, run with KORP_BACKEND_URL set to /backend/korp-nsu
    podman run -d -p 1343:80 -e "KORP_BACKEND_URL=https://gtweb.uit.no/backend/korp-nsu" --name korp-frontend-nsu korp-frontend

korp-frontend-other - static nginx, exposed at port 1345, run with KORP_BACKEND_URL set to /backend/korp-other
    podman run -d -p 1345:80 -e "KORP_BACKEND_URL=https://gtweb.uit.no/backend/korp-other" --name korp-frontend-other korp-frontend
```

## Containers

Container (name): `korp-backend-smi`, gunicorn hosting the korp backend,
exposed at port 1342 (not reachable from outside, due to the firewall),
using the smi config.

**Podman** command to start it:

```bash
podman run --privileged -d -p 1342:1234 -v /corpora/gt_cwb/registry:/corpus:ro -v <***corpus_config on server ...korp/corpus_configs/smi***>:/corpus/corpus_config --security-opt label=disable --name korp-backend-smi korp-backend
```

```bash
korp-backend-nsu - gunicorn hosting korp-backend with corpus_config "nsu", exposed at port 1344
korp-backend-other - gunicorn hosting korp-backend with corpus_config "other", exposed at port 1346
```

This means that we need 6 routes in the entrypoint nginx server
They are defined on the server in the file `/etc/nginx/default.d/korp_2023.conf`,
and they are:

```
/2023_korp  (will be /korp) - proxies to port 1341
(NOT YET) /ukorp - proxies to port 1343
(NOT YET) /fkorp - proxies to port 1345

/backend/korp-smi - proxies to port 1342
(NOT YET) /backend/korp-nsu - proxies to port 1344
(NOT YET) /backend/korp-other - proxies to port 1346
```

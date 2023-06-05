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


### korp-frontend

Korp-frontend is a *SPA*, built as a completely static web site. The resulting
built ("*dist*") folder is hosted with nginx. It is statically defined in the
javascript code where to contact the backend. Therefore, a change has been made,
so that when the container runs, a shellscript changes the code to set the
backend url, before nginx is run. Run the container with the environment
variable `KORP_BACKEND_URL` to set the url the frontend tries to contact.

The `Dockerfile` is split in a builder, and a slimmer image that copies and
uses only the necessary build artifacts. If desired, it's also possible to
build the final image by copying the locally built *dist* folder into the
image.

### korp-backend

A builder step installs and builds the requirements, which are then copied over
to the final image. Python requirements are c modules, which needs headers to be built,
as well as CWB binaries that are installed.

`podman build -t korp-backend -f Dockerfile`

The backend expects to find the CWB corpus folder (the one containing data/ and
registry/ folders), in `/corpus` - because the setting is set like that in the default config.

CWB_REGISTRY - `/corpus/registry`, the CWB registry files (the registry files must have the data paths also correct accordingly)
CORPUS_CONFIG_DIR - the folder with the yaml files, refers to CWB files found in CWB_REGISTRY


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


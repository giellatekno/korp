# Our korp setup

This repository contains the the builder and configuration files for
our instances of Korp. We build one image for the backend, and one for the
frontend, for each of the 18 languages we have corpora for.

File listing of this repository:

```
github.com/giellatekno/korp  (this repository)
  README.md              - this file
  tasks.py               - script to build and push images, and run them locally
  gtweb2_korp_settings/
    translations/        - translations needed by the frontend,
                           will be built into the image (same for all instances)
    front/
      config-LANG.yam    - frontend config, will be built into the image
                           (one file per LANG)
    corpus_config/       - backend config (one folder per LANG)
      LANG/              
        attributes/
          structural/    - contains metadata fields of the corpora,
                           stuff like title, one file per property
                           date, isbn, etc
          positional/    - data per word, such as pos, deprel, etc,
                           one file per column (per property)
        corpora/         - one .yaml file per cwb corpora,
                           e.g. "sme_admin_20211118.yaml",
        modes/           - each "sub-corp". So we could have one for the
                           default korp for that language. one for historical
                           corpora, one for parallell, etc. språkbanken
                           uses modes for separating them corpora like that.
```


## The images

The **Dockerfile**s to build the images is embedded in the source code of
**tasks.py**. On the server, running them is done with the
**gtweb-service-script**. It knows where this repo is located, so it can
mount in the configuration files from this repo correctly.


## Frontend image

The frontend image takes two arguments: *lang* and *backend*. *lang* is which
corpora this is the frontend for, which is one of the 12 different languages
we have a corpora for. *backend* is the url to the backend to use for this
specific frontend. Building the frontend image is done with the command:

```
python tasks.py build front <lang> <--local|--prod|--backend=BACKEND>
```

`--local` is shorthand for setting the backend to
`http://localhost:{port_of('back', lang)}`. `--prod` is shorthand for setting
the backend to `https://gtweb-02.uit.no/korp/backend-{lang}`.

*Anders: I would ideally like to be able to set as many variables at image
runtime, because then I can build just one image, and run them with different
arguments - but upstream doesn't support it.*.


## Backend image

The backend reads config files at startup to determine all of its settings,
so there is just one built image. Running a different instance is just a matter
of mounting in different config files. Build the backend using

```
python tasks.py build back
```

## The config


## Running locally

While testing

t to build images, and to push them to the *ACR*
(*Azure Container Registry*) that we have on Azure (which the server then
*pulls* images from). It can also be used to run the images locally, but
you do need to have the compiled cwb files for the backend to actually have
any data.


## Running on the server

All required configuration files and paths are mapped into the container at
runtime, using *bind mounts*. The configuration files 





(argument `-v OUTSIDE_PATH:INSIDE_PATH` passed to
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

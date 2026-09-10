# gen-altorth-korps

A utility to process historical corpora into cwb files, ready for Korp.

The generated files will reside in a `<cwd>/generated` directory.
Additionally, in the corpus directories, `.tokenized` files of cached
data will be created. These can be deleted with the `clean-tokenized`
command, i.e. `cargo run --release -- clean-tokenized`.


# What it does

1. Finds the corpora root, by default from the root setting in `gut`.
   If `gut` is not installed on the system, the process aborts with
   an error message. The `--corpora-root` argument can be given to
   override this, and set the corpora root directly without using
   the `gut` config. If `gut` is not installed on the system, using
   this argument is required.
2. Finds all corpus `.xml` files in the `hist/` section. If no files
   are found, the process aborts.
3. Read all those files, parsing the xml. If the document has no
   `<orthography>` element in the `<header>`, a warning message is
   printed, as that indicates an error in the corpus data. The same
   goes for a missing or empty `<body>`.

   The content of the `<body>` is then tokenized. Unless the
   `--cache-policy` is set to `never`, it looks for cached body in
   a `<filename>.xml.tokenized` file. If found, that data is used.
   If not, `hfst-tokenize` will be called with the `sme` tokeniser,
   and the output of that process cached. The cache policy can be
   overridden with `--cache-policy auto|never|require`. See `--help`
   for more information.
4. The metadata and tokenized body of each file is written out to
   `.vrt` files, in the `generated/vrt/` directory. One `.vrt` file
   will be written for each orthography, for each category. So, files
   for like `generated/vrt/leem_bible.vrt` will be written. This __vrt__
   data is the input for Corpus WorkBench ("cwb").
5. Corpus WorkBench tools will be run to create each corpus, one `.vrt`
   file = one cwb "corpus". Each corpus will get it's own pair of a
   `generated/cwb/registry/<orthography>_<category>` file, and corresponding
   `generated/cwb/data/<orthography>_<category>/` directory (which contains
   lots of binary cwb-files with the actual data.


## Remaining manual work

From this point on, all that's left to do, is to upload the cwb files,
and place them where Korp looks for cwb data. Remember to update the path
in the registry file to point to where the data files are. For us, Korp backend
runs in a container, and expects to find the cwb files under `/corpora/(data|registry)`,
regardless of where those files are stored on the host system, and as such,
for us, the path to the data files in the registry files will be `/corpora/data/CORPUS_NAME`.

Finally, on the Korp side, we need to update the settings, for Korp-backend to
know that these corpora are available. Update the corresponding files in
`korp/gtweb2_config/corpus_configs/<LANG>/`. Add a `.yaml` file in the `corpora`
directory (using another existing one as a template for what info belongs in there,
and, optionally, add another mode in the `modes` directory. Look at the existing
files in this directory to figure out what goes where.


mod cli;
mod cwb;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cwb::{cwb_encode, cwb_makeall, cwb_huffcode, cwb_compress_rdx};

use clap::Parser;
use cmd_lib::run_fun;

use crate::cli::CachePolicy;

//use hfst::pmatch::PmatchContainer;

fn is_sentence_separator(s: &str) -> bool {
    matches!(s, "." | "?" | "!")
}

/// A paragraph break in hfst-tokenize output: an empty line (which is what a
/// blank line in the input becomes), or a `¶`. It ends the current sentence,
/// but is not a token itself.
fn is_paragraph_break(s: &str) -> bool {
    s.is_empty() || s == "¶"
}

struct SplitSentences<'a> {
    it: std::str::Split<'a, char>,
}

fn split_sentences<'a>(tokenized: &'a str) -> SplitSentences<'a> {
    SplitSentences {
        it: tokenized.split('\n'),
    }
}

impl<'a> Iterator for SplitSentences<'a> {
    type Item = Vec<&'a str>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut cur = vec![];
        while let Some(item) = self.it.next() {
            if is_paragraph_break(item) {
                if !cur.is_empty() {
                    return Some(cur);
                }
                continue;
            }

            cur.push(item);
            if is_sentence_separator(item) {
                return Some(cur);
            }
        }
        if !cur.is_empty() {
            return Some(cur);
        }
        None
    }
}

// <title/>
// <genre code="hist"/>
// <author><unknown/></author>
// <year>1757</year>
// <wordcount>8607</wordcount>
// <conversion_status type="ocr"/>
// <availability><license type="standard"/></availability>
// <multilingual><language xml:lang="dan"/></multilingual>
// <metadata><uncomplete/></metadata>
// <version>XSLtemplate $Revision$; file-specific xsl  Revision; common.xsl  $Revision$; </version>
// <orthography>leem</orthography></header>

//    root.set("title", f_title)
//    root.set("lang", f_lang)
//    root.set("orig_lang", f_orig_lang)
//    root.set("first_name", f_first_name_author)
//    root.set("last_name", f_last_name_author)
//    root.set("nationality", f_nationality)
//    root.set("gt_domain", DOMAIN_MAPPING[f_genre])
//    root.set("date", f_date)
//    root.set("datefrom", f_datefrom)
//    root.set("dateto", f_dateto)
//    root.set("timefrom", f_timefrom)
//    root.set("timeto", f_timeto)

struct Text {
    title: String,
    lang: String,
    orig_lang: String,
    first_name: String,
    last_name: String,
    nationality: String,
    gt_domain: String,
    date: String,
    datefrom: String,
    dateto: String,
    timefrom: String,
    timeto: String,
}

impl Text {
    fn write_text_attributes(&self, vrt: &mut std::fs::File) -> std::io::Result<()> {
        use std::io::Write;

        write!(vrt, "title=\"{}\" ", self.title)?;
        write!(vrt, "lang=\"{}\" ", self.lang)?;
        write!(vrt, "orig_lang=\"{}\" ", self.orig_lang)?;
        write!(vrt, "first_name=\"{}\" ", self.first_name)?;
        write!(vrt, "last_name=\"{}\" ", self.last_name)?;
        write!(vrt, "nationality=\"{}\" ", self.nationality)?;
        write!(vrt, "gt_domain=\"{}\" ", self.gt_domain)?;
        write!(vrt, "date=\"{}\" ", self.date)?;
        write!(vrt, "datefrom=\"{}\" ", self.datefrom)?;
        write!(vrt, "dateto=\"{}\" ", self.dateto)?;
        write!(vrt, "timefrom=\"{}\" ", self.timefrom)?;
        write!(vrt, "timeto=\"{}\"", self.timeto)?;
        Ok(())
    }

    // Usually the "domain" is part of the document header, but for hist
    // it is set to "hist", and the subdirectory inside hist determines
    // the domain instead, so we pass it in
    fn from_doc(doc: &gtcorpusutil::ConvertedDocument, domain: &str) -> Self {
        let title = doc.title().unwrap_or("").to_string();
        let (date, datefrom, dateto) = gtcorpusutil::parse_year(doc.year());
        Self {
            title,
            lang: String::from("sme"),
            orig_lang: String::from("sme"),
            // TODO check these fields
            first_name: String::from("?"),
            last_name: String::from("?"),
            nationality: String::from("?"),
            gt_domain: domain.to_string(),
            date,
            datefrom,
            dateto,
            timefrom: String::from("000000"),
            timeto: String::from("235959"),
        }
    }
}

fn gather_texts(
    files: Vec<(PathBuf, String)>,
    cache_policy: CachePolicy,
) -> anyhow::Result<HashMap<(String, String), Vec<(Text, String)>>> {
    let mut out = HashMap::new();
    for (path, category) in files {
        let content = std::fs::read_to_string(&path)?;
        let xml_doc = gtcorpusutil::ConvertedDocument::new(&content)?;
        let Some(orth) = xml_doc.orthography() else {
            eprintln!(
                "warn: file contains no <orthography> in <header>, even though it's in hist/ - {}",
                path.display()
            );
            continue;
        };
        let Some(body) = xml_doc.body() else {
            eprintln!("warn: file contains no <body>! {}", path.display());
            continue;
        };

        let text = Text::from_doc(&xml_doc, category.as_str());
        let paragraphs = body_paragraphs(&body);
        let tokenized = match get_tokenized(&path, &paragraphs, cache_policy) {
            GetTokenizedResult::Ok(t) => t,
            GetTokenizedResult::OnlyCached => {
                continue;
            }
            GetTokenizedResult::Err(e) => anyhow::bail!(e),
        };

        let key = (orth.to_string(), category.clone());
        out.entry(key).or_insert(vec![])
            .push((text, tokenized));
    }
    Ok(out)
}

/// The state of a file that we want to process.
#[derive(Default)]
struct File {
    path: PathBuf,
    category: String,
    contents: Option<String>,
    xml: Option<Text>,
    orthography: Option<String>,
    tokenized: Option<String>,
}

impl File {
    fn new(path: PathBuf, category: String) -> Self {
        Self {
            path,
            category,
            ..Default::default()
        }
    }
}

fn is_desired_corpus(name: &gtcorpusutil::CorpusName) -> bool {
    name.lang == "sme" && !name.is_orig && name.is_open()
}

// (path, category)
fn find_files(gut_root: gtcorpusutil::Root) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let sme = gut_root
        .corpora()
        .find(|corpus| is_desired_corpus(&corpus.corpus_name))
        .ok_or(anyhow::anyhow!("sme open corpus not found"))?;
    let converted = sme.into_converted();

    println!("Finding files in hist/ ...");

    Ok(converted
        .files()
        .filter(|f| f.is_hist())
        .filter_map(|f| {
            f.category()
                .map(|category| (f.to_path_buf(), category.as_str().to_owned()))
                .ok()
        })
        .collect())
}

fn clean_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(&path) {
        Ok(()) => std::fs::create_dir_all(&path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir_all(&path),
        Err(e) => Err(e),
    }
}

enum GetTokenizedResult {
    Ok(String),
    OnlyCached,
    Err(anyhow::Error),
}

/// Max size of the text given to one hfst-tokenize process. hfst-tokenize's
/// memory use keeps growing with the amount of input it has seen (~4 GB for
/// ~700 KB of text), so large files are tokenized in several runs, each well
/// within the prlimit below. Paragraphs are never split across runs.
const TOKENIZE_CHUNK_BYTES: usize = 100_000;

/// Group paragraphs into chunks of at most `TOKENIZE_CHUNK_BYTES` (a single
/// larger paragraph gets a chunk of its own), joined by blank lines.
fn chunk_paragraphs(paragraphs: &[String]) -> Vec<String> {
    let mut chunks = vec![];
    let mut cur = String::new();
    for p in paragraphs {
        if !cur.is_empty() && cur.len() + 2 + p.len() > TOKENIZE_CHUNK_BYTES {
            chunks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(p);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

fn get_tokenized(path: &Path, paragraphs: &[String], cache_policy: CachePolicy) -> GetTokenizedResult {
    let tokenizer = "/usr/share/giella/sme/tokeniser-disamb-gt-desc.pmhfst";
    let cached_tokenized_path = path.with_added_extension("tokenized");
    match std::fs::read_to_string(&cached_tokenized_path) {
        Ok(s) => return GetTokenizedResult::Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if cache_policy == CachePolicy::Require {
                return GetTokenizedResult::OnlyCached;
            }
            // anders: hfst-tokenize is potentially very memory-hungry, up to the point where
            // a system can completely freeze due to heavy memory pressure (which is presumably
            // not a technical deadlock or processor halt, but the kernel itself being unable
            // to do anything but wait for the memory system, for an amount of time that is
            // unacceptable, so we are right to perceive it as a freeze.)
            // Therefore: limit the virtual address space (not physical RAM, but it bounds it)
            // of the hfst-tokenize process to ~6GB, and kill it after 15 CPU seconds.
            // `prlimit` (util-linux) is used rather than `ulimit`, because `ulimit` is a shell
            // builtin that can't wrap a command, and cmd_lib doesn't run a shell.
            // Note: prlimit takes bytes; `ulimit -v` takes KiB.
            let mut tokenized = String::new();
            for chunk in chunk_paragraphs(paragraphs) {
                match run_fun!(echo $chunk | prlimit --as=6000000000 --cpu=15 hfst-tokenize $tokenizer) {
                    Ok(t) => {
                        tokenized.push_str(&t);
                        // empty line = paragraph break between chunks (see split_sentences)
                        tokenized.push_str("\n\n");
                    }
                    Err(e) => {
                        return GetTokenizedResult::Err(anyhow::anyhow!(
                            "hfst-tokenize failed on {}: {e}",
                            path.display()
                        ))
                    }
                }
            }
            match std::fs::write(&cached_tokenized_path, tokenized.as_bytes()) {
                Ok(_) => {}
                Err(e) => return GetTokenizedResult::Err(anyhow::anyhow!(e)),
            };
            return GetTokenizedResult::Ok(tokenized);
        }
        Err(e) => {
            GetTokenizedResult::Err(anyhow::anyhow!("could not read cached tokenized file: {e}"))
        }
    }
}

fn process_files(gut_root: gtcorpusutil::Root, cache_policy: CachePolicy) -> anyhow::Result<()> {
    let files = find_files(gut_root)?;
    if files.is_empty() {
        println!("No corpus .xml files found in the hist/ directory.");
        return Ok(());
    }
    println!("{} files", files.len());

    print!("Gathering texts ...");
    let texts = gather_texts(files, cache_policy)?;
    println!("done");

    // one vrt file per orthography, and for each category, so e.g.
    // friis_admin, orth2_bible, etc
    //
    // each of those will look like a sequence of <text>,
    // and <text> has attributes for the given document, and each sentence
    // in that text has an incrementing number
    //
    let cwd = std::env::current_dir()?;
    let wd = cwd.join("generated");
    let vrts_dir = wd.join("vrts");
    clean_dir(&wd)?;
    std::fs::create_dir_all(&vrts_dir)?;

    println!("Writing .vrt files ...");
    for ((orth, cat), texts) in texts.iter() {
        let corpus_name = format!("{orth}_{cat}");
        let filepath = vrts_dir.join(format!("{corpus_name}.vrt"));
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&filepath)?;
        for (text, tokenized) in texts {
            write_vrt_file(&mut f, text, &tokenized)?;
        }
        println!("wrote {}", filepath.strip_prefix(&cwd).unwrap().display());
    }

    // A temporary workdir directory is created, at <cwd>/generated/,
    // it contains:
    // /vrt - dir for .vrt files
    // /vrt/<orth>_<category>.vrt - one .vrt file per (orth, cat) pair
    // /cwb - dir for cwb files
    // /cwb/registry/<orth>_<category>  cwb reg file
    // /cwb/data/<orth>_<category>/<...cwb data files>

    // (orthography, category) -> [(text, tokenized)]

    let reg_dir = wd.join("cwb/registry");
    std::fs::create_dir_all(&reg_dir)?;

    println!("cwb-encode ...");
    for ((orth, cat), _texts) in texts.iter() {
        let corpus_name = format!("{orth}_{cat}");
        let vrt = vrts_dir.join(format!("{corpus_name}.vrt"));
        let data_dir = wd.join("cwb/data").join(&corpus_name);
        let reg_file_path = reg_dir.join(&corpus_name);
        std::fs::create_dir_all(&data_dir)?;

        print!("...{corpus_name}...");
        cwb_encode(&reg_file_path, &data_dir, &vrt)?;
        println!("done");
    }

    // TODO: writing cwb/data/<corpus>/.info files, for cwb-makeall
    // to not complain
    println!("Writing .info files ...");
    for ((orth, cat), _texts) in texts.iter() {
        let corpus_name = format!("{orth}_{cat}");
        let filepath = wd.join("cwb/data").join(&corpus_name).join(".info");
        let f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&filepath)?;
        drop(f);
        println!("wrote {}", filepath.strip_prefix(&cwd).unwrap().display());
    }

    println!("cwb-makeall ...");
    for ((orth, cat), _texts) in texts.iter() {
        let corpus_name = format!("{orth}_{cat}");
        //let reg_file_path = reg_dir.join(&corpus_name);
        cwb_makeall(&reg_dir, &corpus_name)?;
    }

    println!("cwb-huffcode ...");
    for ((orth, cat), _texts) in texts.iter() {
        let corpus_name = format!("{orth}_{cat}");
        cwb_huffcode(&reg_dir, &corpus_name)?;
    }

    println!("cwb-compress-rdx ...");
    for ((orth, cat), _texts) in texts.iter() {
        let corpus_name = format!("{orth}_{cat}");
        cwb_compress_rdx(&reg_dir, &corpus_name)?;
    }

    println!("remove unneeded files ...");
    remove_unneeded_files(&wd)?;

    Ok(())
}

/// Write out the full .vrt string from a Text and the tokenized text
fn write_vrt_file(
    mut f: &mut std::fs::File,
    text: &Text,
    tokenized: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    write!(f, "<text ")?;
    text.write_text_attributes(&mut f)?;
    writeln!(f, ">")?;
    let mut sentence_id = 1;
    for sentence in split_sentences(&tokenized) {
        writeln!(f, "<sentence id=\"{}\">", sentence_id)?;
        for word in sentence {
            writeln!(f, "{}", word.replace("<", "&lt;"))?;
        }
        writeln!(f, "</sentence>")?;
        sentence_id += 1;
    }
    writeln!(f, "</text>")?;
    Ok(())
}

fn clean_tokenized(gut_root: gtcorpusutil::Root) -> anyhow::Result<()> {
    let sme = gut_root
        .corpora()
        .find(|corpus| is_desired_corpus(&corpus.corpus_name))
        .ok_or(anyhow::anyhow!("sme open corpus not found"))?
        .into_converted()
        .to_path_buf();
    println!("{}", sme.display());

    fn is_tokenized_file(entry: &walkdir::DirEntry) -> bool {
        entry
            .path()
            .extension()
            .is_some_and(|ext| ext.as_encoded_bytes().ends_with(b"tokenized"))
    }

    let files = walkdir::WalkDir::new(sme)
        .into_iter()
        .filter_map(|maybe_entry| maybe_entry.ok())
        .filter(is_tokenized_file)
        .map(|entry| entry.into_path());

    let mut i = 0;
    let mut errors = 0;
    for file in files {
        if let Err(e) = std::fs::remove_file(&file) {
            errors += 1;
            eprintln!("warn: could not remove file {}: {e}", file.display());
        } else {
            i += 1;
        }
    }

    println!("deleted {i} files");
    if errors > 0 {
        println!("warn: there were {errors} errors");
    }
    println!("done");
    Ok(())
}

fn remove_unneeded_files(path: &Path) -> std::io::Result<()> {
    let it = walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let f = e.file_name().to_str().unwrap();
            f.ends_with(".rev") || f.ends_with(".rdx")
                || f.ends_with(".corpus")
        });

    for e in it {
        std::fs::remove_file(e.path())?;
    }
    Ok(())
}

/// The text of each `<p>` in the body, in document order, with whitespace
/// collapsed. Includes text inside `<span>`s etc. All text in the hist/ bodies
/// is inside `<p>`s (which may be inside `<section>`s).
///
/// The paragraphs are kept apart, so they can be given to hfst-tokenize
/// separated by blank lines. That keeps its memory use in check, and
/// marks paragraph boundaries (previously, paragraphs were glued together
/// without even a space, merging the last and first word of adjacent
/// paragraphs, and text after a `</span>` was lost).
fn body_paragraphs(body: &roxmltree::Node) -> Vec<String> {
    body.descendants()
        .filter(|n| n.has_tag_name("p"))
        // a <p> nested in another <p> is covered by the outer one
        .filter(|p| !p.ancestors().skip(1).any(|a| a.has_tag_name("p")))
        .map(|p| {
            let text: String = p.descendants().filter_map(|n| n.is_text().then(|| n.text()).flatten()).collect();
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn main() -> anyhow::Result<()> {
    let crate::cli::Cli {
        command,
        corpora_root,
        cache_policy,
    } = crate::cli::Cli::parse();

    let root = if let Some(root) = corpora_root {
        gtcorpusutil::Root::new(root)
    } else {
        gtcorpusutil::Root::from_gut_config()?
    };

    match command {
        None => process_files(root, cache_policy),
        Some(crate::cli::Commands::CleanTokenized) => clean_tokenized(root),
    }
}

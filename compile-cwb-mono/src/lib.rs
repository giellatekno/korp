pub mod cwb;

use std::iter::zip;
use std::path::Path;

use gtcorpusutil::{Category, Corpus, KorpMonoDocument, KorpMonoFilePath};
use rayon::prelude::*;

/// Make sure the directory at `path` is an empty directory.
/// Will `mkdir -p DIR` if not it doesn't exist. Will `rm -rf` the directory
/// if it is a directory. Will return error in any other case.
fn clean_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(&path) {
        Ok(()) => std::fs::create_dir_all(&path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir_all(&path),
        Err(e) => Err(e),
    }
}

/// Get todays date on this system as a string with format "YYYYMMDD",
/// by shelling out to `date "+%Y%m%d`. Nice to keep dependencies low.
fn today() -> std::io::Result<String> {
    let mut output = std::process::Command::new("date")
        .arg("+%Y%m%d")
        .output()?
        .stdout;
    output.retain(|byte| matches!(byte, b'0'..=b'9'));
    Ok(String::from_utf8(output).expect("`date \"+%Y%m%d\"` outputs utf8"))
}

fn corpus_name(lang: &str, category: Category, date: &str) -> String {
    format!("{lang}_{}_{date}", category.as_str())
}

#[derive(Debug, thiserror::Error)]
pub enum CompileCwbMonoError {
    #[error("error getting current working directory: {0}")]
    Cwd(std::io::Error),

    #[error("no corpora found for language '{0}'")]
    NoCorpora(String),

    #[error("No analysed files found")]
    NoFiles,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("korp mono document error: {0}")]
    KorpMono(#[from] gtcorpusutil::NewKorpMonoDocumentError),
}

/// Compile analysed files in a corpora for the given language
/// into a Corpus WorkBench registry file and corresponding
/// `data/` directory.
///
/// Assumes that there is one directory on the filesystem
/// which contains the corpora directories to process. This
/// is known as the root, and for users of `gut` this would
/// be `<gut_root_directory>/giellalt`. If `gut` is installed
/// on the system, the [gtcorpusutil::Root::from_gut_config]
/// function will find that directory.
///
/// However, `gut` doesn't have to be installed on the system,
/// since you can also get a [gtcorpusutil::Root] using the
/// [gtcorpusutil::Root::new] function, to use any [PathBuf]
/// as the root.
///
/// What is important, is that the root directory must contain
/// directories with names on the form `corpus-LANG[-x-closed]`,
/// as these are the directories this function will read from.
///
/// A common method is to obtain a [gtcorpusutil::Root] directory,
/// is by first trying `from_gut_config`, and if that fails,
/// require the user to explicitly give the root directory to
/// use as input to the program.
pub fn compile_cwb_mono(
    corpus_root: gtcorpusutil::Root,
    lang: &str,
) -> Result<(), CompileCwbMonoError> {
    print!("[1/8] Finding korp_mono/ files...");
    let files = find_korp_mono_files(&corpus_root, lang);
    let total_files: u64 = files
        .iter()
        .map(|(_category, files)| files.len() as u64)
        .sum();
    println!("{total_files} files");

    // command that I run:
    // $ compile-cwb-mono sme       - takes all sme
    // $ compile-cwb-mono sme --skip-closed --skip-open   - ignore closed/open
    // $ compile-cwb-mono sme admin         - only create the admin corpus
    // create directory 'compiled_mono'

    if files.is_empty() {
        return Err(CompileCwbMonoError::NoFiles);
    }

    let cwd = std::env::current_dir().map_err(|e| CompileCwbMonoError::Cwd(e))?;
    let compiled_mono_dir = cwd.join("compiled_mono");
    let vrts_dir = compiled_mono_dir.join("vrts");
    clean_dir(&compiled_mono_dir)?;
    std::fs::create_dir_all(&vrts_dir)?;
    let date = today()?;

    println!("[2/8] Merging xml files in korp_mono/<category> to one big .vrt file");
    merge_vrts(&files, lang, date.as_str(), vrts_dir.as_path())?;

    println!("[3/8] cwb-encode");
    cwb_encode(&compiled_mono_dir, lang, &vrts_dir, &date, &files)?;

    println!("[4/8] write compiled_mono/data/<corpus>/.info files");
    let data_dir = compiled_mono_dir.join("cwb/data");
    write_info_files(&files, &data_dir, lang, &date)?;

    println!("[5/8] cwb-makeall");
    let registry_dir = compiled_mono_dir.join("cwb/registry");
    cwb_makeall(&files, &registry_dir, lang, &date)?;

    println!("[6/8] cwb-huffcode");
    cwb_huffcode(&files, &registry_dir, lang, &date)?;

    println!("[7/8] cwb-compress-rdx");
    cwb_compress_rdx(&files, &registry_dir, lang, &date)?;

    println!("[8/8] cleanup (rm *.rev, *.rdx, and *.corpus files in cwb/data)");
    walkdir::WalkDir::new(&data_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_str().expect("all file names cwb creates have utf-8 names");
            name.ends_with(".rev") || name.ends_with(".rdx") || name.ends_with(".corpus")
        })
        .for_each(|e| {
            // ignore errors
            let _ = std::fs::remove_file(e.path());
      });

    Ok(())
}

pub fn find_korp_mono_files(
    root: &gtcorpusutil::Root,
    lang: &str,
) -> Vec<(Category, Vec<KorpMonoFilePath>)> {
    let corpora: Vec<Corpus> = root
        .corpora()
        .filter(|corpus| corpus.corpus_name.lang == lang)
        .collect();
    let mut hashmap = std::collections::HashMap::new();
    for corpus in corpora {
        for file in corpus.into_korp_mono().files() {
            let Ok(category) = file.category() else {
                // File resides in an directory that is not a known category, skip it
                continue;
            };
            hashmap.entry(category).or_insert(vec![]).push(file);
        }
    }
    hashmap.drain().collect()
}

fn merge_vrts(
    files: &Vec<(Category, Vec<KorpMonoFilePath>)>,
    lang: &str,
    date: &str,
    vrts_dir: &Path,
) -> Result<(), CompileCwbMonoError> {
    let pb_style =
        indicatif::ProgressStyle::with_template("{prefix:9} {wide_bar} {pos:>6}/{len:>6} {msg}")
            .unwrap();

    let mp = indicatif::MultiProgress::new();
    let mut pbs = vec![];

    // give each file it's own progressbar handle
    let files: Vec<(&Category, &Vec<KorpMonoFilePath>, indicatif::ProgressBar)> = files
        .into_iter()
        .map(|(category, files)| {
                let pb = mp.add(indicatif::ProgressBar::new(files.len() as u64));
                pb.set_style(pb_style.clone());
                pb.set_prefix(category.as_str());
                pbs.push(pb.clone());
                (category, files, pb)
            }
        )
        .collect();

    files
        .into_par_iter()
        .map(
            move |(category, files, pb)| -> Result<(), CompileCwbMonoError> {
                let name = corpus_name(lang, *category, &date);
                let vrt_filename = format!("{name}.vrt");
                let vrt_filepath = vrts_dir.join(vrt_filename);
                let f = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&vrt_filepath)?;
                gather_vrts_for_category(f, files, Some(pb))?;
                Ok(())
            },
        )
        .collect::<Result<(), CompileCwbMonoError>>()?;

    mp.clear().unwrap();

    Ok(())
}

fn gather_vrts_for_category(
    mut output_file: std::fs::File,
    korpmono_files: &Vec<KorpMonoFilePath>,
    progress_bar: Option<indicatif::ProgressBar>,
) -> Result<(), CompileCwbMonoError> {
    use std::io::Write;

    for (text_id, korp_mono_file) in zip(1.., korpmono_files) {
        let file_contents = korp_mono_file.read_to_string()?;
        let korp_mono_doc = KorpMonoDocument::new(&file_contents)?;
        let attrs = korp_mono_doc.attributes();

        writeln!(
            output_file,
            "<text id=\"{}\" title=\"{}\" lang=\"{}\" orig_lang=\"{}\" first_name=\"{}\" last_name=\"{}\" nationality=\"{}\" gt_domain=\"{}\" date=\"{}\" datefrom=\"{}\" dateto=\"{}\" timefrom=\"{}\" timeto=\"{}\">",
            text_id,
            attrs.title.unwrap_or(""),
            attrs.lang.unwrap_or(""),
            attrs.orig_lang.unwrap_or(""),
            attrs.first_name.unwrap_or(""),
            attrs.last_name.unwrap_or(""),
            attrs.nationality.unwrap_or(""),
            attrs.gt_domain.unwrap_or(""),
            attrs.date.unwrap_or(""),
            attrs.datefrom.unwrap_or(""),
            attrs.dateto.unwrap_or(""),
            attrs.timefrom.unwrap_or(""),
            attrs.timeto.unwrap_or(""),
        )?;

        for sentence in korp_mono_doc.sentences() {
            let id = match sentence.id {
                Some(id) => id,
                None => {
                    println!(
                        "missing id in file {}",
                        korp_mono_file.to_path_buf().display()
                    );
                    -1
                }
            };
            writeln!(
                output_file,
                "<sentence id=\"{}\">\n{}\n</sentence>",
                id, sentence.content
            )?;
        }
        writeln!(output_file, "</text>")?;
        if let Some(ref pb) = progress_bar {
            pb.inc(1);
        }
    }

    if let Some(ref pb) = progress_bar {
        pb.finish();
    }

    Ok(())
}

fn cwb_encode(
    compiled_mono_dir: &Path,
    lang: &str,
    vrts_dir: &Path,
    date: &str,
    files: &Vec<(Category, Vec<KorpMonoFilePath>)>,
) -> Result<(), CompileCwbMonoError> {
    let registry_dir = compiled_mono_dir.join("cwb/registry");
    let data_dir = compiled_mono_dir.join("cwb/data");
    std::fs::create_dir_all(&registry_dir)?;
    std::fs::create_dir_all(&data_dir)?;

    let pb_style = indicatif::ProgressStyle::with_template("{prefix:9} {spinner} {dur} {msg}").unwrap();
    let mp = indicatif::MultiProgress::new();
    let mut pbs = vec![];
    let files: Vec<(&Category, &Vec<KorpMonoFilePath>, indicatif::ProgressBar)> = files
        .into_iter()
        .map(|(category, files)| {
            let pb = mp.add(indicatif::ProgressBar::no_length());
            pb.set_style(pb_style.clone());
            pb.set_prefix(category.as_str());
            pbs.push(pb.clone());
            (category, files, pb)
        })
        .collect();

    files.into_par_iter().map(|(category, _first_item, pb)| {
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        let name = corpus_name(lang, *category, date);
        let vrt_path = vrts_dir.join(format!("{name}.vrt"));
        let reg_file_path = compiled_mono_dir.join("cwb/registry").join(&name);
        let data_dir_path = compiled_mono_dir.join("cwb/data").join(&name);
        std::fs::create_dir_all(&data_dir_path)?;
        let output = cwb::encode(&reg_file_path, &data_dir_path, &vrt_path)?;
        if !output.status.success() {
            println!("cwb-encode FAILED");
            let stdout = String::from_utf8_lossy_owned(output.stdout);
            let stderr = String::from_utf8_lossy_owned(output.stderr);
            println!("===== stdout =====");
            println!("{stdout}");
            println!("===== stderr =====");
            println!("{stderr}");
            println!("==================");
        }
        pb.finish();
        pb.disable_steady_tick();
        pb.set_message("done, waiting...");
        Ok(())
    })
    .collect::<Result<(), CompileCwbMonoError>>()?;

    mp.clear().unwrap();

    Ok(())
}

fn write_info_files(
    files: &Vec<(Category, Vec<KorpMonoFilePath>)>,
    data_dir: &Path,
    lang: &str,
    date: &str,
) -> std::io::Result<()> {
    for (category, _files) in files {
        let name = corpus_name(lang, *category, date);
        let dotinfo_file_path = data_dir.join(name).join(".info");
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(dotinfo_file_path)?;

        // use std::io::Write;
        // an example of what to write to this file
        // (see spraakbanken/korp-frontend main readme):
        // Sentences: 12345
        // Updated: 2019-11-30
        // FirstDate: 2001-01-16 00:00:00
        // LastDate: 2001-01-30 23:59:59
        <std::fs::File as std::io::Write>::write(&mut f, b"")?;

        // this should be sufficient for the file to be created, right?
        f.sync_all()?;
        drop(f);
    }
    Ok(())
}

fn cwb_makeall(
    files: &Vec<(Category, Vec<KorpMonoFilePath>)>,
    registry_dir: &Path,
    lang: &str,
    date: &str,
) -> std::io::Result<()> {
    for (category, _files) in files {
        let name = corpus_name(lang, *category, date);
        cwb::makeall(registry_dir, &name)?;
    }
    Ok(())
}

fn cwb_huffcode(
    files: &Vec<(Category, Vec<KorpMonoFilePath>)>,
    registry_dir: &Path,
    lang: &str,
    date: &str,
) -> std::io::Result<()> {
    for (category, _files) in files {
        let name = corpus_name(lang, *category, date);
        cwb::huffcode(registry_dir, &name)?;
    }
    Ok(())
}

fn cwb_compress_rdx(
    files: &Vec<(Category, Vec<KorpMonoFilePath>)>,
    registry_dir: &Path,
    lang: &str,
    date: &str,
) -> std::io::Result<()> {
    for (category, _files) in files {
        let name = corpus_name(lang, *category, date);
        cwb::compress_rdx(registry_dir, &name)?;
    }
    Ok(())
}

use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Call `cwb-encode` with path to registry file `reg_file`, data directory
/// `data_dir` and the path to the `.vrt` file in `vrt`.
///
/// Input: `vrt`, the path to the `.vrt` file of the corpus.
/// Output: `reg_file`, path to where to store the registry file.
/// Output: `data_dir`, path to where to store the data/ directory.
pub fn encode(reg_file: &Path, data_dir: &Path, vrt: &Path) -> std::io::Result<Output> {
    let child = Command::new("cwb-encode")
        //.arg("-x") // xml-aware (replace XML entities and ignore <!--
        //.arg("-s") // skip empty lines
        //.arg("-B") // skip leading blanks
        //.arg("-C") // (ignored in utf8 mode anyway)
        //.arg("-9") // discard XML tags for undecleared s-attributes
        .arg("-d")
        .arg(data_dir)
        .arg("-R")
        .arg(reg_file)
        .arg("-c")
        .arg("utf8")
        .arg("-f")
        .arg(vrt)
        .arg("-S")
        .arg("sentence:0+id") // +token_count
        .arg("-S")
        .arg("text:0+id+title+lang+orig_lang+gt_domain+first_name+last_name+nationality+date+datefrom+dateto+timefrom+timeto+sentence_count") // +token_count
        //.arg("-P")
        //.arg("-") // no default p-attribute (all must be declared with -P)
        //.arg("-P")
        //.arg("word")  // the word form, first column always
        .arg("-P")
        .arg("lemma")  // the lemma
        .arg("-P")
        .arg("pos")  // e.g. "N"
        .arg("-P")
        .arg("msd")  // "morpho-syntactic description(?)" - the analysis
        .arg("-P")
        .arg("num")  // e.g. "5" (if it's the 5th word in the sentence)
        .arg("-P")
        .arg("deprel")  // e.g. "-F←SUBJ"
        .arg("-P")
        .arg("ref")  // e.g. "6" (which word index in the sentence this word "goes with",
                     // syntactically speaking... or something like that)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = child.wait_with_output()?;
    Ok(output)
}

pub fn makeall(
    registry_dir: &Path,
    corpus_name: &str,
) -> std::io::Result<()> {
    let upper_corpus_name = corpus_name.to_uppercase();
    let child = std::process::Command::new("cwb-makeall")
        .arg("-D") // debug mode
        .arg("-V") // validate index after creating it
        .arg("-r") // registry dir
        .arg(registry_dir)
        .arg(upper_corpus_name)
        .output()?;
    if child.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&child.stderr);
        let reason = if let Some(code) = child.status.code() {
            format!("exit code: {code}")
        } else {
            format!("no exit code; terminated by signal")
        };
        let error = format!("cwb-makeall failed\n{reason}\nstderr:\n{stderr}");
        Err(std::io::Error::other(error))
    }
}

pub fn huffcode(
    registry_dir: &Path,
    corpus_name: &str,
) -> std::io::Result<()> {
    let upper_corpus_name = corpus_name.to_uppercase();
    let child = std::process::Command::new("cwb-huffcode")
        .arg("-r")
        .arg(registry_dir)
        .arg("-A") // compress all positional attributes
        .arg(upper_corpus_name)
        .output()?;
    if child.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&child.stderr);
        let reason = if let Some(code) = child.status.code() {
            format!("exit code: {code}")
        } else {
            format!("no exit code; terminated by signal")
        };
        let error = format!("cwb-huffcode failed\n{reason}\nstderr:\n{stderr}");
        Err(std::io::Error::other(error))
    }
}

pub fn compress_rdx(
    registry_dir: &Path,
    corpus_name: &str,
) -> std::io::Result<()> {
    let upper_corpus_name = corpus_name.to_uppercase();
    let child = std::process::Command::new("cwb-compress-rdx")
        .arg("-r")
        .arg(registry_dir)
        .arg("-A") // compress all positional attributes
        .arg(upper_corpus_name)
        .output()?;
    if child.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&child.stderr);
        let reason = if let Some(code) = child.status.code() {
            format!("exit code: {code}")
        } else {
            format!("no exit code; terminated by signal")
        };
        let error = format!("cwb-compress-rdx failed\n{reason}\nstderr:\n{stderr}");
        Err(std::io::Error::other(error))
    }
}

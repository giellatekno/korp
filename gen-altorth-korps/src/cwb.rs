use std::path::Path;

pub fn cwb_encode(
    reg_file: &Path,
    data_dir: &Path,
    vrt: &Path,
) -> std::io::Result<()> {
    let mut child = std::process::Command::new("cwb-encode")
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
        .spawn()?;
    child.wait()?;
    Ok(())
}

pub fn cwb_makeall(
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

pub fn cwb_huffcode(
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

pub fn cwb_compress_rdx(
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


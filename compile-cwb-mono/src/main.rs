use compile_cwb_mono::*;

#[derive(Debug, thiserror::Error)]
#[error("fatal: {0}")]
struct ErrorMsg(String);
macro_rules! fail {
    ($msg:expr) => {
        return Err(Box::new(ErrorMsg($msg.to_string())));
    };
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(lang) = std::env::args().skip(1).next() else {
        fail!("no lang given, give lang as first arg");
    };
    let root = gtcorpusutil::Root::from_gut_config()?;

    let t0 = std::time::Instant::now();
    println!(
        "compile-cwb-mono lang={} root={}...",
        lang,
        root.as_path().display()
    );

    compile_cwb_mono(root, &lang)?;

    let dur = t0.elapsed();
    println!("all done (in {dur:?})");
    Ok(())
}

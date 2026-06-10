use std::path::Path;

fn main() {
    let mut files = Vec::new();
    recursion(&mut files, "silk/interface").expect("list silk/interface sources");
    recursion(&mut files, "silk/src").expect("list silk/src sources");
    println!("cargo:rustc-link-lib=static=silk");
    println!("cargo:rerun-if-changed=silk");
    cc::Build::new()
        .includes(["silk/src", "silk/interface"])
        .files(files)
        .compile("silk");
}

fn recursion<P: AsRef<Path>>(v: &mut Vec<String>, dir: P) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            recursion(v, path)?;
        } else if path.extension().is_some_and(|ext| ext == "c") {
            v.push(path.into_os_string().into_string().unwrap());
        }
    }
    Ok(())
}

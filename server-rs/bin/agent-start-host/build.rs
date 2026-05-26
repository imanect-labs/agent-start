use std::fs;
use std::path::PathBuf;

fn main() {
    // rust-embed needs the source folder to exist at compile time even when
    // the front-end bundle hasn't been built yet (fresh checkout, CI step
    // before `vp build`, doc-only contributors). Seed a placeholder so the
    // host always compiles; a real `vp build` overwrites it.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dist = manifest_dir
        .join("..")
        .join("..")
        .join("..")
        .join("front")
        .join("dist");
    if !dist.join("index.html").exists() {
        let _ = fs::create_dir_all(&dist);
        let _ = fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=\"utf-8\"><title>agent-start</title>\
             <body><p>Frontend bundle is missing. Run <code>vp build</code> in <code>front/</code> \
             (or pass <code>--frontend-dist</code> to point at a built bundle).</p></body>",
        );
    }
    println!("cargo:rerun-if-changed=../../../front/dist");
}

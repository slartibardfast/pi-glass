use std::env;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

fn brotli_compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    brotli::BrotliCompress(
        &mut Cursor::new(data),
        &mut out,
        &brotli::enc::BrotliEncoderParams::default(), // quality=11
    )
    .expect("brotli compression failed");
    out
}

fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(data).expect("gzip write failed");
    enc.finish().expect("gzip finish failed")
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Combined CSS — compressed once at build time, both encodings
    let tokens_css = fs::read_to_string("web/dist/tokens.css").expect("missing tokens.css");
    let app_css    = fs::read_to_string("src/app.css").expect("missing app.css");
    let combined   = format!("{tokens_css}\n{app_css}");
    let css_bytes  = combined.as_bytes();
    fs::write(out_dir.join("combined.css.br"), brotli_compress(css_bytes)).expect("write css.br");
    fs::write(out_dir.join("combined.css.gz"), gzip_compress(css_bytes)).expect("write css.gz");

    // JS — both encodings
    let js = fs::read("src/app.js").expect("missing app.js");
    fs::write(out_dir.join("app.js.br"), brotli_compress(&js)).expect("write js.br");
    fs::write(out_dir.join("app.js.gz"), gzip_compress(&js)).expect("write js.gz");

    println!("cargo:rerun-if-changed=web/dist/tokens.css");
    println!("cargo:rerun-if-changed=src/app.css");
    println!("cargo:rerun-if-changed=src/app.js");
}

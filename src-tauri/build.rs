use std::path::PathBuf;

const MUPDF_LIBS: &[&str] = &[
    "libmupdf",
    "libthirdparty",
    "libresources",
    "libpkcs7",
    "libextract",
    "libharfbuzz",
    "libtesseract",
    "libleptonica",
    "libmubarcode",
    "libzxing",
];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mupdf = manifest.join("..").join("native").join("mupdf");
    let libdir = mupdf.join("platform").join("win32").join("x64").join("Release");

    if !libdir.join("libmupdf.lib").exists() {
        panic!(
            "MuPDF libraries not found in {}.\nBuild them first: powershell -File native/build-mupdf.ps1",
            libdir.display()
        );
    }

    cc::Build::new()
        .file("native/folio.c")
        .include(mupdf.join("include"))
        .define("NDEBUG", None)
        .opt_level(2)
        .warnings(false)
        .compile("folio_native");

    println!("cargo:rustc-link-search=native={}", libdir.display());
    for lib in MUPDF_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    for sys in ["advapi32", "user32", "gdi32", "shell32", "ws2_32"] {
        println!("cargo:rustc-link-lib=dylib={sys}");
    }
    // The frontend is embedded at compile time; rebuild when it changes.
    println!("cargo:rerun-if-changed=../dist");
    println!("cargo:rerun-if-changed=native/folio.c");
    println!("cargo:rerun-if-changed=native/folio.h");
    println!("cargo:rerun-if-changed={}", libdir.join("libmupdf.lib").display());

    tauri_build::build();
}

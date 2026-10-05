use std::path::{Path, PathBuf};

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

/// libarchive's sources for reading RAR archives from memory (native/libarchive holds just these).
const LIBARCHIVE: &[&str] = &[
    "archive_acl.c",
    "archive_blake2s_ref.c",
    "archive_blake2sp_ref.c",
    "archive_check_magic.c",
    "archive_entry.c",
    "archive_entry_sparse.c",
    "archive_entry_xattr.c",
    "archive_ppmd7.c",
    "archive_random.c",
    "archive_rb.c",
    "archive_read.c",
    "archive_read_open_memory.c",
    "archive_read_support_format_rar.c",
    "archive_read_support_format_rar5.c",
    "archive_string.c",
    "archive_string_sprintf.c",
    "archive_time.c",
    "archive_util.c",
    "archive_virtual.c",
];

/// Comic books packed with RAR (CBR): libarchive's RAR readers and the MuPDF archive handler on them.
fn rar(mupdf: &Path, windows: bool) {
    let dir = Path::new("native/libarchive");
    let mut b = cc::Build::new();
    for f in LIBARCHIVE {
        b.file(dir.join(f));
    }
    if windows {
        b.file(dir.join("archive_windows.c"));
    }
    b.file("native/cbr.c")
        .include(mupdf.join("include"))
        .include(dir)
        // The hand-written configuration (no configure step), found next to archive_platform.h.
        .define("PLATFORM_CONFIG_H", Some("\"paginaut_config.h\""))
        .define("LIBARCHIVE_STATIC", None)
        .define("NDEBUG", None)
        .opt_level(2)
        .warnings(false)
        .compile("paginaut_rar");
    println!("cargo:rerun-if-changed=native/cbr.c");
    println!("cargo:rerun-if-changed=native/libarchive");
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mupdf = manifest.join("..").join("native").join("mupdf");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        android(&mupdf);
    } else {
        windows(&manifest, &mupdf);
    }
    // The frontend is embedded at compile time; rebuild when it changes.
    println!("cargo:rerun-if-changed=../dist");
    println!("cargo:rerun-if-changed=native/paginaut.c");
    println!("cargo:rerun-if-changed=native/paginaut.h");
    println!("cargo:rerun-if-changed=native/ocr.h");
}

/// The Windows app: MuPDF's MSBuild libraries, with Tesseract for OCR.
fn windows(manifest: &Path, mupdf: &Path) {
    let libdir = mupdf.join("platform").join("win32").join("x64").join("Release");

    if !libdir.join("libmupdf.lib").exists() {
        panic!(
            "MuPDF libraries not found in {}.\nBuild them first: powershell -File native/build-mupdf.ps1",
            libdir.display()
        );
    }

    let tessdata = manifest.join("..").join("native").join("tessdata");
    if !tessdata.join("eng.traineddata").exists() {
        panic!("OCR models not found in {}.\nFetch them first: powershell -File native/get-tessdata.ps1", tessdata.display());
    }

    cc::Build::new()
        .file("native/paginaut.c")
        .include(mupdf.join("include"))
        .define("NDEBUG", None)
        .opt_level(2)
        .warnings(false)
        .compile("paginaut_native");
    rar(mupdf, true);

    // Tesseract bridge, against the Tesseract and Leptonica built into MuPDF's
    // libraries (same defines, so the classes match).
    let third = mupdf.join("thirdparty");
    cc::Build::new()
        .cpp(true)
        .file("native/ocr.cpp")
        .include(third.join("tesseract").join("include"))
        .include(mupdf.join("scripts").join("tesseract"))
        .include(third.join("leptonica").join("src"))
        .define("NDEBUG", None)
        .define("HAVE_LEPTONICA", None)
        .define("HAVE_TESSERACT", None)
        .define("DISABLED_LEGACY_ENGINE", None)
        .define("GRAPHICS_DISABLED", None)
        .define("TESSERACT_IMAGEDATA_AS_PIX", None)
        .define("TESSERACT_DISABLE_DEBUG_FONTS", None)
        .flag_if_supported("/std:c++17")
        .flag_if_supported("/EHsc")
        .opt_level(2)
        .warnings(false)
        .compile("paginaut_ocr");

    println!("cargo:rustc-link-search=native={}", libdir.display());
    for lib in MUPDF_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    for sys in ["advapi32", "user32", "gdi32", "shell32", "ws2_32", "bcrypt"] {
        println!("cargo:rustc-link-lib=dylib={sys}");
    }
    println!("cargo:rerun-if-changed=native/ocr.cpp");
    println!("cargo:rerun-if-changed={}", libdir.join("libmupdf.lib").display());

    // Our own Windows manifest: Tauri's, plus Windows 10/11 as supported (see windows/app.manifest).
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows/app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("failed to run the Tauri build script");
}

/// The Android viewer: MuPDF built by native/build-mupdf-android.sh for this ABI, without OCR.
fn android(mupdf: &Path) {
    let abi = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64-v8a",
        "arm" => "armeabi-v7a",
        "x86" => "x86",
        "x86_64" => "x86_64",
        other => panic!("no Android ABI for {other}"),
    };
    let libdir = mupdf.join("build").join(format!("android-{abi}"));
    if !libdir.join("libmupdf.a").exists() {
        panic!("MuPDF for Android not found in {}.\nBuild it first: bash native/build-mupdf-android.sh {abi}", libdir.display());
    }
    cc::Build::new()
        .file("native/paginaut.c")
        .file("native/ocr_none.c")
        .include(mupdf.join("include"))
        .define("NDEBUG", None)
        .opt_level(2)
        .warnings(false)
        .compile("paginaut_native");
    rar(mupdf, false);
    println!("cargo:rustc-link-search=native={}", libdir.display());
    println!("cargo:rustc-link-lib=static=mupdf");
    println!("cargo:rustc-link-lib=static=mupdf-third");
    // HarfBuzz is C++.
    println!("cargo:rustc-link-lib=c++_static");
    println!("cargo:rustc-link-lib=c++abi");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rerun-if-changed=native/ocr_none.c");
    println!("cargo:rerun-if-changed={}", libdir.join("libmupdf.a").display());
    tauri_build::build();
}

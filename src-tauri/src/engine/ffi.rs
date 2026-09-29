//! Raw bindings to `native/paginaut.c`. Everything here is `unsafe`; the safe
//! layer lives in `engine::mupdf`.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_float, c_int, c_uchar, c_void};

#[repr(C)]
pub struct fo_ctx {
    _p: [u8; 0],
}
#[repr(C)]
pub struct fo_doc {
    _p: [u8; 0],
}
#[repr(C)]
pub struct fo_stamp {
    _p: [u8; 0],
}
#[repr(C)]
pub struct fo_page {
    _p: [u8; 0],
}
#[repr(C)]
pub struct fo_list {
    _p: [u8; 0],
}
#[repr(C)]
pub struct fo_text {
    _p: [u8; 0],
}
#[repr(C)]
pub struct fo_cookie {
    _p: [u8; 0],
}

extern "C" {
    pub fn fo_global_init(store_bytes: usize) -> c_int;
    pub fn fo_ctx_new() -> *mut fo_ctx;
    pub fn fo_ctx_new_private(store_bytes: usize) -> *mut fo_ctx;
    pub fn fo_ctx_drop(c: *mut fo_ctx);
    pub fn fo_error(c: *mut fo_ctx) -> *const c_char;
    pub fn fo_store_shrink(c: *mut fo_ctx, percent: c_int);

    pub fn fo_cookie_new() -> *mut fo_cookie;
    pub fn fo_cookie_abort(k: *mut fo_cookie);
    pub fn fo_cookie_drop(k: *mut fo_cookie);

    pub fn fo_open(c: *mut fo_ctx, path: *const c_char) -> *mut fo_doc;
    pub fn fo_doc_drop(c: *mut fo_ctx, d: *mut fo_doc);
    pub fn fo_needs_password(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_authenticate(c: *mut fo_ctx, d: *mut fo_doc, password: *const c_char) -> c_int;
    pub fn fo_layout(c: *mut fo_ctx, d: *mut fo_doc, w: c_float, h: c_float, em: c_float) -> c_int;
    pub fn fo_is_reflowable(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_is_pdf(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_count_pages(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_metadata(c: *mut fo_ctx, d: *mut fo_doc, key: *const c_char, buf: *mut c_char, size: c_int) -> c_int;
    pub fn fo_outline_json(c: *mut fo_ctx, d: *mut fo_doc) -> *mut c_char;

    pub fn fo_page_load(c: *mut fo_ctx, d: *mut fo_doc, index: c_int) -> *mut fo_page;
    pub fn fo_page_drop(c: *mut fo_ctx, p: *mut fo_page);
    pub fn fo_page_bounds(c: *mut fo_ctx, p: *mut fo_page, out: *mut c_float) -> c_int;
    pub fn fo_page_label(c: *mut fo_ctx, p: *mut fo_page, buf: *mut c_char, size: c_int) -> c_int;
    pub fn fo_links_json(c: *mut fo_ctx, d: *mut fo_doc, p: *mut fo_page) -> *mut c_char;
    pub fn fo_list_new(c: *mut fo_ctx, p: *mut fo_page, extras: c_int) -> *mut fo_list;
    pub fn fo_list_drop(c: *mut fo_ctx, l: *mut fo_list);

    pub fn fo_render(
        c: *mut fo_ctx,
        l: *mut fo_list,
        bounds: *const c_float,
        scale: c_float,
        rotate: c_int,
        clip: *const c_int,
        out: *mut c_uchar,
        k: *mut fo_cookie,
    ) -> c_int;
    pub fn fo_render_size(bounds: *const c_float, scale: c_float, rotate: c_int, w: *mut c_int, h: *mut c_int);

    pub fn fo_text_new(c: *mut fo_ctx, l: *mut fo_list) -> *mut fo_text;
    pub fn fo_text_drop(c: *mut fo_ctx, t: *mut fo_text);
    pub fn fo_text_highlight(
        c: *mut fo_ctx, t: *mut fo_text, bounds: *const c_float,
        ax: c_float, ay: c_float, bx: c_float, by: c_float, out: *mut c_float, max: c_int,
    ) -> c_int;
    pub fn fo_text_snap(
        c: *mut fo_ctx, t: *mut fo_text, bounds: *const c_float,
        ax: c_float, ay: c_float, bx: c_float, by: c_float, mode: c_int, out: *mut c_float,
    ) -> c_int;
    pub fn fo_text_copy(
        c: *mut fo_ctx, t: *mut fo_text, bounds: *const c_float,
        ax: c_float, ay: c_float, bx: c_float, by: c_float,
    ) -> *mut c_char;
    pub fn fo_text_all(c: *mut fo_ctx, t: *mut fo_text) -> *mut c_char;
    pub fn fo_text_char_count(c: *mut fo_ctx, t: *mut fo_text) -> c_int;
    pub fn fo_text_search_json(c: *mut fo_ctx, t: *mut fo_text, bounds: *const c_float, needle: *const c_char) -> *mut c_char;
    pub fn fo_text_match_json(c: *mut fo_ctx, t: *mut fo_text, bounds: *const c_float, needle: *const c_char, flags: c_int, snippets: c_int) -> *mut c_char;

    pub fn fo_pdf_apply_page_edits(c: *mut fo_ctx, d: *mut fo_doc, rot: *const c_int, npages: c_int, order: *const c_int, count: c_int, file_pages: c_int) -> c_int;
    pub fn fo_pdf_can_save_incrementally(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_pdf_save(c: *mut fo_ctx, d: *mut fo_doc, path: *const c_char, incremental: c_int) -> c_int;
    pub fn fo_convert_to_pdf(c: *mut fo_ctx, d: *mut fo_doc, path: *const c_char, order: *const c_int, count: c_int, rot: *const c_int) -> c_int;
    pub fn fo_pdf_insert_pages(c: *mut fo_ctx, d: *mut fo_doc, src: *mut fo_doc, pages: *const c_int, count: c_int, fit: *const c_float) -> c_int;
    pub fn fo_pdf_new(c: *mut fo_ctx) -> *mut fo_doc;
    pub fn fo_pdf_outline_clear(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_pdf_outline_add(c: *mut fo_ctx, d: *mut fo_doc, parent: c_int, title: *const c_char, page: c_int, y: c_float) -> c_int;
    pub fn fo_pdf_page_label_ranges_json(c: *mut fo_ctx, d: *mut fo_doc) -> *mut c_char;
    pub fn fo_pdf_set_page_labels(c: *mut fo_ctx, d: *mut fo_doc, starts: *const c_int, styles: *const c_char, prefixes: *const c_char, firsts: *const c_int, count: c_int) -> c_int;
    pub fn fo_set_metadata(c: *mut fo_ctx, d: *mut fo_doc, key: *const c_char, value: *const c_char) -> c_int;
    pub fn fo_stamp_begin(c: *mut fo_ctx, p: *mut fo_page) -> *mut fo_stamp;
    pub fn fo_stamp_text(c: *mut fo_ctx, st: *mut fo_stamp, html: *const c_char, css: *const c_char, width: c_float, valign: c_int, m: *const c_float, opacity: c_float) -> c_int;
    pub fn fo_stamp_image(c: *mut fo_ctx, st: *mut fo_stamp, data: *const u8, len: usize, w: c_float, h: c_float, m: *const c_float, opacity: c_float) -> c_int;
    pub fn fo_stamp_end(c: *mut fo_ctx, st: *mut fo_stamp, kind: *const c_char, behind: c_int) -> c_int;
    pub fn fo_stamp_drop(c: *mut fo_ctx, st: *mut fo_stamp);
    pub fn fo_stamp_remove(c: *mut fo_ctx, p: *mut fo_page, kind: *const c_char) -> c_int;
    pub fn fo_page_content_box(c: *mut fo_ctx, p: *mut fo_page, out: *mut c_float) -> c_int;
    pub fn fo_pdf_page_size(c: *mut fo_ctx, p: *mut fo_page, crop: *const c_float, size: *const c_float, fit: c_int, ax: c_float, ay: c_float, remove: c_int, fill: c_int, rgb: *const c_float) -> c_int;
    pub fn fo_pdf_split_by_size(c: *mut fo_ctx, d: *mut fo_doc, order: *const c_int, count: c_int, max_bytes: f64, starts: *mut c_int) -> c_int;
    pub fn fo_pdf_outline_group(c: *mut fo_ctx, d: *mut fo_doc, src: *mut fo_doc, title: *const c_char, base: c_int, pages: *const c_int, count: c_int) -> c_int;
    pub fn fo_pdf_insert_blank(c: *mut fo_ctx, d: *mut fo_doc, w: c_float, h: c_float, count: c_int) -> c_int;

    pub fn fo_image_rects_json(c: *mut fo_ctx, l: *mut fo_list, bounds: *const c_float) -> *mut c_char;
    pub fn fo_reflow_json(c: *mut fo_ctx, l: *mut fo_list, bounds: *const c_float) -> *mut c_char;

    pub fn fo_pdf_forms_init(c: *mut fo_ctx, d: *mut fo_doc, xfa: *mut c_int) -> c_int;
    pub fn fo_widgets_json(c: *mut fo_ctx, p: *mut fo_page, bounds: *const c_float) -> *mut c_char;
    pub fn fo_widget_set(c: *mut fo_ctx, p: *mut fo_page, num: c_int, value: *const c_char) -> c_int;
    pub fn fo_widget_toggle(c: *mut fo_ctx, p: *mut fo_page, num: c_int) -> c_int;
    pub fn fo_pdf_undo(c: *mut fo_ctx, d: *mut fo_doc, redo: c_int) -> c_int;
    pub fn fo_pdf_has_changes(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_pdf_reset_form(c: *mut fo_ctx, d: *mut fo_doc) -> c_int;
    pub fn fo_pdf_bake(c: *mut fo_ctx, d: *mut fo_doc, annots: c_int) -> c_int;

    pub fn fo_annots_json(c: *mut fo_ctx, p: *mut fo_page, bounds: *const c_float) -> *mut c_char;
    pub fn fo_doc_annots_json(c: *mut fo_ctx, d: *mut fo_doc) -> *mut c_char;
    pub fn fo_pdf_begin_op(c: *mut fo_ctx, d: *mut fo_doc, name: *const c_char);
    pub fn fo_pdf_end_op(c: *mut fo_ctx, d: *mut fo_doc);
    pub fn fo_pdf_abandon_op(c: *mut fo_ctx, d: *mut fo_doc);
    pub fn fo_annot_create(c: *mut fo_ctx, p: *mut fo_page, kind: *const c_char, author: *const c_char) -> c_int;
    pub fn fo_annot_delete(c: *mut fo_ctx, p: *mut fo_page, num: c_int) -> c_int;
    pub fn fo_annot_update(c: *mut fo_ctx, p: *mut fo_page, num: c_int) -> c_int;
    pub fn fo_annot_set_rect(c: *mut fo_ctx, p: *mut fo_page, num: c_int, r: *const c_float) -> c_int;
    pub fn fo_annot_set_color(c: *mut fo_ctx, p: *mut fo_page, num: c_int, n: c_int, rgb: *const c_float) -> c_int;
    pub fn fo_annot_set_interior(c: *mut fo_ctx, p: *mut fo_page, num: c_int, n: c_int, rgb: *const c_float) -> c_int;
    pub fn fo_annot_set_opacity(c: *mut fo_ctx, p: *mut fo_page, num: c_int, o: c_float) -> c_int;
    pub fn fo_annot_set_border(c: *mut fo_ctx, p: *mut fo_page, num: c_int, w: c_float) -> c_int;
    pub fn fo_annot_set_contents(c: *mut fo_ctx, p: *mut fo_page, num: c_int, text: *const c_char) -> c_int;
    pub fn fo_annot_set_icon(c: *mut fo_ctx, p: *mut fo_page, num: c_int, name: *const c_char) -> c_int;
    pub fn fo_annot_set_quads(c: *mut fo_ctx, p: *mut fo_page, num: c_int, q: *const c_float, n: c_int) -> c_int;
    pub fn fo_annot_set_ink(c: *mut fo_ctx, p: *mut fo_page, num: c_int, counts: *const c_int, nstrokes: c_int, pts: *const c_float) -> c_int;
    pub fn fo_annot_set_line(c: *mut fo_ctx, p: *mut fo_page, num: c_int, ab: *const c_float, start: c_int, end: c_int) -> c_int;
    pub fn fo_annot_set_font(c: *mut fo_ctx, p: *mut fo_page, num: c_int, font: *const c_char, size: c_float, rgb: *const c_float) -> c_int;
    pub fn fo_annot_set_image(c: *mut fo_ctx, p: *mut fo_page, num: c_int, data: *const u8, len: usize) -> c_int;

    pub fn fo_sign_page(
        c: *mut fo_ctx,
        p: *mut fo_page,
        num: c_int,
        rect: *const c_float,
        cb: fo_sign_cb,
        user: *mut c_void,
        max: usize,
        dn: *const *const c_char,
        reason: *const c_char,
        location: *const c_char,
        img: *const u8,
        img_len: usize,
        flags: c_int,
        lock: c_int,
    ) -> c_int;
    pub fn fo_sigs_json(c: *mut fo_ctx, d: *mut fo_doc) -> *mut c_char;
    pub fn fo_sig_data(c: *mut fo_ctx, d: *mut fo_doc, i: c_int, data: *mut *mut u8, len: *mut usize, cms: *mut *mut u8, cms_len: *mut usize) -> c_int;

    pub fn fo_free(c: *mut fo_ctx, p: *mut c_void);

    pub fn fo_ocr_init() -> c_int;
    pub fn fo_ocr_ink(c: *mut fo_ctx, l: *mut fo_list, bounds: *const c_float) -> c_int;
    pub fn fo_ocr_page(c: *mut fo_ctx, l: *mut fo_list, bounds: *const c_float, dpi: c_float, t: *mut po_tess, k: *mut fo_cookie, aborted: *mut c_int) -> *mut c_char;
    pub fn fo_ocr_apply(c: *mut fo_ctx, p: *mut fo_page, content: *const c_char, len: usize, font_num: *mut c_int) -> c_int;
    pub fn fo_ocr_has_layer(c: *mut fo_ctx, p: *mut fo_page) -> c_int;

    pub fn po_tess_new(langs: *const c_char, files: *const c_char, err: *mut c_char, errlen: c_int) -> *mut po_tess;
    pub fn po_tess_drop(t: *mut po_tess);
}

#[repr(C)]
pub struct po_tess {
    _p: [u8; 0],
}

/// Makes the CMS blob for the bytes MuPDF hands over while saving a signed document.
pub type fo_sign_cb = extern "C" fn(user: *mut c_void, data: *const u8, len: usize, out: *mut u8, cap: usize) -> isize;

/* Folio's thin C layer over MuPDF.
 *
 * Rust never touches MuPDF structs directly: everything goes through these
 * functions, which wrap MuPDF's setjmp-based exceptions in fz_try/fz_catch
 * and report failure as a return code plus a per-context error message.
 *
 * Threading model (MuPDF's documented pattern):
 *   - one base context with locks; every thread uses its own clone
 *     (clones share the resource store, so decoded images are shared);
 *   - an fz_document is only touched by one thread at a time;
 *   - display lists are immutable once built and can be rendered by any
 *     thread in parallel.
 */
#ifndef FOLIO_H
#define FOLIO_H

#include <stddef.h>

typedef struct fo_ctx fo_ctx;
typedef struct fo_doc fo_doc;
typedef struct fo_page fo_page;
typedef struct fo_list fo_list;
typedef struct fo_text fo_text;
typedef struct fo_cookie fo_cookie;

int fo_global_init(size_t store_bytes);
fo_ctx *fo_ctx_new(void);
/* A fully independent context (own store, font engine and glyph cache, no locks).
 * Everything created with it must stay on the thread that created it. */
fo_ctx *fo_ctx_new_private(size_t store_bytes);
void fo_ctx_drop(fo_ctx *c);
const char *fo_error(fo_ctx *c);
void fo_store_shrink(fo_ctx *c, int percent);

fo_cookie *fo_cookie_new(void);
void fo_cookie_abort(fo_cookie *k);
void fo_cookie_drop(fo_cookie *k);

/* Documents */
fo_doc *fo_open(fo_ctx *c, const char *path_utf8);
void fo_doc_drop(fo_ctx *c, fo_doc *d);
int fo_needs_password(fo_ctx *c, fo_doc *d);
int fo_authenticate(fo_ctx *c, fo_doc *d, const char *password);
int fo_layout(fo_ctx *c, fo_doc *d, float w, float h, float em);
int fo_is_reflowable(fo_ctx *c, fo_doc *d);
int fo_is_pdf(fo_ctx *c, fo_doc *d);
int fo_count_pages(fo_ctx *c, fo_doc *d);
int fo_metadata(fo_ctx *c, fo_doc *d, const char *key, char *buf, int size);
/* JSON outline: [{"t":title,"p":page,"k":[children]}]. Free with fo_free. */
char *fo_outline_json(fo_ctx *c, fo_doc *d);

/* Pages */
fo_page *fo_page_load(fo_ctx *c, fo_doc *d, int index);
void fo_page_drop(fo_ctx *c, fo_page *p);
int fo_page_bounds(fo_ctx *c, fo_page *p, float out[4]);
int fo_page_label(fo_ctx *c, fo_page *p, char *buf, int size);
/* JSON links: [{"r":[x0,y0,x1,y1],"u":uri}|{"r":...,"p":page}], coords relative to page origin. */
char *fo_links_json(fo_ctx *c, fo_doc *d, fo_page *p);
/* extras: 0 page contents only, 1 with annotations and form fields,
 * 2 with annotations but without fillable fields (drawn by the app). */
fo_list *fo_list_new(fo_ctx *c, fo_page *p, int extras);
void fo_list_drop(fo_ctx *c, fo_list *l);

/* Render a display list into caller-provided RGBA memory.
 * bounds: the page bounds (pt); scale: device px per pt; rotate: degrees;
 * clip: device-pixel rect (x0,y0,x1,y1) relative to the page's top-left.
 * out must hold (x1-x0)*(y1-y0)*4 bytes. Returns 0, 1 if aborted, -1 on error. */
int fo_render(fo_ctx *c, fo_list *l, const float bounds[4], float scale, int rotate,
	const int clip[4], unsigned char *out, fo_cookie *k);
/* Size in device pixels of the whole page at scale/rotate. */
void fo_render_size(const float bounds[4], float scale, int rotate, int *w, int *h);

/* Structured text, built from a display list (thread-safe). */
fo_text *fo_text_new(fo_ctx *c, fo_list *l);
void fo_text_drop(fo_ctx *c, fo_text *t);
/* Quads relative to page origin written into out (8 floats each). Returns count. */
int fo_text_highlight(fo_ctx *c, fo_text *t, const float bounds[4], float ax, float ay, float bx, float by, float *out, int max);
/* mode: 0 chars, 1 words, 2 lines; writes one quad. */
int fo_text_snap(fo_ctx *c, fo_text *t, const float bounds[4], float ax, float ay, float bx, float by, int mode, float *out);
char *fo_text_copy(fo_ctx *c, fo_text *t, const float bounds[4], float ax, float ay, float bx, float by);
char *fo_text_all(fo_ctx *c, fo_text *t);
int fo_text_char_count(fo_ctx *c, fo_text *t);
/* JSON hits: [[quad8...],...] one array of quads per hit. */
char *fo_text_search_json(fo_ctx *c, fo_text *t, const float bounds[4], const char *needle);
/* Search with options. flags: 1 = match case, 2 = regular expression.
 * JSON: [{"q":[quad8...],"s":"text of the line"}...]; "s" only when snippets != 0. */
char *fo_text_match_json(fo_ctx *c, fo_text *t, const float bounds[4], const char *needle, int flags, int snippets);

/* Page edits and saving (PDF). rot[i] is extra rotation for source page i
 * (npages entries); order lists source pages in their new order (count entries). */
int fo_pdf_apply_page_edits(fo_ctx *c, fo_doc *d, const int *rot, int npages, const int *order, int count);
int fo_pdf_can_save_incrementally(fo_ctx *c, fo_doc *d);
/* incremental = 1: append changes to the document's own file (path must be that file). */
int fo_pdf_save(fo_ctx *c, fo_doc *d, const char *path, int incremental);
/* Writes any document as a new PDF with the given page order and extra rotations. */
int fo_convert_to_pdf(fo_ctx *c, fo_doc *d, const char *path, const int *order, int count, const int *rot);

/* JSON [[x0,y0,x1,y1],...]: where images are drawn on the page (relative to its origin). */
char *fo_image_rects_json(fo_ctx *c, fo_list *l, const float bounds[4]);

/* Page content for reflowing, in reading order (columns segmented, paragraphs
 * split, tables found). JSON {"w":,"h":,"b":[...]} where each block is
 *   {"t":"p","x":[bbox],"l":[{"x":[bbox],"j":1?,"s":[["text",size,style],...]}]}
 *   {"t":"i","x":[bbox]}   image        {"t":"tb","x":[bbox]}   table
 *   {"t":"v","x":[bbox]}   vector graphics (lines, fills)
 * style bits: 1 bold, 2 italic, 4 monospace, 8 superscript. Coordinates are
 * relative to the page origin. "j" marks a line joined to the next (dehyphenated). */
char *fo_reflow_json(fo_ctx *c, fo_list *l, const float bounds[4]);

/* Forms. fo_pdf_forms_init turns on the undo journal (and form scripts when
 * there are fields); returns the number of fields, -1 on error. */
int fo_pdf_forms_init(fo_ctx *c, fo_doc *d, int *xfa);
/* JSON [{"n":objnum,"t":type,"r":[bbox],"name","v","ff":flags,"ro","fs","q","max","fmt","on","opts":[[label,export]],"multi","signed"}] */
char *fo_widgets_json(fo_ctx *c, fo_page *p, const float bounds[4]);
int fo_widget_set(fo_ctx *c, fo_page *p, int num, const char *value);
int fo_widget_toggle(fo_ctx *c, fo_page *p, int num);
int fo_pdf_undo(fo_ctx *c, fo_doc *d, int redo);
int fo_pdf_has_changes(fo_ctx *c, fo_doc *d);
int fo_pdf_reset_form(fo_ctx *c, fo_doc *d);
int fo_pdf_bake(fo_ctx *c, fo_doc *d, int annots);

/* Annotations. Edits between fo_pdf_begin_op/fo_pdf_end_op are one undo step.
 * Coordinates are page space (as rendered), not relative to the page origin. */
char *fo_annots_json(fo_ctx *c, fo_page *p, const float bounds[4]);
char *fo_doc_annots_json(fo_ctx *c, fo_doc *d);
void fo_pdf_begin_op(fo_ctx *c, fo_doc *d, const char *name);
void fo_pdf_end_op(fo_ctx *c, fo_doc *d);
int fo_annot_create(fo_ctx *c, fo_page *p, const char *type, const char *author);
int fo_annot_delete(fo_ctx *c, fo_page *p, int num);
int fo_annot_update(fo_ctx *c, fo_page *p, int num);
int fo_annot_set_rect(fo_ctx *c, fo_page *p, int num, const float *r);
int fo_annot_set_color(fo_ctx *c, fo_page *p, int num, int n, const float *rgb);
int fo_annot_set_interior(fo_ctx *c, fo_page *p, int num, int n, const float *rgb);
int fo_annot_set_opacity(fo_ctx *c, fo_page *p, int num, float o);
int fo_annot_set_border(fo_ctx *c, fo_page *p, int num, float w);
int fo_annot_set_contents(fo_ctx *c, fo_page *p, int num, const char *text);
int fo_annot_set_icon(fo_ctx *c, fo_page *p, int num, const char *name);
int fo_annot_set_quads(fo_ctx *c, fo_page *p, int num, const float *q, int n);
int fo_annot_set_ink(fo_ctx *c, fo_page *p, int num, const int *counts, int nstrokes, const float *pts);
int fo_annot_set_line(fo_ctx *c, fo_page *p, int num, const float *ab, int start, int end);
int fo_annot_set_font(fo_ctx *c, fo_page *p, int num, const char *font, float size, const float *rgb);
int fo_annot_set_image(fo_ctx *c, fo_page *p, int num, const unsigned char *data, size_t len);

/* Digital signatures. fo_sign_page adds a signed signature field (rect all zero:
 * invisible); the signature itself is made on the next (incremental) save, when
 * MuPDF calls cb with the bytes to sign: it writes the CMS blob into out (at most
 * cap bytes) and returns its length, or -1. dn: CN, O, OU, email, country.
 * num: an existing empty signature field's object number, or -1 for a new one.
 * flags: PDF_SIGNATURE_SHOW_*; lock: lock every form field (new fields only). */
typedef ptrdiff_t (*fo_sign_cb)(void *user, const unsigned char *data, size_t len, unsigned char *out, size_t cap);
int fo_sign_page(fo_ctx *c, fo_page *p, int num, const float rect[4], fo_sign_cb cb, void *user, size_t max,
	const char *const dn[5], const char *reason, const char *location,
	const unsigned char *img, size_t img_len, int flags, int lock);
char *fo_sigs_json(fo_ctx *c, fo_doc *d);
int fo_sig_data(fo_ctx *c, fo_doc *d, int i, unsigned char **data, size_t *len, unsigned char **cms, size_t *cms_len);

void fo_free(fo_ctx *c, void *p);

#endif

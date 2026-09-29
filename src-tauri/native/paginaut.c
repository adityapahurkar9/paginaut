#include "paginaut.h"
#include "ocr.h"
#include <mupdf/fitz.h>
#include <mupdf/pdf.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
#include <time.h>

#ifdef _WIN32
#include <windows.h>
static CRITICAL_SECTION g_locks[FZ_LOCK_MAX];
static void lock_fn(void *u, int i) { (void)u; EnterCriticalSection(&g_locks[i]); }
static void unlock_fn(void *u, int i) { (void)u; LeaveCriticalSection(&g_locks[i]); }
/* MuPDF takes these locks for very short sections (refcounts, the store), so spin
   briefly before sleeping; without a spin count, contention across 16 threads
   made parallel work slower than one thread. */
static void locks_init(void) { for (int i = 0; i < FZ_LOCK_MAX; i++) InitializeCriticalSectionAndSpinCount(&g_locks[i], 4000); }
#else
#include <pthread.h>
static pthread_mutex_t g_locks[FZ_LOCK_MAX];
static void lock_fn(void *u, int i) { (void)u; pthread_mutex_lock(&g_locks[i]); }
static void unlock_fn(void *u, int i) { (void)u; pthread_mutex_unlock(&g_locks[i]); }
static void locks_init(void) { for (int i = 0; i < FZ_LOCK_MAX; i++) pthread_mutex_init(&g_locks[i], NULL); }
#endif

struct fo_ctx
{
	fz_context *ctx;
	char err[512];
};

static fz_context *g_base = NULL;
static fz_locks_context g_locks_ctx = { NULL, lock_fn, unlock_fn };

static void quiet(void *user, const char *msg) { (void)user; (void)msg; }

#define CTX (c->ctx)
#define FAIL() snprintf(c->err, sizeof c->err, "%s", fz_caught_message(CTX))

int fo_global_init(size_t store_bytes)
{
	if (g_base)
		return 0;
	locks_init();
	g_base = fz_new_context(NULL, &g_locks_ctx, store_bytes);
	if (!g_base)
		return -1;
	fz_set_warning_callback(g_base, quiet, NULL);
	fz_set_error_callback(g_base, quiet, NULL);
	fz_try(g_base)
	{
		fz_register_document_handlers(g_base);
		fz_disable_icc(g_base);
	}
	fz_catch(g_base)
		return -1;
	return 0;
}

fo_ctx *fo_ctx_new(void)
{
	fo_ctx *c;
	if (!g_base)
		return NULL;
	c = calloc(1, sizeof *c);
	if (!c)
		return NULL;
	c->ctx = fz_clone_context(g_base);
	if (!c->ctx)
	{
		free(c);
		return NULL;
	}
	return c;
}

fo_ctx *fo_ctx_new_private(size_t store_bytes)
{
	fo_ctx *c = calloc(1, sizeof *c);
	if (!c)
		return NULL;
	c->ctx = fz_new_context(NULL, NULL, store_bytes);
	if (!c->ctx)
	{
		free(c);
		return NULL;
	}
	fz_set_warning_callback(c->ctx, quiet, NULL);
	fz_set_error_callback(c->ctx, quiet, NULL);
	fz_try(c->ctx)
	{
		fz_register_document_handlers(c->ctx);
		fz_disable_icc(c->ctx);
	}
	fz_catch(c->ctx)
	{
		fz_drop_context(c->ctx);
		free(c);
		return NULL;
	}
	return c;
}

void fo_ctx_drop(fo_ctx *c)
{
	if (!c)
		return;
	fz_drop_context(c->ctx);
	free(c);
}

const char *fo_error(fo_ctx *c) { return c->err; }

void fo_store_shrink(fo_ctx *c, int percent)
{
	if (percent >= 100)
		fz_empty_store(CTX);
	else
		fz_shrink_store(CTX, (unsigned int)(100 - percent));
}

fo_cookie *fo_cookie_new(void) { return (fo_cookie *)calloc(1, sizeof(fz_cookie)); }
void fo_cookie_abort(fo_cookie *k) { if (k) ((volatile fz_cookie *)k)->abort = 1; }
void fo_cookie_drop(fo_cookie *k) { free(k); }

void fo_free(fo_ctx *c, void *p) { fz_free(CTX, p); }

/* Appends the text of the line containing point p (UTF-8) to b. */
static void line_text_at(fz_context *ctx, fz_buffer *b, fz_stext_page *page, fz_point p)
{
	fz_stext_block *block;
	fz_stext_line *line;
	fz_stext_char *ch;
	for (block = page->first_block; block; block = block->next)
	{
		if (block->type != FZ_STEXT_BLOCK_TEXT)
			continue;
		for (line = block->u.t.first_line; line; line = line->next)
		{
			fz_rect r = line->bbox;
			if (p.x < r.x0 - 1 || p.x > r.x1 + 1 || p.y < r.y0 - 1 || p.y > r.y1 + 1)
				continue;
			for (ch = line->first_char; ch; ch = ch->next)
				fz_append_rune(ctx, b, ch->c);
			return;
		}
	}
}

static void put_quad(float *out, fz_quad q, const float b[4]);

/* ---------------------------------------------------------------- JSON helpers */

static void json_str(fz_context *ctx, fz_buffer *b, const char *s)
{
	fz_append_byte(ctx, b, '"');
	for (; s && *s; s++)
	{
		unsigned char ch = (unsigned char)*s;
		if (ch == '"' || ch == '\\')
		{
			fz_append_byte(ctx, b, '\\');
			fz_append_byte(ctx, b, ch);
		}
		else if (ch < 0x20)
			fz_append_printf(ctx, b, "\\u%04x", ch);
		else
			fz_append_byte(ctx, b, ch);
	}
	fz_append_byte(ctx, b, '"');
}

static char *buffer_to_string(fz_context *ctx, fz_buffer *b)
{
	unsigned char *data;
	size_t len = fz_buffer_storage(ctx, b, &data);
	char *s = fz_malloc(ctx, len + 1);
	memcpy(s, data, len);
	s[len] = 0;
	return s;
}

/* ---------------------------------------------------------------- documents */

/* Windows paths longer than MAX_PATH only work in the \\?\ form (absolute,
 * backslashes). Returns a string to free with fz_free. */
static char *os_path(fz_context *ctx, const char *path)
{
	size_t n = strlen(path);
#ifdef _WIN32
	if (n >= 240 && strncmp(path, "\\\\?\\", 4) != 0)
	{
		int unc = (path[0] == '\\' || path[0] == '/') && (path[1] == '\\' || path[1] == '/');
		const char *prefix = unc ? "\\\\?\\UNC\\" : "\\\\?\\";
		const char *rest = unc ? path + 2 : path;
		size_t p = strlen(prefix), m = strlen(rest);
		char *s = fz_malloc(ctx, p + m + 1);
		memcpy(s, prefix, p);
		for (size_t i = 0; i <= m; i++)
			s[p + i] = rest[i] == '/' ? '\\' : rest[i];
		return s;
	}
#endif
	return fz_strdup(ctx, path);
}

fo_doc *fo_open(fo_ctx *c, const char *path)
{
	fz_document *doc = NULL;
	char *p = NULL;
	fz_var(p);
	fz_try(CTX)
	{
		p = os_path(CTX, path);
		doc = fz_open_document(CTX, p);
	}
	fz_always(CTX)
		fz_free(CTX, p);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return (fo_doc *)doc;
}

void fo_doc_drop(fo_ctx *c, fo_doc *d) { fz_drop_document(CTX, (fz_document *)d); }

int fo_needs_password(fo_ctx *c, fo_doc *d) { return fz_needs_password(CTX, (fz_document *)d); }

int fo_authenticate(fo_ctx *c, fo_doc *d, const char *password)
{
	int ok = 0;
	fz_try(CTX)
		ok = fz_authenticate_password(CTX, (fz_document *)d, password);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return ok;
}

int fo_is_reflowable(fo_ctx *c, fo_doc *d) { return fz_is_document_reflowable(CTX, (fz_document *)d); }

int fo_layout(fo_ctx *c, fo_doc *d, float w, float h, float em)
{
	fz_try(CTX)
		fz_layout_document(CTX, (fz_document *)d, w, h, em);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

int fo_is_pdf(fo_ctx *c, fo_doc *d) { return pdf_specifics(CTX, (fz_document *)d) != NULL; }

int fo_count_pages(fo_ctx *c, fo_doc *d)
{
	int n = -1;
	fz_try(CTX)
		n = fz_count_pages(CTX, (fz_document *)d);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return n;
}

int fo_metadata(fo_ctx *c, fo_doc *d, const char *key, char *buf, int size)
{
	int n = -1;
	fz_try(CTX)
		n = fz_lookup_metadata(CTX, (fz_document *)d, key, buf, (size_t)size);
	fz_catch(CTX)
		return -1;
	return n;
}

static void outline_rec(fz_context *ctx, fz_document *doc, fz_buffer *b, fz_outline *o)
{
	int first = 1;
	fz_append_byte(ctx, b, '[');
	for (; o; o = o->next)
	{
		int page = -1;
		if (!first)
			fz_append_byte(ctx, b, ',');
		first = 0;
		fz_try(ctx)
			page = fz_page_number_from_location(ctx, doc, o->page);
		fz_catch(ctx)
			page = -1;
		fz_append_string(ctx, b, "{\"t\":");
		json_str(ctx, b, o->title ? o->title : "");
		fz_append_printf(ctx, b, ",\"p\":%d,\"y\":%g", page, o->y);
		if (o->down)
		{
			fz_append_string(ctx, b, ",\"k\":");
			outline_rec(ctx, doc, b, o->down);
		}
		fz_append_byte(ctx, b, '}');
	}
	fz_append_byte(ctx, b, ']');
}

char *fo_outline_json(fo_ctx *c, fo_doc *d)
{
	fz_outline *o = NULL;
	fz_buffer *b = NULL;
	char *s = NULL;
	fz_var(o);
	fz_var(b);
	fz_try(CTX)
	{
		o = fz_load_outline(CTX, (fz_document *)d);
		b = fz_new_buffer(CTX, 1024);
		outline_rec(CTX, (fz_document *)d, b, o);
		s = buffer_to_string(CTX, b);
	}
	fz_always(CTX)
	{
		fz_drop_outline(CTX, o);
		fz_drop_buffer(CTX, b);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* ---------------------------------------------------------------- pages */

/* Loading and drawing pages can make MuPDF (re)generate a field's
 * appearance. That isn't an edit: it must not become an undo step, and it
 * must not wipe out the redo history. These pause the journal around it. */
static pdf_journal *pause_journal(fz_context *ctx, fz_document *doc)
{
	pdf_document *pdf = doc ? pdf_specifics(ctx, doc) : NULL;
	pdf_journal *j;
	if (!pdf)
		return NULL;
	j = pdf->journal;
	pdf->journal = NULL;
	return j;
}

static void resume_journal(fz_context *ctx, fz_document *doc, pdf_journal *j)
{
	pdf_document *pdf = doc ? pdf_specifics(ctx, doc) : NULL;
	if (pdf && j)
		pdf->journal = j;
}

fo_page *fo_page_load(fo_ctx *c, fo_doc *d, int index)
{
	fz_page *p = NULL;
	pdf_journal *j = pause_journal(CTX, (fz_document *)d);
	fz_try(CTX)
		p = fz_load_page(CTX, (fz_document *)d, index);
	fz_always(CTX)
		resume_journal(CTX, (fz_document *)d, j);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return (fo_page *)p;
}

void fo_page_drop(fo_ctx *c, fo_page *p) { fz_drop_page(CTX, (fz_page *)p); }

int fo_page_bounds(fo_ctx *c, fo_page *p, float out[4])
{
	fz_rect r;
	fz_try(CTX)
		r = fz_bound_page(CTX, (fz_page *)p);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	out[0] = r.x0; out[1] = r.y0; out[2] = r.x1; out[3] = r.y1;
	return 0;
}

int fo_page_label(fo_ctx *c, fo_page *p, char *buf, int size)
{
	fz_try(CTX)
		fz_page_label(CTX, (fz_page *)p, buf, size);
	fz_catch(CTX)
	{
		buf[0] = 0;
		return -1;
	}
	return 0;
}

char *fo_links_json(fo_ctx *c, fo_doc *d, fo_page *p)
{
	fz_link *links = NULL, *l;
	fz_buffer *b = NULL;
	char *s = NULL;
	fz_rect pb;
	int first = 1;
	fz_var(links);
	fz_var(b);
	fz_try(CTX)
	{
		pb = fz_bound_page(CTX, (fz_page *)p);
		links = fz_load_links(CTX, (fz_page *)p);
		b = fz_new_buffer(CTX, 256);
		fz_append_byte(CTX, b, '[');
		for (l = links; l; l = l->next)
		{
			if (!l->uri)
				continue;
			if (!first)
				fz_append_byte(CTX, b, ',');
			first = 0;
			fz_append_printf(CTX, b, "{\"r\":[%g,%g,%g,%g]", l->rect.x0 - pb.x0, l->rect.y0 - pb.y0, l->rect.x1 - pb.x0, l->rect.y1 - pb.y0);
			if (fz_is_external_link(CTX, l->uri))
			{
				fz_append_string(CTX, b, ",\"u\":");
				json_str(CTX, b, l->uri);
			}
			else
			{
				float x = 0, y = 0;
				int page = -1;
				fz_try(CTX)
					page = fz_page_number_from_location(CTX, (fz_document *)d, fz_resolve_link(CTX, (fz_document *)d, l->uri, &x, &y));
				fz_catch(CTX)
					page = -1;
				fz_append_printf(CTX, b, ",\"p\":%d,\"y\":%g", page, y);
			}
			fz_append_byte(CTX, b, '}');
		}
		fz_append_byte(CTX, b, ']');
		s = buffer_to_string(CTX, b);
	}
	fz_always(CTX)
	{
		fz_drop_link(CTX, links);
		fz_drop_buffer(CTX, b);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* The page with fillable fields left out (the app draws those itself as
 * inputs); push buttons and signature fields are kept. */
static fz_display_list *list_without_fields(fz_context *ctx, fz_page *page)
{
	fz_display_list *l = fz_new_display_list(ctx, fz_bound_page(ctx, page));
	fz_device *dev = NULL;
	fz_var(dev);
	fz_try(ctx)
	{
		pdf_page *pp = pdf_page_from_fz_page(ctx, page);
		dev = fz_new_list_device(ctx, l);
		fz_run_page_contents(ctx, page, dev, fz_identity, NULL);
		fz_run_page_annots(ctx, page, dev, fz_identity, NULL);
		if (pp)
			for (pdf_annot *w = pdf_first_widget(ctx, pp); w; w = pdf_next_widget(ctx, w))
			{
				int t = pdf_widget_type(ctx, w);
				if (t == PDF_WIDGET_TYPE_BUTTON || t == PDF_WIDGET_TYPE_SIGNATURE || t == PDF_WIDGET_TYPE_UNKNOWN)
					pdf_run_annot(ctx, w, dev, fz_identity, NULL);
			}
		fz_close_device(ctx, dev);
	}
	fz_always(ctx)
		fz_drop_device(ctx, dev);
	fz_catch(ctx)
	{
		fz_drop_display_list(ctx, l);
		fz_rethrow(ctx);
	}
	return l;
}

fo_list *fo_list_new(fo_ctx *c, fo_page *p, int extras)
{
	fz_display_list *l = NULL;
	fz_document *doc = ((fz_page *)p)->doc;
	pdf_journal *j = pause_journal(CTX, doc);
	fz_try(CTX)
	{
		if (extras == 2)
			l = list_without_fields(CTX, (fz_page *)p);
		else if (extras)
			l = fz_new_display_list_from_page(CTX, (fz_page *)p);
		else
			l = fz_new_display_list_from_page_contents(CTX, (fz_page *)p);
	}
	fz_always(CTX)
		resume_journal(CTX, doc, j);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return (fo_list *)l;
}

void fo_list_drop(fo_ctx *c, fo_list *l) { fz_drop_display_list(CTX, (fz_display_list *)l); }

/* ---------------------------------------------------------------- rendering */

static fz_matrix page_matrix(fz_rect b, float scale, int rotate)
{
	fz_matrix m = fz_concat(fz_translate(-b.x0, -b.y0), fz_scale(scale, scale));
	fz_rect r;
	if (rotate % 360)
		m = fz_concat(m, fz_rotate((float)rotate));
	r = fz_transform_rect(b, m);
	return fz_concat(m, fz_translate(-r.x0, -r.y0));
}

void fo_render_size(const float bf[4], float scale, int rotate, int *w, int *h)
{
	fz_rect b = fz_make_rect(bf[0], bf[1], bf[2], bf[3]);
	fz_rect r = fz_transform_rect(b, page_matrix(b, scale, rotate));
	*w = (int)(r.x1 - r.x0 + 0.5f);
	*h = (int)(r.y1 - r.y0 + 0.5f);
}

int fo_render(fo_ctx *c, fo_list *l, const float bf[4], float scale, int rotate,
	const int clip[4], unsigned char *out, fo_cookie *k)
{
	fz_pixmap *pix = NULL;
	fz_device *dev = NULL;
	fz_var(pix);
	fz_var(dev);
	fz_try(CTX)
	{
		fz_rect b = fz_make_rect(bf[0], bf[1], bf[2], bf[3]);
		fz_matrix m = page_matrix(b, scale, rotate);
		fz_irect area = fz_make_irect(clip[0], clip[1], clip[2], clip[3]);
		pix = fz_new_pixmap_with_bbox_and_data(CTX, fz_device_rgb(CTX), area, NULL, 1, out);
		fz_clear_pixmap_with_value(CTX, pix, 255);
		dev = fz_new_draw_device(CTX, fz_identity, pix);
		fz_run_display_list(CTX, (fz_display_list *)l, dev, m, fz_rect_from_irect(area), (fz_cookie *)k);
		fz_close_device(CTX, dev);
	}
	fz_always(CTX)
	{
		fz_drop_device(CTX, dev);
		fz_drop_pixmap(CTX, pix);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	if (k && ((fz_cookie *)k)->abort)
		return 1;
	return 0;
}

char *fo_text_match_json(fo_ctx *c, fo_text *t, const float b[4], const char *needle, int flags, int snippets)
{
	enum { MAX_HITS = 2000 };
	fz_quad *quads = NULL;
	int *marks = NULL;
	fz_buffer *buf = NULL, *line = NULL;
	char *s = NULL;
	fz_var(quads);
	fz_var(marks);
	fz_var(buf);
	fz_var(line);
	fz_try(CTX)
	{
		int n, opts = FZ_SEARCH_EXACT;
		if (!(flags & 1))
			opts |= FZ_SEARCH_IGNORE_CASE | FZ_SEARCH_IGNORE_DIACRITICS;
		if (flags & 2)
			opts |= FZ_SEARCH_REGEXP;
		quads = fz_malloc_array(CTX, MAX_HITS, fz_quad);
		marks = fz_malloc_array(CTX, MAX_HITS, int);
		n = fz_match_stext_page(CTX, (fz_stext_page *)t, needle, marks, quads, MAX_HITS, (fz_search_options)opts);
		buf = fz_new_buffer(CTX, 256);
		line = fz_new_buffer(CTX, 128);
		fz_append_byte(CTX, buf, '[');
		for (int i = 0; i < n; i++)
		{
			float q[8];
			int first = (i == 0 || marks[i]);
			if (first)
			{
				if (i > 0)
				{
					fz_append_byte(CTX, buf, ']');
					if (snippets)
					{
						fz_append_string(CTX, buf, ",\"s\":");
						fz_terminate_buffer(CTX, line);
						json_str(CTX, buf, fz_string_from_buffer(CTX, line));
					}
					fz_append_string(CTX, buf, "},");
				}
				fz_append_string(CTX, buf, "{\"q\":[");
				if (snippets)
				{
					fz_clear_buffer(CTX, line);
					line_text_at(CTX, line, (fz_stext_page *)t, fz_make_point((quads[i].ul.x + quads[i].lr.x) / 2, (quads[i].ul.y + quads[i].lr.y) / 2));
				}
			}
			else
				fz_append_byte(CTX, buf, ',');
			put_quad(q, quads[i], b);
			fz_append_printf(CTX, buf, "%g,%g,%g,%g,%g,%g,%g,%g", q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]);
		}
		if (n > 0)
		{
			fz_append_byte(CTX, buf, ']');
			if (snippets)
			{
				fz_append_string(CTX, buf, ",\"s\":");
				fz_terminate_buffer(CTX, line);
				json_str(CTX, buf, fz_string_from_buffer(CTX, line));
			}
			fz_append_byte(CTX, buf, '}');
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_free(CTX, quads);
		fz_free(CTX, marks);
		fz_drop_buffer(CTX, buf);
		fz_drop_buffer(CTX, line);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

char *fo_image_rects_json(fo_ctx *c, fo_list *l, const float b[4])
{
	fz_stext_page *t = NULL;
	fz_buffer *buf = NULL;
	char *s = NULL;
	fz_var(t);
	fz_var(buf);
	fz_try(CTX)
	{
		fz_stext_options opts;
		int first = 1;
		fz_init_stext_options(CTX, &opts);
		opts.flags |= FZ_STEXT_PRESERVE_IMAGES;
		t = fz_new_stext_page_from_display_list(CTX, (fz_display_list *)l, &opts);
		buf = fz_new_buffer(CTX, 128);
		fz_append_byte(CTX, buf, '[');
		for (fz_stext_block *block = t->first_block; block; block = block->next)
		{
			if (block->type != FZ_STEXT_BLOCK_IMAGE)
				continue;
			/* Pictures with a transparent background (signatures, logos) darken with the text. */
			if (block->u.i.image && block->u.i.image->mask)
				continue;
			if (!first)
				fz_append_byte(CTX, buf, ',');
			first = 0;
			fz_append_printf(CTX, buf, "[%g,%g,%g,%g]", block->bbox.x0 - b[0], block->bbox.y0 - b[1], block->bbox.x1 - b[0], block->bbox.y1 - b[1]);
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_drop_stext_page(CTX, t);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* ---------------------------------------------------------------- reflow */

static void put_bbox(fz_context *ctx, fz_buffer *buf, fz_rect r, const float o[4])
{
	fz_append_printf(ctx, buf, "[%.1f,%.1f,%.1f,%.1f]", r.x0 - o[0], r.y0 - o[1], r.x1 - o[0], r.y1 - o[1]);
}

static fz_rect struct_bbox(fz_stext_block *b)
{
	fz_rect r = fz_empty_rect;
	for (; b; b = b->next)
	{
		if (b->type == FZ_STEXT_BLOCK_STRUCT && b->u.s.down)
			r = fz_union_rect(r, struct_bbox(b->u.s.down->first_block));
		else
			r = fz_union_rect(r, b->bbox);
	}
	return r;
}

/* Recognised (OCR) text uses an invisible font whose glyphs are all one width
 * (half an em, stretched to fit each word), so its fixed pitch and stretched size
 * say nothing about how the text looked. */
static int is_ocr_font(fz_context *ctx, fz_font *font)
{
	return font && strcmp(fz_font_name(ctx, font), "GlyphLessFont") == 0;
}

/* Text size of a character. MuPDF's size includes horizontal stretching; for OCR
 * text the glyph box height is the size the line was recognised at. */
static float char_size(fz_context *ctx, fz_stext_char *ch)
{
	if (is_ocr_font(ctx, ch->font))
		return hypotf(ch->quad.ul.x - ch->quad.ll.x, ch->quad.ul.y - ch->quad.ll.y);
	return ch->size;
}

static int char_style(fz_context *ctx, fz_stext_char *ch, float line_size, float baseline)
{
	int s = 0;
	if ((ch->flags & FZ_STEXT_BOLD) || (ch->font && fz_font_is_bold(ctx, ch->font)))
		s |= 1;
	if (ch->font && fz_font_is_italic(ctx, ch->font))
		s |= 2;
	if (ch->font && fz_font_is_monospaced(ctx, ch->font) && !is_ocr_font(ctx, ch->font))
		s |= 4;
	if (char_size(ctx, ch) < 0.8f * line_size && ch->origin.y < baseline - 0.15f * line_size)
		s |= 8;
	return s;
}

static void reflow_line(fz_context *ctx, fz_buffer *buf, fz_buffer *span, fz_stext_line *line, const float o[4])
{
	float line_size = 0, baseline = 0, size = -1;
	int style = -1, spans = 0;
	fz_stext_char *ch;
	for (ch = line->first_char; ch; ch = ch->next)
		if (char_size(ctx, ch) > line_size)
		{
			line_size = char_size(ctx, ch);
			baseline = ch->origin.y;
		}
	fz_append_string(ctx, buf, "{\"x\":");
	put_bbox(ctx, buf, line->bbox, o);
	if (line->flags & FZ_STEXT_LINE_FLAGS_JOINED)
		fz_append_string(ctx, buf, ",\"j\":1");
	fz_append_string(ctx, buf, ",\"s\":[");
	fz_clear_buffer(ctx, span);
	for (ch = line->first_char; ch; ch = ch->next)
	{
		float sz = floorf(char_size(ctx, ch) * 2 + 0.5f) / 2;
		int st = char_style(ctx, ch, line_size, baseline);
		if (span->len && (sz != size || st != style))
		{
			if (spans++)
				fz_append_byte(ctx, buf, ',');
			fz_append_byte(ctx, buf, '[');
			fz_terminate_buffer(ctx, span);
			json_str(ctx, buf, fz_string_from_buffer(ctx, span));
			fz_append_printf(ctx, buf, ",%g,%d]", size, style);
			fz_clear_buffer(ctx, span);
		}
		size = sz;
		style = st;
		fz_append_rune(ctx, span, ch->c);
	}
	if (span->len)
	{
		if (spans++)
			fz_append_byte(ctx, buf, ',');
		fz_append_byte(ctx, buf, '[');
		fz_terminate_buffer(ctx, span);
		json_str(ctx, buf, fz_string_from_buffer(ctx, span));
		fz_append_printf(ctx, buf, ",%g,%d]", size, style);
	}
	fz_append_string(ctx, buf, "]}");
}

static void reflow_blocks(fz_context *ctx, fz_buffer *buf, fz_buffer *span, fz_stext_block *b, const float o[4], int *n)
{
	for (; b; b = b->next)
	{
		switch (b->type)
		{
		case FZ_STEXT_BLOCK_TEXT:
		{
			int nl = 0;
			if (!b->u.t.first_line)
				break;
			fz_append_string(ctx, buf, (*n)++ ? ",{\"t\":\"p\",\"x\":" : "{\"t\":\"p\",\"x\":");
			put_bbox(ctx, buf, b->bbox, o);
			fz_append_string(ctx, buf, ",\"l\":[");
			for (fz_stext_line *line = b->u.t.first_line; line; line = line->next)
			{
				if (!line->first_char)
					continue;
				if (nl++)
					fz_append_byte(ctx, buf, ',');
				reflow_line(ctx, buf, span, line, o);
			}
			fz_append_string(ctx, buf, "]}");
			break;
		}
		case FZ_STEXT_BLOCK_IMAGE:
			fz_append_string(ctx, buf, (*n)++ ? ",{\"t\":\"i\",\"x\":" : "{\"t\":\"i\",\"x\":");
			put_bbox(ctx, buf, b->bbox, o);
			fz_append_byte(ctx, buf, '}');
			break;
		case FZ_STEXT_BLOCK_VECTOR:
			if (b->u.v.flags & (FZ_STEXT_VECTOR_IS_HIGHLIGHT | FZ_STEXT_VECTOR_IS_UNDERLINE | FZ_STEXT_VECTOR_IS_STRIKEOUT))
				break;
			fz_append_string(ctx, buf, (*n)++ ? ",{\"t\":\"v\",\"x\":" : "{\"t\":\"v\",\"x\":");
			put_bbox(ctx, buf, b->bbox, o);
			fz_append_byte(ctx, buf, '}');
			break;
		case FZ_STEXT_BLOCK_STRUCT:
		{
			fz_stext_struct *s = b->u.s.down;
			if (!s)
				break;
			if (s->standard == FZ_STRUCTURE_TABLE)
			{
				fz_append_string(ctx, buf, (*n)++ ? ",{\"t\":\"tb\",\"x\":" : "{\"t\":\"tb\",\"x\":");
				put_bbox(ctx, buf, struct_bbox(s->first_block), o);
				fz_append_byte(ctx, buf, '}');
			}
			else
				reflow_blocks(ctx, buf, span, s->first_block, o, n);
			break;
		}
		default:
			break;
		}
	}
}

char *fo_reflow_json(fo_ctx *c, fo_list *l, const float b[4])
{
	fz_stext_page *t = NULL;
	fz_buffer *buf = NULL, *span = NULL;
	char *s = NULL;
	fz_var(t);
	fz_var(buf);
	fz_var(span);
	fz_try(CTX)
	{
		fz_stext_options opts;
		int n = 0;
		fz_init_stext_options(CTX, &opts);
		/* No FZ_STEXT_SEGMENT: it costs ~60 ms a page. Column order is worked out
		 * by the app from the block boxes instead (XY-cut). */
		opts.flags |= FZ_STEXT_PRESERVE_IMAGES | FZ_STEXT_DEHYPHENATE | FZ_STEXT_COLLECT_VECTORS |
			FZ_STEXT_PARAGRAPH_BREAK | FZ_STEXT_TABLE_HUNT;
		t = fz_new_stext_page_from_display_list(CTX, (fz_display_list *)l, &opts);
		buf = fz_new_buffer(CTX, 4096);
		span = fz_new_buffer(CTX, 256);
		fz_append_printf(CTX, buf, "{\"w\":%.1f,\"h\":%.1f,\"b\":[", b[2] - b[0], b[3] - b[1]);
		reflow_blocks(CTX, buf, span, t->first_block, b, &n);
		fz_append_string(CTX, buf, "]}");
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_drop_stext_page(CTX, t);
		fz_drop_buffer(CTX, buf);
		fz_drop_buffer(CTX, span);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* ---------------------------------------------------------------- forms */

int fo_pdf_forms_init(fo_ctx *c, fo_doc *d, int *xfa)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int n = 0;
	*xfa = 0;
	if (!pdf)
		return 0;
	fz_try(CTX)
	{
		pdf_obj *form = pdf_dict_getp(CTX, pdf_trailer(CTX, pdf), "Root/AcroForm");
		n = pdf_array_len(CTX, pdf_dict_get(CTX, form, PDF_NAME(Fields)));
		*xfa = pdf_dict_get(CTX, form, PDF_NAME(XFA)) != NULL;
		/* The journal gives undo/redo for form and annotation edits. */
		pdf_enable_journal(CTX, pdf);
		/* Form scripts: calculated totals, number/date formatting, validation. */
		if (n > 0)
			pdf_enable_js(CTX, pdf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return n;
}

static const char *widget_type_name(int t)
{
	switch (t)
	{
	case PDF_WIDGET_TYPE_TEXT: return "text";
	case PDF_WIDGET_TYPE_CHECKBOX: return "checkbox";
	case PDF_WIDGET_TYPE_RADIOBUTTON: return "radio";
	case PDF_WIDGET_TYPE_COMBOBOX: return "combo";
	case PDF_WIDGET_TYPE_LISTBOX: return "list";
	case PDF_WIDGET_TYPE_SIGNATURE: return "signature";
	case PDF_WIDGET_TYPE_BUTTON: return "button";
	default: return "unknown";
	}
}

char *fo_widgets_json(fo_ctx *c, fo_page *p, const float b[4])
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	fz_buffer *buf = NULL;
	char *s = NULL, *name = NULL;
	const char **opts = NULL, **exps = NULL;
	fz_var(buf);
	fz_var(name);
	fz_var(opts);
	fz_var(exps);
	if (!page)
		return fz_strdup(CTX, "[]");
	fz_try(CTX)
	{
		int first = 1;
		buf = fz_new_buffer(CTX, 1024);
		fz_append_byte(CTX, buf, '[');
		for (pdf_annot *w = pdf_first_widget(CTX, page); w; w = pdf_next_widget(CTX, w))
		{
			pdf_obj *obj = pdf_annot_obj(CTX, w);
			int type = pdf_widget_type(CTX, w);
			int flags = pdf_field_flags(CTX, obj);
			fz_rect r = pdf_bound_widget(CTX, w);
			const char *font = NULL;
			float size = 0, color[4] = { 0 };
			int ncolor = 0;
			if (!first)
				fz_append_byte(CTX, buf, ',');
			first = 0;
			fz_append_printf(CTX, buf, "{\"n\":%d,\"t\":\"%s\",\"r\":[%.2f,%.2f,%.2f,%.2f]", pdf_to_num(CTX, obj), widget_type_name(type),
				r.x0 - b[0], r.y0 - b[1], r.x1 - b[0], r.y1 - b[1]);
			name = pdf_load_field_name(CTX, obj);
			fz_append_string(CTX, buf, ",\"name\":");
			json_str(CTX, buf, name);
			fz_free(CTX, name);
			name = NULL;
			fz_append_string(CTX, buf, ",\"v\":");
			json_str(CTX, buf, pdf_field_value(CTX, obj));
			fz_append_printf(CTX, buf, ",\"ff\":%d", flags);
			if (pdf_widget_is_readonly(CTX, w))
				fz_append_string(CTX, buf, ",\"ro\":1");
			fz_try(CTX)
				pdf_annot_default_appearance(CTX, w, &font, &size, &ncolor, color);
			fz_catch(CTX)
				size = 0;
			fz_append_printf(CTX, buf, ",\"fs\":%g,\"q\":%d", size, pdf_annot_quadding(CTX, w));
			if (type == PDF_WIDGET_TYPE_TEXT)
			{
				char *fv = NULL;
				fz_append_printf(CTX, buf, ",\"max\":%d,\"fmt\":%d", pdf_text_widget_max_len(CTX, w), pdf_text_widget_format(CTX, w));
				/* How the form's own format script shows the value ("Rs 37.50" for 37.5). */
				/* Running the script is not an edit: keep it out of the undo history. */
				{
					pdf_journal *journal = page->doc->journal;
					page->doc->journal = NULL;
					fz_try(CTX)
						fv = pdf_field_event_format(CTX, page->doc, obj);
					fz_always(CTX)
						page->doc->journal = journal;
					fz_catch(CTX)
						fv = NULL;
				}
				if (fv && strcmp(fv, pdf_field_value(CTX, obj)))
				{
					fz_append_string(CTX, buf, ",\"fv\":");
					json_str(CTX, buf, fv);
				}
				fz_free(CTX, fv);
			}
			if (type == PDF_WIDGET_TYPE_CHECKBOX || type == PDF_WIDGET_TYPE_RADIOBUTTON)
			{
				pdf_obj *as = pdf_dict_get(CTX, obj, PDF_NAME(AS));
				int on = as && !pdf_name_eq(CTX, as, PDF_NAME(Off));
				fz_append_printf(CTX, buf, ",\"on\":%d", on);
			}
			if (type == PDF_WIDGET_TYPE_COMBOBOX || type == PDF_WIDGET_TYPE_LISTBOX)
			{
				int n = pdf_choice_widget_options(CTX, w, 0, NULL);
				opts = fz_malloc_array(CTX, n > 0 ? n : 1, const char *);
				exps = fz_malloc_array(CTX, n > 0 ? n : 1, const char *);
				pdf_choice_widget_options(CTX, w, 0, opts);
				pdf_choice_widget_options(CTX, w, 1, exps);
				fz_append_string(CTX, buf, ",\"opts\":[");
				for (int i = 0; i < n; i++)
				{
					if (i)
						fz_append_byte(CTX, buf, ',');
					fz_append_byte(CTX, buf, '[');
					json_str(CTX, buf, opts[i] ? opts[i] : "");
					fz_append_byte(CTX, buf, ',');
					json_str(CTX, buf, exps[i] ? exps[i] : "");
					fz_append_byte(CTX, buf, ']');
				}
				fz_append_byte(CTX, buf, ']');
				fz_free(CTX, opts);
				fz_free(CTX, exps);
				opts = exps = NULL;
				if (pdf_choice_widget_is_multiselect(CTX, w))
					fz_append_string(CTX, buf, ",\"multi\":1");
			}
			if (type == PDF_WIDGET_TYPE_SIGNATURE && pdf_widget_is_signed(CTX, w))
				fz_append_string(CTX, buf, ",\"signed\":1");
			fz_append_byte(CTX, buf, '}');
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_drop_buffer(CTX, buf);
		fz_free(CTX, name);
		fz_free(CTX, opts);
		fz_free(CTX, exps);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

static pdf_annot *find_widget(fz_context *ctx, pdf_page *page, int num)
{
	for (pdf_annot *w = pdf_first_widget(ctx, page); w; w = pdf_next_widget(ctx, w))
		if (pdf_to_num(ctx, pdf_annot_obj(ctx, w)) == num)
			return w;
	return NULL;
}

/* Sets a text or choice field (scripts run: formatting, validation,
 * calculations). The value, the script's effects and the redrawn fields are
 * one undo step. Returns 1 accepted, 0 rejected by the form, -1 error. */
int fo_widget_set(fo_ctx *c, fo_page *p, int num, const char *value)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	int rc = -1;
	if (!page)
		return -1;
	fz_try(CTX)
	{
		pdf_annot *w = find_widget(CTX, page, num);
		if (!w)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "field not found");
		pdf_begin_operation(CTX, page->doc, "Fill in field");
		fz_try(CTX)
		{
			switch (pdf_widget_type(CTX, w))
			{
			case PDF_WIDGET_TYPE_TEXT:
				rc = pdf_set_text_field_value(CTX, w, value) ? 1 : 0;
				break;
			case PDF_WIDGET_TYPE_COMBOBOX:
			case PDF_WIDGET_TYPE_LISTBOX:
				rc = pdf_set_choice_field_value(CTX, w, value) ? 1 : 0;
				break;
			default:
				fz_throw(CTX, FZ_ERROR_ARGUMENT, "not a text or choice field");
			}
			pdf_update_open_pages(CTX, page->doc);
		}
		fz_always(CTX)
			pdf_end_operation(CTX, page->doc);
		fz_catch(CTX)
			fz_rethrow(CTX);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return rc;
}

/* Checkbox or radio button. Returns 1 on success, -1 error. */
int fo_widget_toggle(fo_ctx *c, fo_page *p, int num)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	if (!page)
		return -1;
	fz_try(CTX)
	{
		pdf_annot *w = find_widget(CTX, page, num);
		if (!w)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "field not found");
		pdf_begin_operation(CTX, page->doc, "Toggle field");
		fz_try(CTX)
		{
			pdf_toggle_widget(CTX, w);
			pdf_update_open_pages(CTX, page->doc);
		}
		fz_always(CTX)
			pdf_end_operation(CTX, page->doc);
		fz_catch(CTX)
			fz_rethrow(CTX);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 1;
}

/* Undo (redo = 0) or redo one journal step. Returns 1 if a step was taken, 2 if
 * that step added, removed or resized pages. */
int fo_pdf_undo(fo_ctx *c, fo_doc *d, int redo)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int done = 0;
	if (!pdf)
		return 0;
	fz_try(CTX)
	{
		if (redo ? pdf_can_redo(CTX, pdf) : pdf_can_undo(CTX, pdf))
		{
			int steps, cur = pdf_undoredo_state(CTX, pdf, &steps);
			const char *title = pdf_undoredo_step(CTX, pdf, redo ? cur : cur - 1);
			int layout = title && (!strcmp(title, "Page size") || !strncmp(title, "Insert ", 7));
			/* The journal restores the fields' drawn appearance too; redrawing
			 * here would be a new edit and wipe out the redo history. */
			if (redo)
				pdf_redo(CTX, pdf);
			else
				pdf_undo(CTX, pdf);
			done = layout ? 2 : 1;
		}
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return done;
}

int fo_pdf_has_changes(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int r = 0;
	if (!pdf)
		return 0;
	fz_try(CTX)
		r = pdf_has_unsaved_changes(CTX, pdf);
	fz_catch(CTX)
		r = 0;
	return r;
}

/* Clears every field to its default value (one undo step). */
int fo_pdf_reset_form(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	if (!pdf)
		return -1;
	fz_try(CTX)
	{
		pdf_begin_operation(CTX, pdf, "Clear form");
		fz_try(CTX)
		{
			pdf_reset_form(CTX, pdf, NULL, 0);
			pdf_update_open_pages(CTX, pdf);
		}
		fz_always(CTX)
			pdf_end_operation(CTX, pdf);
		fz_catch(CTX)
			fz_rethrow(CTX);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 1;
}

/* Turns fields (and optionally annotations) into plain page content. */
int fo_pdf_bake(fo_ctx *c, fo_doc *d, int annots)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	if (!pdf)
		return -1;
	fz_try(CTX)
		pdf_bake_document(CTX, pdf, annots, 1);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 1;
}

/* ---------------------------------------------------------------- annotations */

static void put_color(fz_context *ctx, fz_buffer *buf, const char *key, int n, const float *c)
{
	float rgb[3] = { 0, 0, 0 };
	if (n == 1)
		rgb[0] = rgb[1] = rgb[2] = c[0];
	else if (n == 3)
		rgb[0] = c[0], rgb[1] = c[1], rgb[2] = c[2];
	else if (n == 4)
	{
		rgb[0] = 1 - fz_min(1, c[0] + c[3]);
		rgb[1] = 1 - fz_min(1, c[1] + c[3]);
		rgb[2] = 1 - fz_min(1, c[2] + c[3]);
	}
	else
		return;
	fz_append_printf(ctx, buf, ",\"%s\":[%.3f,%.3f,%.3f]", key, rgb[0], rgb[1], rgb[2]);
}

/* JSON list of the page's annotations (not form fields or popups):
 * [{"n":objnum,"t":type,"r":[bbox],"c":[rgb],"ic":[rgb],"o":opacity,"bw":width,
 *   "text","author","date":unix,"icon","q":[quads],"ink":[[x,y,..]..],"line":[x0,y0,x1,y1],"le":[start,end],"fs":size}] */
char *fo_annots_json(fo_ctx *c, fo_page *p, const float b[4])
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	fz_buffer *buf = NULL;
	char *s = NULL;
	fz_var(buf);
	if (!page)
		return fz_strdup(CTX, "[]");
	fz_try(CTX)
	{
		int first = 1;
		buf = fz_new_buffer(CTX, 1024);
		fz_append_byte(CTX, buf, '[');
		for (pdf_annot *a = pdf_first_annot(CTX, page); a; a = pdf_next_annot(CTX, a))
		{
			enum pdf_annot_type type = pdf_annot_type(CTX, a);
			/* The annotation's own Rect where it has one (moving/resizing keeps its
			 * size exactly); otherwise the drawn bounds. */
			fz_rect r = pdf_annot_has_rect(CTX, a) ? pdf_annot_rect(CTX, a) : pdf_bound_annot(CTX, a);
			float col[4];
			int n;
			if (type == PDF_ANNOT_POPUP || type == PDF_ANNOT_WIDGET)
				continue;
			if (!first)
				fz_append_byte(CTX, buf, ',');
			first = 0;
			fz_append_printf(CTX, buf, "{\"n\":%d,\"t\":\"%s\",\"r\":[%.2f,%.2f,%.2f,%.2f]", pdf_to_num(CTX, pdf_annot_obj(CTX, a)),
				pdf_string_from_annot_type(CTX, type), r.x0 - b[0], r.y0 - b[1], r.x1 - b[0], r.y1 - b[1]);
			pdf_annot_color(CTX, a, &n, col);
			put_color(CTX, buf, "c", n, col);
			if (pdf_annot_has_interior_color(CTX, a))
			{
				pdf_annot_interior_color(CTX, a, &n, col);
				put_color(CTX, buf, "ic", n, col);
			}
			fz_append_printf(CTX, buf, ",\"o\":%.3f", pdf_annot_opacity(CTX, a));
			if (pdf_annot_has_border(CTX, a))
				fz_append_printf(CTX, buf, ",\"bw\":%.2f", pdf_annot_border_width(CTX, a));
			fz_append_string(CTX, buf, ",\"text\":");
			json_str(CTX, buf, pdf_annot_contents(CTX, a));
			if (pdf_annot_has_author(CTX, a))
			{
				fz_append_string(CTX, buf, ",\"author\":");
				json_str(CTX, buf, pdf_annot_author(CTX, a));
			}
			{
				/* fz_append_printf has no 64-bit integers. */
				char tmp[32];
				snprintf(tmp, sizeof tmp, "%lld", (long long)pdf_annot_modification_date(CTX, a));
				fz_append_printf(CTX, buf, ",\"date\":%s", tmp);
			}
			if (pdf_annot_has_icon_name(CTX, a))
			{
				fz_append_string(CTX, buf, ",\"icon\":");
				json_str(CTX, buf, pdf_annot_icon_name(CTX, a));
			}
			if (pdf_annot_has_quad_points(CTX, a))
			{
				int nq = pdf_annot_quad_point_count(CTX, a);
				fz_append_string(CTX, buf, ",\"q\":[");
				for (int i = 0; i < nq; i++)
				{
					float q[8];
					put_quad(q, pdf_annot_quad_point(CTX, a, i), b);
					fz_append_printf(CTX, buf, "%s%.2f,%.2f,%.2f,%.2f,%.2f,%.2f,%.2f,%.2f", i ? "," : "", q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]);
				}
				fz_append_byte(CTX, buf, ']');
			}
			if (pdf_annot_has_ink_list(CTX, a))
			{
				int ns = pdf_annot_ink_list_count(CTX, a);
				fz_append_string(CTX, buf, ",\"ink\":[");
				for (int i = 0; i < ns; i++)
				{
					int nv = pdf_annot_ink_list_stroke_count(CTX, a, i);
					fz_append_string(CTX, buf, i ? ",[" : "[");
					for (int k = 0; k < nv; k++)
					{
						fz_point v = pdf_annot_ink_list_stroke_vertex(CTX, a, i, k);
						fz_append_printf(CTX, buf, "%s%.2f,%.2f", k ? "," : "", v.x - b[0], v.y - b[1]);
					}
					fz_append_byte(CTX, buf, ']');
				}
				fz_append_byte(CTX, buf, ']');
			}
			if (pdf_annot_has_line(CTX, a))
			{
				fz_point pa, pb;
				enum pdf_line_ending s0, s1;
				pdf_annot_line(CTX, a, &pa, &pb);
				pdf_annot_line_ending_styles(CTX, a, &s0, &s1);
				fz_append_printf(CTX, buf, ",\"line\":[%.2f,%.2f,%.2f,%.2f],\"le\":[%d,%d]", pa.x - b[0], pa.y - b[1], pb.x - b[0], pb.y - b[1], (int)s0, (int)s1);
			}
			if (type == PDF_ANNOT_FREE_TEXT)
			{
				const char *font = NULL;
				float size = 0, dc[4];
				int dn = 0;
				fz_try(CTX)
					pdf_annot_default_appearance(CTX, a, &font, &size, &dn, dc);
				fz_catch(CTX)
					size = 0;
				fz_append_printf(CTX, buf, ",\"fs\":%g", size);
				put_color(CTX, buf, "tc", dn, dc);
			}
			fz_append_byte(CTX, buf, '}');
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
		fz_drop_buffer(CTX, buf);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

static pdf_annot *find_annot(fz_context *ctx, pdf_page *page, int num)
{
	for (pdf_annot *a = pdf_first_annot(ctx, page); a; a = pdf_next_annot(ctx, a))
		if (pdf_to_num(ctx, pdf_annot_obj(ctx, a)) == num)
			return a;
	return NULL;
}

void fo_pdf_begin_op(fo_ctx *c, fo_doc *d, const char *name)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	fz_try(CTX)
		if (pdf)
			pdf_begin_operation(CTX, pdf, name);
	fz_catch(CTX)
		;
}

void fo_pdf_end_op(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	fz_try(CTX)
		if (pdf)
			pdf_end_operation(CTX, pdf);
	fz_catch(CTX)
		;
}

void fo_pdf_abandon_op(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	fz_try(CTX)
		if (pdf)
			pdf_abandon_operation(CTX, pdf);
	fz_catch(CTX)
		;
}

/* Creates an annotation of the given type ("Highlight", "Ink", "FreeText"…).
 * Returns its object number, or -1. */
int fo_annot_create(fo_ctx *c, fo_page *p, const char *type, const char *author)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	int num = -1;
	if (!page)
		return -1;
	fz_try(CTX)
	{
		enum pdf_annot_type t = pdf_annot_type_from_string(CTX, type);
		pdf_annot *a;
		if (t == PDF_ANNOT_UNKNOWN)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "unknown annotation type %s", type);
		a = pdf_create_annot(CTX, page, t);
		if (author && *author && pdf_annot_has_author(CTX, a))
			pdf_set_annot_author(CTX, a, author);
		pdf_set_annot_modification_date(CTX, a, time(NULL));
		num = pdf_to_num(CTX, pdf_annot_obj(CTX, a));
		pdf_drop_annot(CTX, a);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return num;
}

#define WITH_ANNOT(body) \
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p); \
	if (!page) \
		return -1; \
	fz_try(CTX) \
	{ \
		pdf_annot *a = find_annot(CTX, page, num); \
		if (!a) \
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "annotation not found"); \
		body; \
	} \
	fz_catch(CTX) \
	{ \
		FAIL(); \
		return -1; \
	} \
	return 0;

int fo_annot_delete(fo_ctx *c, fo_page *p, int num) { WITH_ANNOT(pdf_delete_annot(CTX, page, a)) }
int fo_annot_update(fo_ctx *c, fo_page *p, int num) { WITH_ANNOT(pdf_update_annot(CTX, a)) }
int fo_annot_set_rect(fo_ctx *c, fo_page *p, int num, const float *r) { WITH_ANNOT(pdf_set_annot_rect(CTX, a, fz_make_rect(r[0], r[1], r[2], r[3]))) }
int fo_annot_set_color(fo_ctx *c, fo_page *p, int num, int n, const float *rgb) { WITH_ANNOT(pdf_set_annot_color(CTX, a, n, rgb)) }
int fo_annot_set_interior(fo_ctx *c, fo_page *p, int num, int n, const float *rgb) { WITH_ANNOT(pdf_set_annot_interior_color(CTX, a, n, rgb)) }
int fo_annot_set_opacity(fo_ctx *c, fo_page *p, int num, float o) { WITH_ANNOT(pdf_set_annot_opacity(CTX, a, o)) }
int fo_annot_set_border(fo_ctx *c, fo_page *p, int num, float w) { WITH_ANNOT(pdf_set_annot_border_width(CTX, a, w)) }
int fo_annot_set_contents(fo_ctx *c, fo_page *p, int num, const char *text)
{
	WITH_ANNOT({ pdf_set_annot_contents(CTX, a, text); pdf_set_annot_modification_date(CTX, a, time(NULL)); })
}
int fo_annot_set_icon(fo_ctx *c, fo_page *p, int num, const char *name) { WITH_ANNOT(pdf_set_annot_icon_name(CTX, a, name)) }

int fo_annot_set_quads(fo_ctx *c, fo_page *p, int num, const float *q, int n)
{
	WITH_ANNOT({
		fz_quad *qv = fz_malloc_array(CTX, n > 0 ? n : 1, fz_quad);
		for (int i = 0; i < n; i++)
		{
			const float *v = q + 8 * i;
			qv[i].ul = fz_make_point(v[0], v[1]);
			qv[i].ur = fz_make_point(v[2], v[3]);
			qv[i].ll = fz_make_point(v[4], v[5]);
			qv[i].lr = fz_make_point(v[6], v[7]);
		}
		fz_try(CTX)
			pdf_set_annot_quad_points(CTX, a, n, qv);
		fz_always(CTX)
			fz_free(CTX, qv);
		fz_catch(CTX)
			fz_rethrow(CTX);
	})
}

int fo_annot_set_ink(fo_ctx *c, fo_page *p, int num, const int *counts, int nstrokes, const float *pts)
{
	WITH_ANNOT({
		int total = 0;
		fz_point *v;
		for (int i = 0; i < nstrokes; i++)
			total += counts[i];
		v = fz_malloc_array(CTX, total > 0 ? total : 1, fz_point);
		for (int i = 0; i < total; i++)
			v[i] = fz_make_point(pts[2 * i], pts[2 * i + 1]);
		fz_try(CTX)
			pdf_set_annot_ink_list(CTX, a, nstrokes, counts, v);
		fz_always(CTX)
			fz_free(CTX, v);
		fz_catch(CTX)
			fz_rethrow(CTX);
	})
}

int fo_annot_set_line(fo_ctx *c, fo_page *p, int num, const float *ab, int start, int end)
{
	WITH_ANNOT({
		pdf_set_annot_line(CTX, a, fz_make_point(ab[0], ab[1]), fz_make_point(ab[2], ab[3]));
		pdf_set_annot_line_ending_styles(CTX, a, (enum pdf_line_ending)start, (enum pdf_line_ending)end);
	})
}

int fo_annot_set_font(fo_ctx *c, fo_page *p, int num, const char *font, float size, const float *rgb)
{
	WITH_ANNOT(pdf_set_annot_default_appearance(CTX, a, font, size, 3, rgb))
}

/* A picture (signature, image stamp) from PNG/JPEG bytes. */
int fo_annot_set_image(fo_ctx *c, fo_page *p, int num, const unsigned char *data, size_t len)
{
	WITH_ANNOT({
		fz_buffer *buf = fz_new_buffer_from_copied_data(CTX, data, len);
		fz_image *img = NULL;
		fz_var(img);
		fz_try(CTX)
		{
			img = fz_new_image_from_buffer(CTX, buf);
			pdf_set_annot_stamp_image(CTX, a, img);
		}
		fz_always(CTX)
		{
			fz_drop_image(CTX, img);
			fz_drop_buffer(CTX, buf);
		}
		fz_catch(CTX)
			fz_rethrow(CTX);
	})
}

/* Every annotation in the document, without loading pages (for the comments list):
 * [{"p":page,"n":objnum,"t":type,"text","author","date"}] */
char *fo_doc_annots_json(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	fz_buffer *buf = NULL;
	char *s = NULL;
	fz_var(buf);
	if (!pdf)
		return fz_strdup(CTX, "[]");
	fz_try(CTX)
	{
		int n = pdf_count_pages(CTX, pdf), first = 1;
		buf = fz_new_buffer(CTX, 1024);
		fz_append_byte(CTX, buf, '[');
		for (int i = 0; i < n; i++)
		{
			pdf_obj *annots = pdf_dict_get(CTX, pdf_lookup_page_obj(CTX, pdf, i), PDF_NAME(Annots));
			int k, m = pdf_array_len(CTX, annots);
			for (k = 0; k < m; k++)
			{
				pdf_obj *a = pdf_array_get(CTX, annots, k);
				pdf_obj *st = pdf_dict_get(CTX, a, PDF_NAME(Subtype));
				const char *name = pdf_to_name(CTX, st);
				if (!*name || pdf_name_eq(CTX, st, PDF_NAME(Popup)) || pdf_name_eq(CTX, st, PDF_NAME(Widget)) || pdf_name_eq(CTX, st, PDF_NAME(Link)))
					continue;
				if (!first)
					fz_append_byte(CTX, buf, ',');
				first = 0;
				fz_append_printf(CTX, buf, "{\"p\":%d,\"n\":%d,\"t\":", i, pdf_to_num(CTX, a));
				json_str(CTX, buf, name);
				fz_append_string(CTX, buf, ",\"text\":");
				json_str(CTX, buf, pdf_dict_get_text_string(CTX, a, PDF_NAME(Contents)));
				fz_append_string(CTX, buf, ",\"author\":");
				json_str(CTX, buf, pdf_dict_get_text_string(CTX, a, PDF_NAME(T)));
				{
					char tmp[32];
					snprintf(tmp, sizeof tmp, "%lld", (long long)pdf_dict_get_date(CTX, a, PDF_NAME(M)));
					fz_append_printf(CTX, buf, ",\"date\":%s}", tmp);
				}
			}
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
		fz_drop_buffer(CTX, buf);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* ---------------------------------------------------------------- page edits and saving */

/* A small growable set of object numbers. */
typedef struct { int *v; int n, cap; } num_set;

static int num_set_has(const num_set *s, int x)
{
	for (int i = 0; i < s->n; i++)
		if (s->v[i] == x)
			return 1;
	return 0;
}

static void num_set_add(fz_context *ctx, num_set *s, int x)
{
	if (num_set_has(s, x))
		return;
	if (s->n == s->cap)
	{
		s->cap = s->cap ? s->cap * 2 : 32;
		s->v = fz_realloc_array(ctx, s->v, s->cap, int);
	}
	s->v[s->n++] = x;
}

/* A page's label: style (/D, /R, /r, /A, /a or none), prefix and number. */
typedef struct
{
	pdf_obj *style;
	const char *prefix;
	int value;
} page_label;

/* Collects the ranges of a page label number tree, in page order. */
static void label_ranges(fz_context *ctx, pdf_obj *node, pdf_obj *starts, pdf_obj *dicts, int depth)
{
	pdf_obj *nums = pdf_dict_get(ctx, node, PDF_NAME(Nums));
	pdf_obj *kids = pdf_dict_get(ctx, node, PDF_NAME(Kids));
	if (depth > 32)
		return;
	for (int i = 0; i + 1 < pdf_array_len(ctx, nums); i += 2)
	{
		pdf_array_push(ctx, starts, pdf_array_get(ctx, nums, i));
		pdf_array_push(ctx, dicts, pdf_array_get(ctx, nums, i + 1));
	}
	for (int i = 0; i < pdf_array_len(ctx, kids); i++)
		label_ranges(ctx, pdf_array_get(ctx, kids, i), starts, dicts, depth + 1);
}

/* Every page's label from the document's /PageLabels tree. */
static page_label *read_labels(fz_context *ctx, pdf_document *doc, pdf_obj *tree, int n)
{
	page_label *out = fz_calloc(ctx, n > 0 ? n : 1, sizeof *out);
	pdf_obj *starts = NULL, *dicts = NULL;
	fz_var(starts);
	fz_var(dicts);
	fz_try(ctx)
	{
		starts = pdf_new_array(ctx, doc, 8);
		dicts = pdf_new_array(ctx, doc, 8);
		label_ranges(ctx, tree, starts, dicts, 0);
		for (int i = 0; i < n; i++)
		{
			int r = -1;
			for (int k = 0; k < pdf_array_len(ctx, starts); k++)
				if (pdf_array_get_int(ctx, starts, k) <= i)
					r = k;
			out[i].prefix = "";
			out[i].value = i + 1;
			if (r >= 0)
			{
				pdf_obj *d = pdf_array_get(ctx, dicts, r);
				pdf_obj *st = pdf_dict_get(ctx, d, PDF_NAME(St));
				out[i].style = pdf_dict_get(ctx, d, PDF_NAME(S));
				out[i].prefix = pdf_dict_get_text_string(ctx, d, PDF_NAME(P));
				out[i].value = (st ? pdf_to_int(ctx, st) : 1) + i - pdf_array_get_int(ctx, starts, r);
			}
		}
	}
	fz_always(ctx)
	{
		pdf_drop_obj(ctx, starts);
		pdf_drop_obj(ctx, dicts);
	}
	fz_catch(ctx)
	{
		fz_free(ctx, out);
		fz_rethrow(ctx);
	}
	return out;
}

/* Page labels for the new page order: a page with the same style and prefix as
 * the one before it continues its numbering (so deleting page 5 of 1…10 gives
 * 1…9, and front matter in i, ii, iii stays apart); pages added from other
 * files continue the page before them. */
static void write_labels(fz_context *ctx, pdf_obj *root, const page_label *old, int file_pages, const int *order, int count)
{
	pdf_obj *tree = pdf_dict_put_dict(ctx, root, PDF_NAME(PageLabels), 1);
	pdf_obj *nums = pdf_dict_put_array(ctx, tree, PDF_NAME(Nums), 8);
	page_label prev = { NULL, "", 0 };
	for (int k = 0; k < count; k++)
	{
		page_label l = old[order[k]];
		int added = order[k] >= file_pages;
		int same = k > 0 && (added || (pdf_name_eq(ctx, l.style, prev.style) && !strcmp(l.prefix, prev.prefix)));
		if (same)
		{
			l.style = prev.style;
			l.prefix = prev.prefix;
			l.value = prev.value + 1;
		}
		else
		{
			pdf_obj *d;
			pdf_array_push_int(ctx, nums, k);
			d = pdf_array_push_dict(ctx, nums, 3);
			if (l.style)
				pdf_dict_put(ctx, d, PDF_NAME(S), l.style);
			if (l.prefix[0])
				pdf_dict_put_text_string(ctx, d, PDF_NAME(P), l.prefix);
			if (l.value != 1)
				pdf_dict_put_int(ctx, d, PDF_NAME(St), l.value);
		}
		prev = l;
	}
}

/* A field (or any of its kids) has a widget on a kept page. Kids that don't are dropped. */
static int field_kept(fz_context *ctx, pdf_obj *field, const num_set *widgets, int depth)
{
	pdf_obj *kids = pdf_dict_get(ctx, field, PDF_NAME(Kids));
	int kept = num_set_has(widgets, pdf_to_num(ctx, field));
	if (depth > 32 || !pdf_is_array(ctx, kids))
		return kept;
	for (int i = pdf_array_len(ctx, kids) - 1; i >= 0; i--)
		if (!field_kept(ctx, pdf_array_get(ctx, kids, i), widgets, depth + 1))
			pdf_array_delete(ctx, kids, i);
	return kept || pdf_array_len(ctx, kids) > 0;
}

/* A page destination (explicit array, or an action going to one) on a kept page. */
static int dest_kept(fz_context *ctx, pdf_obj *dest, const num_set *pages)
{
	if (pdf_is_dict(ctx, dest))
	{
		if (!pdf_name_eq(ctx, pdf_dict_get(ctx, dest, PDF_NAME(S)), PDF_NAME(GoTo)))
			return 1;
		dest = pdf_dict_get(ctx, dest, PDF_NAME(D));
	}
	if (!pdf_is_array(ctx, dest))
		return 1; /* a named destination: kept or not with the Dests tree */
	return num_set_has(pages, pdf_to_num(ctx, pdf_array_get(ctx, dest, 0)));
}

/* pdf_rearrange_pages rebuilds the catalog with only a few entries, which
 * would lose the form, page labels, attachments, document scripts, metadata
 * and viewer settings. Puts them back, leaving out what only served dropped
 * pages (so a full rewrite doesn't keep those pages alive). */
static void restore_catalog(fz_context *ctx, pdf_document *doc, pdf_obj *old, const page_label *labels, int file_pages, const int *order, int count)
{
	pdf_obj *root = pdf_dict_get(ctx, pdf_trailer(ctx, doc), PDF_NAME(Root));
	int n = pdf_count_pages(ctx, doc);
	num_set pages = { 0 }, widgets = { 0 };
	fz_try(ctx)
	{
		for (int i = 0; i < n; i++)
		{
			pdf_obj *page = pdf_lookup_page_obj(ctx, doc, i);
			pdf_obj *annots = pdf_dict_get(ctx, page, PDF_NAME(Annots));
			num_set_add(ctx, &pages, pdf_to_num(ctx, page));
			for (int k = 0; k < pdf_array_len(ctx, annots); k++)
			{
				pdf_obj *a = pdf_array_get(ctx, annots, k);
				if (pdf_name_eq(ctx, pdf_dict_get(ctx, a, PDF_NAME(Subtype)), PDF_NAME(Widget)))
					num_set_add(ctx, &widgets, pdf_to_num(ctx, a));
			}
		}
		for (int i = 0; i < pdf_dict_len(ctx, old); i++)
		{
			pdf_obj *key = pdf_dict_get_key(ctx, old, i);
			pdf_obj *val = pdf_dict_get_val(ctx, old, i);
			/* What pdf_rearrange_pages rebuilt or dropped on purpose (bookmarks and
			 * destinations of deleted pages) stays as it left it. */
			if (pdf_dict_get(ctx, root, key) || pdf_name_eq(ctx, key, PDF_NAME(PageLabels)) || pdf_name_eq(ctx, key, PDF_NAME(Outlines))
				|| pdf_name_eq(ctx, key, PDF_NAME(Dests)) || pdf_name_eq(ctx, key, PDF_NAME(OCProperties))
				|| pdf_name_eq(ctx, key, PDF_NAME(StructTreeRoot)) || pdf_name_eq(ctx, key, PDF_NAME(Pages)))
				continue;
			if (pdf_name_eq(ctx, key, PDF_NAME(Names)))
			{
				/* Dests were rebuilt; the other name trees (attachments, scripts…) come back. */
				pdf_obj *names = pdf_dict_get(ctx, root, PDF_NAME(Names));
				for (int k = 0; k < pdf_dict_len(ctx, val); k++)
				{
					pdf_obj *tk = pdf_dict_get_key(ctx, val, k);
					if (pdf_name_eq(ctx, tk, PDF_NAME(Dests)))
						continue;
					if (!names)
						names = pdf_dict_put_dict(ctx, root, PDF_NAME(Names), 2);
					if (!pdf_dict_get(ctx, names, tk))
						pdf_dict_put(ctx, names, tk, pdf_dict_get_val(ctx, val, k));
				}
				continue;
			}
			if (!strcmp(pdf_to_name(ctx, key), "OpenAction") && !dest_kept(ctx, val, &pages))
				continue;
			if (pdf_name_eq(ctx, key, PDF_NAME(AcroForm)))
			{
				pdf_obj *fields = pdf_dict_get(ctx, val, PDF_NAME(Fields));
				pdf_obj *co = pdf_dict_get(ctx, val, PDF_NAME(CO));
				for (int k = pdf_array_len(ctx, fields) - 1; k >= 0; k--)
					if (!field_kept(ctx, pdf_array_get(ctx, fields, k), &widgets, 0))
						pdf_array_delete(ctx, fields, k);
				/* Calculation order: only fields still there. */
				for (int k = pdf_array_len(ctx, co) - 1; k >= 0; k--)
				{
					pdf_obj *f = pdf_array_get(ctx, co, k);
					if (!num_set_has(&widgets, pdf_to_num(ctx, f)) && !pdf_array_len(ctx, pdf_dict_get(ctx, f, PDF_NAME(Kids))))
						pdf_array_delete(ctx, co, k);
				}
			}
			pdf_dict_put(ctx, root, key, val);
		}
		if (pdf_dict_get(ctx, old, PDF_NAME(PageLabels)))
			write_labels(ctx, root, labels, file_pages, order, count);
	}
	fz_always(ctx)
	{
		fz_free(ctx, pages.v);
		fz_free(ctx, widgets.v);
	}
	fz_catch(ctx)
		fz_rethrow(ctx);
}

int fo_pdf_apply_page_edits(fo_ctx *c, fo_doc *d, const int *rot, int npages, const int *order, int count, int file_pages)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int identity = 1;
	pdf_obj *old = NULL;
	page_label *labels = NULL;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(old);
	fz_var(labels);
	/* With the undo journal on, every change must be inside an operation. The
	 * document is reopened after saving, so this step is never undone. */
	pdf_begin_operation(CTX, pdf, "Page edits");
	fz_try(CTX)
	{
		int n = pdf_count_pages(CTX, pdf);
		if (npages != n)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "page count changed (%d vs %d)", npages, n);
		for (int i = 0; i < count; i++)
			if (order[i] < 0 || order[i] >= n)
				fz_throw(CTX, FZ_ERROR_ARGUMENT, "no page %d", order[i]);
		/* Rotations first: they refer to pages by their original position. */
		for (int i = 0; i < n; i++)
		{
			int extra = ((rot[i] % 360) + 360) % 360;
			if (extra)
			{
				pdf_obj *page = pdf_lookup_page_obj(CTX, pdf, i);
				int cur = pdf_to_int(CTX, pdf_dict_get_inheritable(CTX, page, PDF_NAME(Rotate)));
				pdf_dict_put_int(CTX, page, PDF_NAME(Rotate), (((cur + extra) % 360) + 360) % 360);
			}
		}
		if (count != n)
			identity = 0;
		for (int i = 0; identity && i < count; i++)
			if (order[i] != i)
				identity = 0;
		if (!identity)
		{
			pdf_obj *root = pdf_dict_get(CTX, pdf_trailer(CTX, pdf), PDF_NAME(Root));
			old = pdf_copy_dict(CTX, root);
			if (pdf_dict_get(CTX, old, PDF_NAME(PageLabels)))
				labels = read_labels(CTX, pdf, pdf_dict_get(CTX, old, PDF_NAME(PageLabels)), n);
			pdf_rearrange_pages(CTX, pdf, count, order, PDF_CLEAN_STRUCTURE_KEEP);
			restore_catalog(CTX, pdf, old, labels, file_pages, order, count);
		}
	}
	fz_always(CTX)
	{
		pdf_end_operation(CTX, pdf);
		pdf_drop_obj(CTX, old);
		fz_free(CTX, labels);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

int fo_pdf_can_save_incrementally(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int ok = 0;
	if (!pdf)
		return 0;
	fz_try(CTX)
		ok = pdf_can_be_saved_incrementally(CTX, pdf);
	fz_catch(CTX)
		ok = 0;
	return ok;
}

int fo_pdf_save(fo_ctx *c, fo_doc *d, const char *path, int incremental)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_write_options opts = pdf_default_write_options;
	/* Compress streams that aren't yet (pictures and appearances we added).
	 * Streams that already are, such as scans, are copied as they are. */
	opts.do_compress = 1;
	opts.do_compress_images = 1;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	if (incremental == 1)
		opts.do_incremental = 1;
	else if (incremental == 2)
	{
		/* A plain copy: no clean-up, so the document in memory is left as it is. */
	}
	else
	{
		/* Full rewrite: drop objects no longer used (e.g. deleted pages).
		 * Existing streams are copied as they are, so scans aren't re-encoded. */
		opts.do_garbage = 1;
	}
	fz_try(CTX)
	{
		char *p = os_path(CTX, path);
		fz_try(CTX)
			pdf_save_document(CTX, pdf, p, &opts);
		fz_always(CTX)
			fz_free(CTX, p);
		fz_catch(CTX)
			fz_rethrow(CTX);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

int fo_convert_to_pdf(fo_ctx *c, fo_doc *d, const char *path, const int *order, int count, const int *rot)
{
	fz_document_writer *w = NULL;
	fz_page *page = NULL;
	fz_var(w);
	fz_var(page);
	fz_try(CTX)
	{
	{
		char *p = os_path(CTX, path);
		fz_try(CTX)
			w = fz_new_pdf_writer(CTX, p, "");
		fz_always(CTX)
			fz_free(CTX, p);
		fz_catch(CTX)
			fz_rethrow(CTX);
	}
		for (int k = 0; k < count; k++)
		{
			fz_rect b, box;
			fz_matrix m;
			fz_device *dev;
			page = fz_load_page(CTX, (fz_document *)d, order[k]);
			b = fz_bound_page(CTX, page);
			m = page_matrix(b, 1, rot ? rot[order[k]] : 0);
			box = fz_transform_rect(b, m);
			dev = fz_begin_page(CTX, w, box);
			fz_run_page(CTX, page, dev, m, NULL);
			fz_end_page(CTX, w);
			fz_drop_page(CTX, page);
			page = NULL;
		}
		fz_close_document_writer(CTX, w);
	}
	fz_always(CTX)
	{
		fz_drop_page(CTX, page);
		fz_drop_document_writer(CTX, w);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- inserting pages */

/* Link annotations that jump to a page of their own document: after copying,
 * that page isn't there (or is somewhere else). */
static int is_internal_link(fz_context *ctx, pdf_obj *annot)
{
	pdf_obj *a;
	if (!pdf_name_eq(ctx, pdf_dict_get(ctx, annot, PDF_NAME(Subtype)), PDF_NAME(Link)))
		return 0;
	if (pdf_dict_get(ctx, annot, PDF_NAME(Dest)))
		return 1;
	a = pdf_dict_get(ctx, annot, PDF_NAME(A));
	return pdf_name_eq(ctx, pdf_dict_get(ctx, a, PDF_NAME(S)), PDF_NAME(GoTo));
}

/* Where an internal link goes: the index in pages[] of its target page (with
 * the view in *view, a new array in dst without the page), or -1 when its
 * page isn't among those copied. */
static int link_target(fz_context *ctx, pdf_document *dst, pdf_document *src, pdf_obj *a, const int *pages, int count, pdf_obj **view)
{
	pdf_obj *dest = pdf_dict_get(ctx, a, PDF_NAME(Dest)), *first;
	int t;
	if (!dest)
		dest = pdf_dict_get(ctx, pdf_dict_get(ctx, a, PDF_NAME(A)), PDF_NAME(D));
	if (pdf_is_name(ctx, dest) || pdf_is_string(ctx, dest))
		dest = pdf_lookup_dest(ctx, src, dest);
	if (pdf_is_dict(ctx, dest))
		dest = pdf_dict_get(ctx, dest, PDF_NAME(D));
	if (!pdf_is_array(ctx, dest))
		return -1;
	first = pdf_array_get(ctx, dest, 0);
	t = pdf_is_int(ctx, first) ? pdf_to_int(ctx, first) : pdf_lookup_page_number(ctx, src, first);
	for (int k = 0; k < count; k++)
		if (pages[k] == t)
		{
			*view = pdf_new_array(ctx, dst, 4);
			for (int i = 1; i < pdf_array_len(ctx, dest); i++)
			{
				pdf_obj *v = pdf_array_get(ctx, dest, i);
				if (pdf_is_number(ctx, v))
					pdf_array_push_real(ctx, *view, pdf_to_real(ctx, v));
				else if (pdf_is_name(ctx, v))
					pdf_array_push(ctx, *view, v);
				else
					pdf_array_push(ctx, *view, PDF_NULL);
			}
			return k;
		}
	return -1;
}

/* Drops the kids of a form field that have no widget on the copied pages, so
 * copying the field doesn't pull in widgets (and through them, pages) from
 * the rest of the document. Returns 1 if the field is kept. */
static int prune_field(fz_context *ctx, pdf_obj *field, const num_set *keep, int depth)
{
	pdf_obj *kids = pdf_dict_get(ctx, field, PDF_NAME(Kids));
	int kept = num_set_has(keep, pdf_to_num(ctx, field));
	if (depth > 32 || !pdf_is_array(ctx, kids))
		return kept;
	for (int i = pdf_array_len(ctx, kids) - 1; i >= 0; i--)
		if (!prune_field(ctx, pdf_array_get(ctx, kids, i), keep, depth + 1))
			pdf_array_delete(ctx, kids, i);
	return kept || pdf_array_len(ctx, kids) > 0;
}

static pdf_obj *acroform(fz_context *ctx, pdf_document *doc, int create)
{
	pdf_obj *root = pdf_dict_get(ctx, pdf_trailer(ctx, doc), PDF_NAME(Root));
	pdf_obj *form = pdf_dict_get(ctx, root, PDF_NAME(AcroForm));
	if (!form && create)
		form = pdf_dict_put_dict(ctx, root, PDF_NAME(AcroForm), 4);
	return form;
}

/* Copies PDF pages with their annotations and form fields. */
static void insert_pdf_pages(fz_context *ctx, pdf_document *dst, pdf_document *src, const int *pages, int count)
{
	static pdf_obj * const copy_list[] = {
		PDF_NAME(Contents), PDF_NAME(Resources), PDF_NAME(MediaBox), PDF_NAME(CropBox), PDF_NAME(BleedBox),
		PDF_NAME(TrimBox), PDF_NAME(ArtBox), PDF_NAME(Rotate), PDF_NAME(UserUnit), PDF_NAME(Group),
	};
	int total = pdf_count_pages(ctx, src);
	int base = pdf_count_pages(ctx, dst);
	num_set widgets = { 0 }, tops = { 0 };
	pdf_graft_map *map = NULL;
	pdf_obj *page_dict = NULL, *ref = NULL, *annots = NULL, *field_objs = NULL, *links = NULL, *view = NULL;
	fz_var(links);
	fz_var(view);
	fz_var(map);
	fz_var(page_dict);
	fz_var(ref);
	fz_var(annots);
	fz_var(field_objs);
	fz_try(ctx)
	{
		/* Annotations point at their page (/P); copying one would copy that page
		 * too. src is a throwaway copy, so those links are cut here. */
		for (int i = 0; i < total; i++)
		{
			pdf_obj *list = pdf_dict_get(ctx, pdf_lookup_page_obj(ctx, src, i), PDF_NAME(Annots));
			for (int k = 0; k < pdf_array_len(ctx, list); k++)
			{
				pdf_obj *a = pdf_array_get(ctx, list, k);
				pdf_dict_del(ctx, a, PDF_NAME(P));
				pdf_dict_del(ctx, a, PDF_NAME(Popup));
			}
		}
		/* The fields that have widgets on the copied pages, trimmed to those widgets. */
		field_objs = pdf_new_array(ctx, src, 8);
		for (int k = 0; k < count; k++)
		{
			pdf_obj *list = pdf_dict_get(ctx, pdf_lookup_page_obj(ctx, src, pages[k]), PDF_NAME(Annots));
			for (int j = 0; j < pdf_array_len(ctx, list); j++)
			{
				pdf_obj *a = pdf_array_get(ctx, list, j), *top = a;
				if (!pdf_name_eq(ctx, pdf_dict_get(ctx, a, PDF_NAME(Subtype)), PDF_NAME(Widget)))
					continue;
				num_set_add(ctx, &widgets, pdf_to_num(ctx, a));
				for (int depth = 0; depth < 32 && pdf_dict_get(ctx, top, PDF_NAME(Parent)); depth++)
					top = pdf_dict_get(ctx, top, PDF_NAME(Parent));
				if (!num_set_has(&tops, pdf_to_num(ctx, top)))
				{
					num_set_add(ctx, &tops, pdf_to_num(ctx, top));
					pdf_array_push(ctx, field_objs, top);
				}
			}
		}
		for (int i = 0; i < pdf_array_len(ctx, field_objs); i++)
			prune_field(ctx, pdf_array_get(ctx, field_objs, i), &widgets, 0);

		map = pdf_new_graft_map(ctx, dst);
		/* Links to copied pages: (link, index in pages[], view) triples, pointed at the new pages at the end. */
		links = pdf_new_array(ctx, dst, 12);
		for (int k = 0; k < count; k++)
		{
			pdf_obj *from = pdf_lookup_page_obj(ctx, src, pages[k]);
			pdf_obj *list = pdf_dict_get(ctx, from, PDF_NAME(Annots));
			page_dict = pdf_new_dict(ctx, dst, 12);
			pdf_dict_put(ctx, page_dict, PDF_NAME(Type), PDF_NAME(Page));
			for (int i = 0; i < (int)nelem(copy_list); i++)
			{
				pdf_obj *obj = pdf_dict_get_inheritable(ctx, from, copy_list[i]);
				if (obj)
					pdf_dict_put_drop(ctx, page_dict, copy_list[i], pdf_graft_mapped_object(ctx, map, obj));
			}
			ref = pdf_add_object(ctx, dst, page_dict);
			if (pdf_array_len(ctx, list))
			{
				annots = pdf_new_array(ctx, dst, pdf_array_len(ctx, list));
				for (int j = 0; j < pdf_array_len(ctx, list); j++)
				{
					pdf_obj *a = pdf_array_get(ctx, list, j), *copy;
					int to = -1;
					if (pdf_name_eq(ctx, pdf_dict_get(ctx, a, PDF_NAME(Subtype)), PDF_NAME(Popup)))
						continue;
					if (is_internal_link(ctx, a))
					{
						/* Kept only if its page came along; its target is set once that page is in. */
						to = link_target(ctx, dst, src, a, pages, count, &view);
						if (to < 0)
							continue;
						pdf_dict_del(ctx, a, PDF_NAME(Dest));
						pdf_dict_del(ctx, a, PDF_NAME(A));
					}
					copy = pdf_graft_mapped_object(ctx, map, a);
					pdf_dict_put(ctx, copy, PDF_NAME(P), ref);
					if (to >= 0)
					{
						pdf_array_push(ctx, links, copy);
						pdf_array_push_int(ctx, links, to);
						pdf_array_push_drop(ctx, links, view);
						view = NULL;
					}
					pdf_array_push_drop(ctx, annots, copy);
				}
				pdf_dict_put(ctx, ref, PDF_NAME(Annots), annots);
				pdf_drop_obj(ctx, annots);
				annots = NULL;
			}
			pdf_insert_page(ctx, dst, -1, ref);
			pdf_drop_obj(ctx, ref);
			ref = NULL;
			pdf_drop_obj(ctx, page_dict);
			page_dict = NULL;
		}

		for (int i = 0; i + 2 < pdf_array_len(ctx, links); i += 3)
		{
			pdf_obj *v = pdf_array_get(ctx, links, i + 2);
			pdf_obj *dest = pdf_dict_put_array(ctx, pdf_array_get(ctx, links, i), PDF_NAME(Dest), 1 + pdf_array_len(ctx, v));
			pdf_array_push(ctx, dest, pdf_lookup_page_obj(ctx, dst, base + pdf_array_get_int(ctx, links, i + 1)));
			for (int k = 0; k < pdf_array_len(ctx, v); k++)
				pdf_array_push(ctx, dest, pdf_array_get(ctx, v, k));
		}

		/* Register the copied fields with this document's form. */
		if (pdf_array_len(ctx, field_objs))
		{
			pdf_obj *sform = acroform(ctx, src, 0);
			pdf_obj *form = acroform(ctx, dst, 1);
			pdf_obj *fields = pdf_dict_get(ctx, form, PDF_NAME(Fields));
			pdf_obj *sfonts = pdf_dict_getp(ctx, sform, "DR/Font");
			pdf_obj *da = pdf_dict_get(ctx, sform, PDF_NAME(DA));
			if (!pdf_is_array(ctx, fields))
				fields = pdf_dict_put_array(ctx, form, PDF_NAME(Fields), 8);
			for (int i = 0; i < pdf_array_len(ctx, field_objs); i++)
			{
				pdf_obj *copy = pdf_graft_mapped_object(ctx, map, pdf_array_get(ctx, field_objs, i));
				const char *name = pdf_dict_get_text_string(ctx, copy, PDF_NAME(T));
				fz_try(ctx)
				{
					/* A field with the same name would share its value: rename. */
					if (name[0])
					{
						char base[200], next[220];
						int taken = 1;
						fz_strlcpy(base, name, sizeof base);
						fz_strlcpy(next, base, sizeof next);
						for (int n = 2; taken && n < 10000; )
						{
							taken = 0;
							for (int f = 0; f < pdf_array_len(ctx, fields); f++)
								if (!strcmp(pdf_dict_get_text_string(ctx, pdf_array_get(ctx, fields, f), PDF_NAME(T)), next))
									taken = 1;
							if (taken)
								fz_snprintf(next, sizeof next, "%s_%d", base, n++);
						}
						if (strcmp(next, base))
							pdf_dict_put_text_string(ctx, copy, PDF_NAME(T), next);
					}
					if (da && !pdf_dict_get(ctx, copy, PDF_NAME(DA)))
						pdf_dict_put_drop(ctx, copy, PDF_NAME(DA), pdf_graft_mapped_object(ctx, map, da));
					pdf_array_push(ctx, fields, copy);
				}
				fz_always(ctx)
					pdf_drop_obj(ctx, copy);
				fz_catch(ctx)
					fz_rethrow(ctx);
			}
			/* Fonts the fields' text is drawn with. */
			if (pdf_is_dict(ctx, sfonts))
			{
				pdf_obj *dr = pdf_dict_get(ctx, form, PDF_NAME(DR));
				pdf_obj *fonts;
				if (!pdf_is_dict(ctx, dr))
					dr = pdf_dict_put_dict(ctx, form, PDF_NAME(DR), 1);
				fonts = pdf_dict_get(ctx, dr, PDF_NAME(Font));
				if (!pdf_is_dict(ctx, fonts))
					fonts = pdf_dict_put_dict(ctx, dr, PDF_NAME(Font), 4);
				for (int i = 0; i < pdf_dict_len(ctx, sfonts); i++)
				{
					pdf_obj *key = pdf_dict_get_key(ctx, sfonts, i);
					if (!pdf_dict_get(ctx, fonts, key))
						pdf_dict_put_drop(ctx, fonts, key, pdf_graft_mapped_object(ctx, map, pdf_dict_get_val(ctx, sfonts, i)));
				}
			}
			if (da && !pdf_dict_get(ctx, form, PDF_NAME(DA)))
				pdf_dict_put_drop(ctx, form, PDF_NAME(DA), pdf_graft_mapped_object(ctx, map, da));
		}
	}
	fz_always(ctx)
	{
		pdf_drop_obj(ctx, annots);
		pdf_drop_obj(ctx, ref);
		pdf_drop_obj(ctx, page_dict);
		pdf_drop_obj(ctx, field_objs);
		pdf_drop_obj(ctx, links);
		pdf_drop_obj(ctx, view);
		pdf_drop_graft_map(ctx, map);
		fz_free(ctx, widgets.v);
		fz_free(ctx, tops.v);
	}
	fz_catch(ctx)
		fz_rethrow(ctx);
}

/* Draws pages of any other format (pictures, EPUB, XPS…) into new PDF pages. */
static void insert_drawn_pages(fz_context *ctx, pdf_document *dst, fz_document *src, const int *pages, int count, const float fit[2])
{
	fz_page *page = NULL;
	fz_device *dev = NULL;
	pdf_obj *res = NULL, *ref = NULL;
	fz_buffer *contents = NULL;
	fz_var(page);
	fz_var(dev);
	fz_var(res);
	fz_var(ref);
	fz_var(contents);
	fz_try(ctx)
	{
		for (int k = 0; k < count; k++)
		{
			fz_rect b, box;
			fz_matrix m;
			float bw, bh;
			page = fz_load_page(ctx, src, pages[k]);
			b = fz_bound_page(ctx, page);
			bw = b.x1 - b.x0;
			bh = b.y1 - b.y0;
			m = fz_translate(-b.x0, -b.y0);
			box = fz_make_rect(0, 0, bw, bh);
			if (fit && fit[0] > 0 && fit[1] > 0 && bw > 0 && bh > 0)
			{
				/* The page turned to the picture's orientation, the picture centred in it. */
				float w = fit[0], h = fit[1], s;
				if ((bw > bh) != (w > h))
				{
					float t = w;
					w = h;
					h = t;
				}
				s = fz_min(w / bw, h / bh);
				m = fz_concat(m, fz_scale(s, s));
				m = fz_concat(m, fz_translate((w - bw * s) / 2, (h - bh * s) / 2));
				box = fz_make_rect(0, 0, w, h);
			}
			dev = pdf_page_write(ctx, dst, box, &res, &contents);
			fz_run_page(ctx, page, dev, m, NULL);
			fz_close_device(ctx, dev);
			fz_drop_device(ctx, dev);
			dev = NULL;
			ref = pdf_add_page(ctx, dst, box, 0, res, contents);
			pdf_insert_page(ctx, dst, -1, ref);
			pdf_drop_obj(ctx, ref);
			ref = NULL;
			pdf_drop_obj(ctx, res);
			res = NULL;
			fz_drop_buffer(ctx, contents);
			contents = NULL;
			fz_drop_page(ctx, page);
			page = NULL;
		}
	}
	fz_always(ctx)
	{
		fz_drop_device(ctx, dev);
		pdf_drop_obj(ctx, ref);
		pdf_drop_obj(ctx, res);
		fz_drop_buffer(ctx, contents);
		fz_drop_page(ctx, page);
	}
	fz_catch(ctx)
		fz_rethrow(ctx);
}

int fo_pdf_insert_pages(fo_ctx *c, fo_doc *d, fo_doc *s, const int *pages, int count, const float fit[2])
{
	pdf_document *dst = pdf_specifics(CTX, (fz_document *)d);
	pdf_document *src = pdf_specifics(CTX, (fz_document *)s);
	if (!dst)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_try(CTX)
	{
		int n = fz_count_pages(CTX, (fz_document *)s);
		for (int k = 0; k < count; k++)
			if (pages[k] < 0 || pages[k] >= n)
				fz_throw(CTX, FZ_ERROR_ARGUMENT, "page %d is out of range", pages[k] + 1);
		if (src)
			insert_pdf_pages(CTX, dst, src, pages, count);
		else
			insert_drawn_pages(CTX, dst, (fz_document *)s, pages, count, fit);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return count;
}

int fo_pdf_insert_blank(fo_ctx *c, fo_doc *d, float w, float h, int count)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *res = NULL, *ref = NULL;
	fz_buffer *contents = NULL;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(res);
	fz_var(ref);
	fz_var(contents);
	fz_try(CTX)
	{
		for (int k = 0; k < count; k++)
		{
			res = pdf_new_dict(CTX, pdf, 1);
			contents = fz_new_buffer(CTX, 1);
			ref = pdf_add_page(CTX, pdf, fz_make_rect(0, 0, w, h), 0, res, contents);
			pdf_insert_page(CTX, pdf, -1, ref);
			pdf_drop_obj(CTX, ref);
			ref = NULL;
			pdf_drop_obj(CTX, res);
			res = NULL;
			fz_drop_buffer(CTX, contents);
			contents = NULL;
		}
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, ref);
		pdf_drop_obj(CTX, res);
		fz_drop_buffer(CTX, contents);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return count;
}

fo_doc *fo_pdf_new(fo_ctx *c)
{
	pdf_document *pdf = NULL;
	fz_try(CTX)
		pdf = pdf_create_document(CTX);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return (fo_doc *)pdf;
}

/* Adds an outline item as the last child of parent (the /Outlines root or an
 * item). New items are closed; the root counts its visible children. */
static pdf_obj *outline_add(fz_context *ctx, pdf_document *doc, pdf_obj *parent, int is_root, const char *title, int page, float y)
{
	pdf_obj *item = pdf_add_new_dict(ctx, doc, 6);
	pdf_obj *last = pdf_dict_get(ctx, parent, PDF_NAME(Last));
	fz_try(ctx)
	{
		pdf_dict_put_text_string(ctx, item, PDF_NAME(Title), title ? title : "");
		pdf_dict_put(ctx, item, PDF_NAME(Parent), parent);
		if (page >= 0)
		{
			pdf_obj *pobj = pdf_lookup_page_obj(ctx, doc, page);
			fz_rect mb = pdf_to_rect(ctx, pdf_dict_get_inheritable(ctx, pobj, PDF_NAME(MediaBox)));
			pdf_obj *dest = pdf_dict_put_array(ctx, item, PDF_NAME(Dest), 5);
			pdf_array_push(ctx, dest, pobj);
			pdf_array_push(ctx, dest, PDF_NAME(XYZ));
			pdf_array_push(ctx, dest, PDF_NULL);
			if (y > 0 && y < mb.y1 - mb.y0)
				pdf_array_push_real(ctx, dest, mb.y1 - y);
			else
				pdf_array_push(ctx, dest, PDF_NULL);
			pdf_array_push(ctx, dest, PDF_NULL);
		}
		if (last)
		{
			pdf_dict_put(ctx, last, PDF_NAME(Next), item);
			pdf_dict_put(ctx, item, PDF_NAME(Prev), last);
		}
		else
			pdf_dict_put(ctx, parent, PDF_NAME(First), item);
		pdf_dict_put(ctx, parent, PDF_NAME(Last), item);
		pdf_dict_put_int(ctx, parent, PDF_NAME(Count), pdf_dict_get_int(ctx, parent, PDF_NAME(Count)) + (is_root ? 1 : -1));
	}
	fz_catch(ctx)
	{
		pdf_drop_obj(ctx, item);
		fz_rethrow(ctx);
	}
	return item;
}

/* Where a source outline entry lands: its page in the new document, or -1. */
static int outline_target(fz_context *ctx, fz_document *src, fz_outline *o, const int *map, int n)
{
	int p;
	if (!o->uri && o->page.page < 0)
		return -1;
	p = fz_page_number_from_location(ctx, src, o->page);
	return p >= 0 && p < n ? map[p] : -1;
}

/* The first kept page among an entry's descendants, or -1. */
static int first_kept_target(fz_context *ctx, fz_document *src, fz_outline *o, const int *map, int n, int depth)
{
	for (; o && depth < 32; o = o->next)
	{
		int t = outline_target(ctx, src, o, map, n);
		if (t < 0)
			t = first_kept_target(ctx, src, o->down, map, n, depth + 1);
		if (t >= 0)
			return t;
	}
	return -1;
}

/* Copies source outline entries whose pages were kept. An entry whose own page
 * was left out stays if something under it was kept (it then goes there). */
static void copy_outline(fz_context *ctx, pdf_document *dst, pdf_obj *parent, int is_root, fz_document *src, fz_outline *o, const int *map, int n, int depth)
{
	for (; o && depth < 32; o = o->next)
	{
		int t = outline_target(ctx, src, o, map, n);
		float y = o->y;
		pdf_obj *item;
		if (t < 0)
		{
			t = first_kept_target(ctx, src, o->down, map, n, depth + 1);
			y = 0;
		}
		if (t < 0)
			continue;
		item = outline_add(ctx, dst, parent, is_root, o->title, t, y);
		fz_try(ctx)
			copy_outline(ctx, dst, item, 0, src, o->down, map, n, depth + 1);
		fz_always(ctx)
			pdf_drop_obj(ctx, item);
		fz_catch(ctx)
			fz_rethrow(ctx);
	}
}

int fo_pdf_outline_group(fo_ctx *c, fo_doc *d, fo_doc *s, const char *title, int base, const int *pages, int count)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	fz_outline *outline = NULL;
	int *map = NULL;
	pdf_obj *group = NULL;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(outline);
	fz_var(map);
	fz_var(group);
	fz_try(CTX)
	{
		pdf_obj *root = pdf_dict_get(CTX, pdf_trailer(CTX, pdf), PDF_NAME(Root));
		pdf_obj *outlines = pdf_dict_get(CTX, root, PDF_NAME(Outlines));
		int n = fz_count_pages(CTX, (fz_document *)s);
		map = fz_malloc_array(CTX, n > 0 ? n : 1, int);
		for (int i = 0; i < n; i++)
			map[i] = -1;
		for (int k = count - 1; k >= 0; k--)
			if (pages[k] >= 0 && pages[k] < n)
				map[pages[k]] = base + k;
		if (!outlines)
		{
			outlines = pdf_add_new_dict(CTX, pdf, 4);
			pdf_dict_put(CTX, outlines, PDF_NAME(Type), PDF_NAME(Outlines));
			pdf_dict_put(CTX, root, PDF_NAME(Outlines), outlines);
			pdf_drop_obj(CTX, outlines);
		}
		group = outline_add(CTX, pdf, outlines, 1, title, count ? base : -1, 0);
		fz_try(CTX)
			outline = fz_load_outline(CTX, (fz_document *)s);
		fz_catch(CTX)
			outline = NULL; /* a broken outline just isn't copied */
		copy_outline(CTX, pdf, group, 0, (fz_document *)s, outline, map, n, 0);
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, group);
		fz_drop_outline(CTX, outline);
		fz_free(CTX, map);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- splitting by size */

typedef struct
{
	pdf_document *doc;
	int *page_stamp, *part_stamp; /* per object number: last page / part that counted it */
	int n, page, part;
	int64_t added;
} size_walk;

/* Adds the bytes of objects a page uses that its part doesn't have yet. Other
 * pages (reached through links, parents or annotations' /P) are not followed. */
static void walk_size(fz_context *ctx, size_walk *w, pdf_obj *o, int depth)
{
	if (depth > 64 || !o)
		return;
	if (pdf_is_indirect(ctx, o))
	{
		int num = pdf_to_num(ctx, o);
		if (num <= 0 || num >= w->n || w->page_stamp[num] == w->page)
			return;
		w->page_stamp[num] = w->page;
		if (w->part_stamp[num] != w->part)
		{
			w->part_stamp[num] = w->part;
			w->added += 40; /* object header and xref entry */
			if (pdf_is_stream(ctx, o))
				w->added += pdf_dict_get_int(ctx, o, PDF_NAME(Length));
		}
		if (depth > 0 && pdf_name_eq(ctx, pdf_dict_get(ctx, o, PDF_NAME(Type)), PDF_NAME(Page)))
			return;
	}
	if (pdf_is_dict(ctx, o))
	{
		for (int i = 0; i < pdf_dict_len(ctx, o); i++)
		{
			pdf_obj *key = pdf_dict_get_key(ctx, o, i);
			if (pdf_name_eq(ctx, key, PDF_NAME(Parent)) || pdf_name_eq(ctx, key, PDF_NAME(P)))
				continue;
			walk_size(ctx, w, pdf_dict_get_val(ctx, o, i), depth + 1);
		}
	}
	else if (pdf_is_array(ctx, o))
	{
		for (int i = 0; i < pdf_array_len(ctx, o); i++)
			walk_size(ctx, w, pdf_array_get(ctx, o, i), depth + 1);
	}
}

int fo_pdf_split_by_size(fo_ctx *c, fo_doc *d, const int *order, int count, double max_bytes, int *starts)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	size_walk w = { 0 };
	int parts = 0;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(w);
	fz_try(CTX)
	{
		int64_t part_bytes = 0;
		w.doc = pdf;
		w.n = pdf_xref_len(CTX, pdf);
		w.page_stamp = fz_calloc(CTX, w.n, sizeof(int));
		w.part_stamp = fz_calloc(CTX, w.n, sizeof(int));
		w.part = 1;
		for (int k = 0; k < count; k++)
		{
			w.page = k + 1;
			w.added = 0;
			walk_size(CTX, &w, pdf_lookup_page_obj(CTX, pdf, order[k]), 0);
			if (k > 0 && part_bytes > 0 && part_bytes + w.added > max_bytes)
			{
				/* Start a new part with this page: count it again from scratch. */
				starts[parts++] = k;
				w.part++;
				w.page = count + k + 1;
				w.added = 0;
				walk_size(CTX, &w, pdf_lookup_page_obj(CTX, pdf, order[k]), 0);
				part_bytes = 0;
			}
			else if (k == 0)
				starts[parts++] = 0;
			part_bytes += w.added;
		}
	}
	fz_always(CTX)
	{
		fz_free(CTX, w.page_stamp);
		fz_free(CTX, w.part_stamp);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return parts;
}

/* ---------------------------------------------------------------- page size (crop and resize) */

/* Where the page's content is: the box around everything drawn (page
 * coordinates, relative to its top-left), or an empty box on a blank page. */
int fo_page_content_box(fo_ctx *c, fo_page *p, float out[4])
{
	fz_rect r = fz_empty_rect;
	fz_device *dev = NULL;
	fz_var(dev);
	fz_try(CTX)
	{
		fz_rect b = fz_bound_page(CTX, (fz_page *)p);
		dev = fz_new_bbox_device(CTX, &r);
		fz_run_page(CTX, (fz_page *)p, dev, fz_translate(-b.x0, -b.y0), NULL);
		fz_close_device(CTX, dev);
		r = fz_intersect_rect(r, fz_make_rect(0, 0, b.x1 - b.x0, b.y1 - b.y0));
	}
	fz_always(CTX)
		fz_drop_device(CTX, dev);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	out[0] = r.x0;
	out[1] = r.y0;
	out[2] = r.x1;
	out[3] = r.y1;
	return fz_is_empty_rect(r) ? 0 : 1;
}

/* Transforms a flat array of x, y pairs in place. */
static void transform_points(fz_context *ctx, pdf_obj *arr, fz_matrix m)
{
	for (int i = 0; i + 1 < pdf_array_len(ctx, arr); i += 2)
	{
		fz_point p = fz_transform_point(fz_make_point(pdf_array_get_real(ctx, arr, i), pdf_array_get_real(ctx, arr, i + 1)), m);
		pdf_array_put_real(ctx, arr, i, p.x);
		pdf_array_put_real(ctx, arr, i + 1, p.y);
	}
}

/* Moves an annotation's geometry (PDF user space) with the page's content. */
static void transform_annot_obj(fz_context *ctx, pdf_obj *obj, fz_matrix m)
{
	pdf_obj *ink = pdf_dict_get(ctx, obj, PDF_NAME(InkList));
	pdf_dict_put_rect(ctx, obj, PDF_NAME(Rect), fz_transform_rect(pdf_dict_get_rect(ctx, obj, PDF_NAME(Rect)), m));
	transform_points(ctx, pdf_dict_get(ctx, obj, PDF_NAME(QuadPoints)), m);
	transform_points(ctx, pdf_dict_get(ctx, obj, PDF_NAME(Vertices)), m);
	transform_points(ctx, pdf_dict_get(ctx, obj, PDF_NAME(L)), m);
	transform_points(ctx, pdf_dict_get(ctx, obj, PDF_NAME(CL)), m);
	for (int i = 0; i < pdf_array_len(ctx, ink); i++)
		transform_points(ctx, pdf_array_get(ctx, ink, i), m);
}

/* Kinds whose look MuPDF draws itself are redrawn after moving; others
 * (stamps, pictures) keep theirs, which viewers fit to the new rectangle. */
static void redraw_moved(fz_context *ctx, pdf_annot *a)
{
	switch (pdf_annot_type(ctx, a))
	{
	case PDF_ANNOT_HIGHLIGHT: case PDF_ANNOT_UNDERLINE: case PDF_ANNOT_STRIKE_OUT: case PDF_ANNOT_SQUIGGLY:
	case PDF_ANNOT_INK: case PDF_ANNOT_SQUARE: case PDF_ANNOT_CIRCLE: case PDF_ANNOT_LINE:
	case PDF_ANNOT_POLYGON: case PDF_ANNOT_POLY_LINE: case PDF_ANNOT_FREE_TEXT: case PDF_ANNOT_WIDGET:
		pdf_dirty_annot(ctx, a);
		pdf_update_annot(ctx, a);
		break;
	default:
		break;
	}
}

/* Appends a page's content streams, one after the other, to buf. */
static void append_contents(fz_context *ctx, fz_buffer *buf, pdf_obj *contents)
{
	if (pdf_is_array(ctx, contents))
	{
		for (int i = 0; i < pdf_array_len(ctx, contents); i++)
			append_contents(ctx, buf, pdf_array_get(ctx, contents, i));
		return;
	}
	if (pdf_is_stream(ctx, contents))
	{
		fz_buffer *b = pdf_load_stream(ctx, contents);
		fz_try(ctx)
		{
			fz_append_buffer(ctx, buf, b);
			fz_append_byte(ctx, buf, '\n');
		}
		fz_always(ctx)
			fz_drop_buffer(ctx, b);
		fz_catch(ctx)
			fz_rethrow(ctx);
	}
}

/* A w×h RGB picture from pixels of pix (x0, y0 with steps dx, dy per column / row). */
static pdf_obj *strip_image(fz_context *ctx, pdf_document *doc, fz_pixmap *pix, int x0, int y0, int w, int h)
{
	fz_pixmap *strip = fz_new_pixmap(ctx, fz_device_rgb(ctx), w, h, NULL, 0);
	fz_image *img = NULL;
	pdf_obj *ref = NULL;
	fz_var(img);
	fz_try(ctx)
	{
		for (int y = 0; y < h; y++)
			for (int x = 0; x < w; x++)
			{
				int sx = fz_clampi(x0 + x, 0, pix->w - 1), sy = fz_clampi(y0 + y, 0, pix->h - 1);
				memcpy(strip->samples + y * strip->stride + x * 3, pix->samples + sy * pix->stride + sx * pix->n, 3);
			}
		img = fz_new_image_from_pixmap(ctx, strip, NULL);
		ref = pdf_add_image(ctx, doc, img);
	}
	fz_always(ctx)
	{
		fz_drop_image(ctx, img);
		fz_drop_pixmap(ctx, strip);
	}
	fz_catch(ctx)
		fz_rethrow(ctx);
	return ref;
}

static void put_color_rect(fz_context *ctx, fz_buffer *buf, const unsigned char *rgb, fz_rect r)
{
	if (r.x1 - r.x0 < 0.01f || r.y1 - r.y0 < 0.01f)
		return;
	fz_append_printf(ctx, buf, "%g %g %g rg %g %g %g %g re f\n", rgb[0] / 255.0f, rgb[1] / 255.0f, rgb[2] / 255.0f, r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0);
}

int fo_pdf_page_size(fo_ctx *c, fo_page *p, const float crop[4], const float size[2], int fit, float ax, float ay, int remove, int fill, const float rgb[3])
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	pdf_document *doc = page ? page->doc : NULL;
	fz_buffer *old = NULL, *buf = NULL;
	fz_pixmap *pix = NULL;
	pdf_obj *res = NULL, *xobjs = NULL, *form = NULL, *contents = NULL, *img = NULL;
	if (!page)
	{
		snprintf(c->err, sizeof c->err, "not a PDF page");
		return -1;
	}
	fz_var(old);
	fz_var(buf);
	fz_var(pix);
	fz_var(res);
	fz_var(form);
	fz_var(contents);
	fz_var(img);
	fz_try(CTX)
	{
		pdf_obj *pobj = page->obj;
		fz_rect media, bounds, C;
		fz_matrix ctm, inv;
		pdf_page_transform(CTX, page, &media, &ctm);
		bounds = fz_transform_rect(media, ctm);
		C = fz_intersect_rect(fz_make_rect(crop[0] + bounds.x0, crop[1] + bounds.y0, crop[2] + bounds.x0, crop[3] + bounds.y0), bounds);
		if (C.x1 - C.x0 < 1 || C.y1 - C.y0 < 1)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "the crop box is empty");
		inv = fz_invert_matrix(ctm);

		/* Content outside: deleted (redacted without boxes), and comments fully outside dropped. */
		if (remove)
		{
			fz_rect strips[4] = {
				{ bounds.x0, bounds.y0, C.x0, bounds.y1 }, { C.x1, bounds.y0, bounds.x1, bounds.y1 },
				{ C.x0, bounds.y0, C.x1, C.y0 }, { C.x0, C.y1, C.x1, bounds.y1 },
			};
			pdf_redact_options opts = { 0, PDF_REDACT_IMAGE_PIXELS, PDF_REDACT_LINE_ART_REMOVE_IF_COVERED, PDF_REDACT_TEXT_REMOVE };
			pdf_annot *a, *next;
			pdf_obj *annots = pdf_dict_get(CTX, pobj, PDF_NAME(Annots));
			int any = 0;
			/* pdf_redact_page applies every redaction mark on the page: marks
			 * someone placed and hasn't applied yet are hidden from it meanwhile. */
			for (int i = 0; i < pdf_array_len(CTX, annots); i++)
			{
				pdf_obj *o = pdf_array_get(CTX, annots, i);
				if (pdf_name_eq(CTX, pdf_dict_get(CTX, o, PDF_NAME(Subtype)), PDF_NAME(Redact)))
					pdf_dict_puts_drop(CTX, o, "Subtype", pdf_new_name(CTX, "PaginautKeepRedact"));
			}
			for (a = pdf_first_annot(CTX, page); a; a = next)
			{
				next = pdf_next_annot(CTX, a);
				if (fz_is_empty_rect(fz_intersect_rect(pdf_bound_annot(CTX, a), C)))
					pdf_delete_annot(CTX, page, a);
			}
			for (int i = 0; i < 4; i++)
				if (strips[i].x1 - strips[i].x0 > 0.01f && strips[i].y1 - strips[i].y0 > 0.01f)
				{
					pdf_annot *r = pdf_create_annot(CTX, page, PDF_ANNOT_REDACT);
					pdf_set_annot_rect(CTX, r, strips[i]);
					pdf_drop_annot(CTX, r);
					any = 1;
				}
			if (any)
				pdf_redact_page(CTX, doc, page, &opts);
			annots = pdf_dict_get(CTX, pobj, PDF_NAME(Annots));
			for (int i = 0; i < pdf_array_len(CTX, annots); i++)
			{
				pdf_obj *o = pdf_array_get(CTX, annots, i);
				if (!strcmp(pdf_to_name(CTX, pdf_dict_get(CTX, o, PDF_NAME(Subtype))), "PaginautKeepRedact"))
					pdf_dict_put(CTX, o, PDF_NAME(Subtype), PDF_NAME(Redact));
			}
		}

		pdf_dict_del(CTX, pobj, PDF_NAME(TrimBox));
		pdf_dict_del(CTX, pobj, PDF_NAME(BleedBox));
		pdf_dict_del(CTX, pobj, PDF_NAME(ArtBox));
		if (!(size && size[0] > 0 && size[1] > 0))
		{
			/* Crop only: the page box shrinks, nothing moves. */
			fz_rect cu = fz_transform_rect(C, inv);
			pdf_dict_put_rect(CTX, pobj, PDF_NAME(CropBox), cu);
			if (remove)
				pdf_dict_put_rect(CTX, pobj, PDF_NAME(MediaBox), cu);
		}
		else
		{
			/* Resize: the content (from the crop box) is placed on a W×H page. */
			float W = size[0], H = size[1];
			float cw = C.x1 - C.x0, ch = C.y1 - C.y0;
			float s = fit == 0 ? fz_min(W / cw, H / ch) : fit == 1 ? fz_max(W / cw, H / ch) : 1;
			float pw = cw * s, ph = ch * s, ox = (W - pw) * ax, oy = (H - ph) * ay;
			fz_matrix flip = { 1, 0, 0, -1, 0, H };
			fz_matrix place = fz_concat(fz_concat(fz_translate(-C.x0, -C.y0), fz_scale(s, s)), fz_translate(ox, oy));
			fz_matrix T = fz_concat(fz_concat(ctm, place), flip);
			fz_rect page_box = fz_make_rect(0, 0, W, H);
			fz_rect placed = fz_intersect_rect(fz_transform_rect(fz_make_rect(ox, oy, ox + pw, oy + ph), flip), page_box);
			pdf_obj *group = pdf_dict_get(CTX, pobj, PDF_NAME(Group));
			pdf_annot *a;

			old = fz_new_buffer(CTX, 1024);
			append_contents(CTX, old, pdf_dict_get(CTX, pobj, PDF_NAME(Contents)));
			form = pdf_new_xobject(CTX, doc, media, fz_identity, pdf_dict_get_inheritable(CTX, pobj, PDF_NAME(Resources)), old);
			if (group)
				pdf_dict_put(CTX, form, PDF_NAME(Group), group);
			res = pdf_new_dict(CTX, doc, 2);
			xobjs = pdf_dict_put_dict(CTX, res, PDF_NAME(XObject), 6);
			pdf_dict_puts(CTX, xobjs, "PaginautPage", form);
			buf = fz_new_buffer(CTX, 512);

			/* The new space around the content. */
			if (fill && (placed.x0 > 0.01f || placed.y0 > 0.01f || placed.x1 < W - 0.01f || placed.y1 < H - 0.01f))
			{
				fz_rect top = { placed.x0, placed.y1, placed.x1, H }, bottom = { placed.x0, 0, placed.x1, placed.y0 };
				fz_rect left = { 0, placed.y0, placed.x0, placed.y1 }, right = { placed.x1, placed.y0, W, placed.y1 };
				fz_rect corners[4] = { { 0, placed.y1, placed.x0, H }, { placed.x1, placed.y1, W, H }, { 0, 0, placed.x0, placed.y0 }, { placed.x1, 0, W, placed.y0 } };
				fz_append_string(CTX, buf, "q\n");
				if (fill == 1)
				{
					unsigned char col[3] = { (unsigned char)(rgb[0] * 255 + 0.5f), (unsigned char)(rgb[1] * 255 + 0.5f), (unsigned char)(rgb[2] * 255 + 0.5f) };
					put_color_rect(CTX, buf, col, top);
					put_color_rect(CTX, buf, col, bottom);
					put_color_rect(CTX, buf, col, left);
					put_color_rect(CTX, buf, col, right);
					for (int i = 0; i < 4; i++)
						put_color_rect(CTX, buf, col, corners[i]);
				}
				else
				{
					/* Extend edges: the crop box's outer rows and columns of pixels, stretched outwards. */
					float k = fz_min(2.0f, 1400.0f / fz_max(cw, ch));
					int x0, y0, x1, y1;
					struct { fz_rect r; const char *name; int sx, sy, w, h; } strips[4];
					const unsigned char *tl, *tr, *bl, *br;
					pix = fz_new_pixmap_from_page(CTX, (fz_page *)p, fz_concat(fz_translate(-bounds.x0, -bounds.y0), fz_scale(k, k)), fz_device_rgb(CTX), 0);
					x0 = fz_clampi((int)((C.x0 - bounds.x0) * k), 0, pix->w - 1);
					y0 = fz_clampi((int)((C.y0 - bounds.y0) * k), 0, pix->h - 1);
					x1 = fz_clampi((int)((C.x1 - bounds.x0) * k) - 1, x0, pix->w - 1);
					y1 = fz_clampi((int)((C.y1 - bounds.y0) * k) - 1, y0, pix->h - 1);
					strips[0].r = top; strips[0].name = "PaginautT"; strips[0].sx = x0; strips[0].sy = y0; strips[0].w = x1 - x0 + 1; strips[0].h = 1;
					strips[1].r = bottom; strips[1].name = "PaginautB"; strips[1].sx = x0; strips[1].sy = y1; strips[1].w = x1 - x0 + 1; strips[1].h = 1;
					strips[2].r = left; strips[2].name = "PaginautL"; strips[2].sx = x0; strips[2].sy = y0; strips[2].w = 1; strips[2].h = y1 - y0 + 1;
					strips[3].r = right; strips[3].name = "PaginautR"; strips[3].sx = x1; strips[3].sy = y0; strips[3].w = 1; strips[3].h = y1 - y0 + 1;
					for (int i = 0; i < 4; i++)
					{
						fz_rect r = strips[i].r;
						if (r.x1 - r.x0 < 0.01f || r.y1 - r.y0 < 0.01f)
							continue;
						img = strip_image(CTX, doc, pix, strips[i].sx, strips[i].sy, strips[i].w, strips[i].h);
						pdf_dict_puts(CTX, xobjs, strips[i].name, img);
						pdf_drop_obj(CTX, img);
						img = NULL;
						fz_append_printf(CTX, buf, "q %g 0 0 %g %g %g cm /%s Do Q\n", r.x1 - r.x0, r.y1 - r.y0, r.x0, r.y0, strips[i].name);
					}
					tl = pix->samples + y0 * pix->stride + x0 * pix->n;
					tr = pix->samples + y0 * pix->stride + x1 * pix->n;
					bl = pix->samples + y1 * pix->stride + x0 * pix->n;
					br = pix->samples + y1 * pix->stride + x1 * pix->n;
					put_color_rect(CTX, buf, tl, corners[0]);
					put_color_rect(CTX, buf, tr, corners[1]);
					put_color_rect(CTX, buf, bl, corners[2]);
					put_color_rect(CTX, buf, br, corners[3]);
				}
				fz_append_string(CTX, buf, "Q\n");
			}
			/* The content, clipped to where the crop box went. */
			fz_append_printf(CTX, buf, "q %g %g %g %g re W n %g %g %g %g %g %g cm /PaginautPage Do Q\n",
				placed.x0, placed.y0, placed.x1 - placed.x0, placed.y1 - placed.y0, T.a, T.b, T.c, T.d, T.e, T.f);
			contents = pdf_add_stream(CTX, doc, buf, NULL, 0);

			/* Comments, links and form fields move with the content. */
			{
				pdf_obj *annots = pdf_dict_get(CTX, pobj, PDF_NAME(Annots));
				for (int i = 0; i < pdf_array_len(CTX, annots); i++)
					transform_annot_obj(CTX, pdf_array_get(CTX, annots, i), T);
			}
			pdf_dict_put_rect(CTX, pobj, PDF_NAME(MediaBox), page_box);
			pdf_dict_del(CTX, pobj, PDF_NAME(CropBox));
			pdf_dict_put_int(CTX, pobj, PDF_NAME(Rotate), 0);
			pdf_dict_put(CTX, pobj, PDF_NAME(Resources), res);
			pdf_dict_put(CTX, pobj, PDF_NAME(Contents), contents);
			for (a = pdf_first_annot(CTX, page); a; a = pdf_next_annot(CTX, a))
				redraw_moved(CTX, a);
			for (a = pdf_first_widget(CTX, page); a; a = pdf_next_widget(CTX, a))
				redraw_moved(CTX, a);
		}
	}
	fz_always(CTX)
	{
		fz_drop_buffer(CTX, old);
		fz_drop_buffer(CTX, buf);
		fz_drop_pixmap(CTX, pix);
		pdf_drop_obj(CTX, res);
		pdf_drop_obj(CTX, form);
		pdf_drop_obj(CTX, contents);
		pdf_drop_obj(CTX, img);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- stamps (page numbers, headers, watermarks) */

struct fo_stamp
{
	pdf_page *page;
	float w, h;
	fz_device *dev;
	pdf_obj *res;
	fz_buffer *contents;
};

static const char *stamp_subtype(const char *kind)
{
	return !strcmp(kind, "Watermark") ? "Watermark" : !strcmp(kind, "Background") ? "Background" : "Header";
}

fo_stamp *fo_stamp_begin(fo_ctx *c, fo_page *p)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	fo_stamp *st = NULL;
	if (!page)
	{
		snprintf(c->err, sizeof c->err, "not a PDF page");
		return NULL;
	}
	fz_var(st);
	fz_try(CTX)
	{
		fz_rect b = fz_bound_page(CTX, (fz_page *)p);
		st = fz_malloc_struct(CTX, fo_stamp);
		st->page = page;
		st->w = b.x1 - b.x0;
		st->h = b.y1 - b.y0;
		st->dev = pdf_page_write(CTX, page->doc, fz_make_rect(0, 0, st->w, st->h), &st->res, &st->contents);
	}
	fz_catch(CTX)
	{
		fz_free(CTX, st);
		FAIL();
		return NULL;
	}
	return (fo_stamp *)st;
}

void fo_stamp_drop(fo_ctx *c, fo_stamp *st)
{
	if (!st)
		return;
	fz_try(CTX)
		fz_close_device(CTX, st->dev);
	fz_catch(CTX)
		;
	fz_drop_device(CTX, st->dev);
	pdf_drop_obj(CTX, st->res);
	fz_drop_buffer(CTX, st->contents);
	fz_free(CTX, st);
}

static fz_matrix to_matrix(const float m[6])
{
	return fz_make_matrix(m[0], m[1], m[2], m[3], m[4], m[5]);
}

/* Text laid out by MuPDF's HTML engine (shaping, Indic scripts, fallback fonts)
 * in a box `width` wide. valign: the box's top (0), middle (1) or bottom (2)
 * sits at y = 0 of the box space; m takes box space to page coordinates. */
int fo_stamp_text(fo_ctx *c, fo_stamp *st, const char *html, const char *css, float width, int valign, const float m[6], float opacity)
{
	fz_buffer *buf = NULL;
	fz_story *story = NULL;
	fz_var(buf);
	fz_var(story);
	fz_try(CTX)
	{
		fz_rect filled;
		float hgt;
		fz_matrix ctm;
		buf = fz_new_buffer_from_copied_data(CTX, (const unsigned char *)html, strlen(html));
		story = fz_new_story(CTX, buf, css, 12, NULL);
		fz_place_story(CTX, story, fz_make_rect(0, 0, width, 100000), &filled);
		hgt = filled.y1 - filled.y0;
		ctm = fz_concat(fz_translate(0, -hgt * valign / 2.0f), to_matrix(m));
		if (opacity < 0.999f)
			fz_begin_group(CTX, st->dev, fz_make_rect(0, 0, st->w, st->h), NULL, 0, 0, FZ_BLEND_NORMAL, opacity);
		fz_draw_story(CTX, story, st->dev, ctm);
		if (opacity < 0.999f)
			fz_end_group(CTX, st->dev);
	}
	fz_always(CTX)
	{
		fz_drop_story(CTX, story);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* A picture filling the w×h box at the box origin; m takes box space to page coordinates. */
int fo_stamp_image(fo_ctx *c, fo_stamp *st, const unsigned char *data, size_t len, float w, float h, const float m[6], float opacity)
{
	fz_buffer *buf = NULL;
	fz_image *img = NULL;
	fz_var(buf);
	fz_var(img);
	fz_try(CTX)
	{
		buf = fz_new_buffer_from_copied_data(CTX, data, len);
		img = fz_new_image_from_buffer(CTX, buf);
		fz_fill_image(CTX, st->dev, img, fz_concat(fz_scale(w, h), to_matrix(m)), opacity, fz_default_color_params);
	}
	fz_always(CTX)
	{
		fz_drop_image(CTX, img);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

static pdf_obj *page_own_resources(fz_context *ctx, pdf_document *doc, pdf_obj *pobj)
{
	pdf_obj *res = pdf_dict_get(ctx, pobj, PDF_NAME(Resources));
	if (!res)
	{
		pdf_obj *inherited = pdf_dict_get_inheritable(ctx, pobj, PDF_NAME(Resources));
		res = inherited ? pdf_copy_dict(ctx, inherited) : pdf_new_dict(ctx, doc, 1);
		pdf_dict_put_drop(ctx, pobj, PDF_NAME(Resources), res);
	}
	return res;
}

/* The document's "Watermark" layer (optional content), made on first use. */
static pdf_obj *watermark_layer(fz_context *ctx, pdf_document *doc)
{
	pdf_obj *root = pdf_dict_get(ctx, pdf_trailer(ctx, doc), PDF_NAME(Root));
	pdf_obj *ocp = pdf_dict_get(ctx, root, PDF_NAME(OCProperties));
	pdf_obj *ocgs, *d, *order, *ocg, *usage;
	if (!ocp)
		ocp = pdf_dict_put_dict(ctx, root, PDF_NAME(OCProperties), 2);
	ocgs = pdf_dict_get(ctx, ocp, PDF_NAME(OCGs));
	if (!ocgs)
		ocgs = pdf_dict_put_array(ctx, ocp, PDF_NAME(OCGs), 1);
	for (int i = 0; i < pdf_array_len(ctx, ocgs); i++)
		if (pdf_dict_gets(ctx, pdf_array_get(ctx, ocgs, i), "PaginautLayer"))
			return pdf_array_get(ctx, ocgs, i);
	ocg = pdf_add_new_dict(ctx, doc, 4);
	pdf_dict_put(ctx, ocg, PDF_NAME(Type), PDF_NAME(OCG));
	pdf_dict_put_text_string(ctx, ocg, PDF_NAME(Name), "Watermark");
	pdf_dict_puts_drop(ctx, ocg, "PaginautLayer", PDF_TRUE);
	usage = pdf_dict_put_dict(ctx, ocg, PDF_NAME(Usage), 1);
	{
		pdf_obj *el = pdf_new_dict(ctx, doc, 1);
		pdf_dict_put_name(ctx, el, PDF_NAME(Subtype), "WM");
		pdf_dict_puts_drop(ctx, usage, "PageElement", el);
	}
	pdf_array_push(ctx, ocgs, ocg);
	d = pdf_dict_get(ctx, ocp, PDF_NAME(D));
	if (!d)
		d = pdf_dict_put_dict(ctx, ocp, PDF_NAME(D), 2);
	order = pdf_dict_get(ctx, d, PDF_NAME(Order));
	if (!order)
		order = pdf_dict_put_array(ctx, d, PDF_NAME(Order), 1);
	pdf_array_push(ctx, order, ocg);
	pdf_drop_obj(ctx, ocg);
	return ocg;
}

static pdf_obj *add_stamp_stream(fz_context *ctx, pdf_document *doc, const char *kind, fz_buffer *b)
{
	pdf_obj *d = pdf_new_dict(ctx, doc, 2), *ref = NULL;
	fz_try(ctx)
	{
		pdf_dict_puts_drop(ctx, d, "PaginautStamp", pdf_new_name(ctx, kind));
		ref = pdf_add_stream(ctx, doc, b, d, 0);
	}
	fz_always(ctx)
		pdf_drop_obj(ctx, d);
	fz_catch(ctx)
		fz_rethrow(ctx);
	return ref;
}

/* Puts what was drawn on the page, behind or in front of its content, marked
 * as `kind` ("Header", "Watermark", "Background") so it can be removed again. */
int fo_stamp_end(fo_ctx *c, fo_stamp *st, const char *kind, int behind)
{
	pdf_page *page = st->page;
	pdf_document *doc = page->doc;
	pdf_obj *xobj = NULL, *pre = NULL, *post = NULL;
	fz_buffer *b = NULL;
	fz_var(xobj);
	fz_var(pre);
	fz_var(post);
	fz_var(b);
	fz_try(CTX)
	{
		pdf_obj *pobj = page->obj;
		pdf_obj *contents = pdf_dict_get(CTX, pobj, PDF_NAME(Contents));
		pdf_obj *res, *xobjs, *arr;
		fz_matrix ctm, m;
		char name[64];
		int n = 1;
		fz_close_device(CTX, st->dev);
		xobj = pdf_new_xobject(CTX, doc, fz_make_rect(0, 0, st->w, st->h), fz_identity, st->res, st->contents);
		res = page_own_resources(CTX, doc, pobj);
		xobjs = pdf_dict_get(CTX, res, PDF_NAME(XObject));
		if (!xobjs)
			xobjs = pdf_dict_put_dict(CTX, res, PDF_NAME(XObject), 1);
		do
			fz_snprintf(name, sizeof name, "PgnStamp%s%d", kind, n++);
		while (pdf_dict_gets(CTX, xobjs, name));
		pdf_dict_puts(CTX, xobjs, name, xobj);

		/* The drawing is in page space as shown, y down; back to the PDF's own space. */
		pdf_page_transform(CTX, page, NULL, &ctm);
		m = fz_concat(fz_make_matrix(1, 0, 0, -1, 0, st->h), fz_invert_matrix(ctm));
		b = fz_new_buffer(CTX, 256);
		if (!strcmp(kind, "Watermark"))
		{
			pdf_obj *props = pdf_dict_get(CTX, res, PDF_NAME(Properties));
			if (!props)
				props = pdf_dict_put_dict(CTX, res, PDF_NAME(Properties), 1);
			pdf_dict_puts(CTX, props, "PgnWM", watermark_layer(CTX, doc));
			fz_append_string(CTX, b, "/OC /PgnWM BDC ");
		}
		fz_append_printf(CTX, b, "/Artifact <</Type /Pagination /Subtype /%s>> BDC q %g %g %g %g %g %g cm /%s Do Q EMC",
			stamp_subtype(kind), m.a, m.b, m.c, m.d, m.e, m.f, name);
		if (!strcmp(kind, "Watermark"))
			fz_append_string(CTX, b, " EMC");
		fz_append_byte(CTX, b, '\n');

		if (pdf_is_array(CTX, contents))
			arr = contents;
		else
		{
			arr = pdf_new_array(CTX, doc, 3);
			if (contents)
				pdf_array_push(CTX, arr, contents);
			pdf_dict_put_drop(CTX, pobj, PDF_NAME(Contents), arr);
		}
		if (behind)
		{
			post = add_stamp_stream(CTX, doc, kind, b);
			pdf_array_insert(CTX, arr, post, 0);
		}
		else
		{
			/* The page's own content runs inside q … Q, so the state it leaves can't move the stamp. */
			fz_buffer *q = fz_new_buffer_from_copied_data(CTX, (const unsigned char *)"q\n", 2);
			fz_try(CTX)
				pre = add_stamp_stream(CTX, doc, kind, q);
			fz_always(CTX)
				fz_drop_buffer(CTX, q);
			fz_catch(CTX)
				fz_rethrow(CTX);
			fz_drop_buffer(CTX, b);
			b = NULL;
			b = fz_new_buffer(CTX, 256);
			fz_append_string(CTX, b, "Q\n");
			if (!strcmp(kind, "Watermark"))
				fz_append_string(CTX, b, "/OC /PgnWM BDC ");
			fz_append_printf(CTX, b, "/Artifact <</Type /Pagination /Subtype /%s>> BDC q %g %g %g %g %g %g cm /%s Do Q EMC",
				stamp_subtype(kind), m.a, m.b, m.c, m.d, m.e, m.f, name);
			if (!strcmp(kind, "Watermark"))
				fz_append_string(CTX, b, " EMC");
			fz_append_byte(CTX, b, '\n');
			post = add_stamp_stream(CTX, doc, kind, b);
			pdf_array_insert(CTX, arr, pre, 0);
			pdf_array_push(CTX, arr, post);
		}
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, xobj);
		pdf_drop_obj(CTX, pre);
		pdf_drop_obj(CTX, post);
		fz_drop_buffer(CTX, b);
		fz_drop_device(CTX, st->dev);
		st->dev = NULL;
		pdf_drop_obj(CTX, st->res);
		fz_drop_buffer(CTX, st->contents);
		fz_free(CTX, st);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* Removes the stamps of one kind from a page. Returns how many were there. */
int fo_stamp_remove(fo_ctx *c, fo_page *p, const char *kind)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	int count = 0;
	if (!page)
		return 0;
	fz_try(CTX)
	{
		pdf_obj *contents = pdf_dict_get(CTX, page->obj, PDF_NAME(Contents));
		pdf_obj *xobjs = pdf_dict_getp(CTX, page->obj, "Resources/XObject");
		char prefix[64];
		fz_snprintf(prefix, sizeof prefix, "PgnStamp%s", kind);
		for (int i = pdf_array_len(CTX, contents) - 1; i >= 0; i--)
		{
			pdf_obj *k = pdf_dict_gets(CTX, pdf_array_get(CTX, contents, i), "PaginautStamp");
			if (k && !strcmp(pdf_to_name(CTX, k), kind))
			{
				pdf_array_delete(CTX, contents, i);
				count++;
			}
		}
		for (int i = pdf_dict_len(CTX, xobjs) - 1; count && i >= 0; i--)
			if (!strncmp(pdf_to_name(CTX, pdf_dict_get_key(CTX, xobjs, i)), prefix, strlen(prefix)))
				pdf_dict_del(CTX, xobjs, pdf_dict_get_key(CTX, xobjs, i));
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return count;
}

/* ---------------------------------------------------------------- bookmarks, page labels, properties */

/* Takes the document's bookmarks away (fo_pdf_outline_add then builds new ones). */
int fo_pdf_outline_clear(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_try(CTX)
		pdf_dict_del(CTX, pdf_dict_get(CTX, pdf_trailer(CTX, pdf), PDF_NAME(Root)), PDF_NAME(Outlines));
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* Adds a bookmark as the last child of `parent` (an object number from an
 * earlier call, or 0 for the top level). Returns the new one's object number. */
int fo_pdf_outline_add(fo_ctx *c, fo_doc *d, int parent, const char *title, int page, float y)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *item = NULL, *outlines = NULL;
	int num = -1;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(item);
	fz_var(outlines);
	fz_try(CTX)
	{
		pdf_obj *root = pdf_dict_get(CTX, pdf_trailer(CTX, pdf), PDF_NAME(Root));
		pdf_obj *parent_obj;
		if (parent > 0)
			parent_obj = outlines = pdf_new_indirect(CTX, pdf, parent, 0);
		else
		{
			parent_obj = pdf_dict_get(CTX, root, PDF_NAME(Outlines));
			if (!parent_obj)
			{
				outlines = pdf_add_new_dict(CTX, pdf, 4);
				pdf_dict_put(CTX, outlines, PDF_NAME(Type), PDF_NAME(Outlines));
				pdf_dict_put(CTX, root, PDF_NAME(Outlines), outlines);
				parent_obj = outlines;
			}
		}
		if (page >= pdf_count_pages(CTX, pdf))
			page = -1;
		item = outline_add(CTX, pdf, parent_obj, parent <= 0, title, page, y);
		num = pdf_to_num(CTX, item);
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, item);
		pdf_drop_obj(CTX, outlines);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return num;
}

/* The page label ranges: JSON [{"s":first page index,"st":"D"|"r"|"R"|"a"|"A"|"","p":prefix,"n":first number}...]. */
char *fo_pdf_page_label_ranges_json(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *starts = NULL, *dicts = NULL;
	fz_buffer *b = NULL;
	char *s = NULL;
	fz_var(starts);
	fz_var(dicts);
	fz_var(b);
	fz_try(CTX)
	{
		b = fz_new_buffer(CTX, 128);
		fz_append_byte(CTX, b, '[');
		if (pdf)
		{
			pdf_obj *tree = pdf_dict_getp(CTX, pdf_trailer(CTX, pdf), "Root/PageLabels");
			starts = pdf_new_array(CTX, pdf, 8);
			dicts = pdf_new_array(CTX, pdf, 8);
			label_ranges(CTX, tree, starts, dicts, 0);
			for (int i = 0; i < pdf_array_len(CTX, starts); i++)
			{
				pdf_obj *dd = pdf_array_get(CTX, dicts, i);
				pdf_obj *st = pdf_dict_get(CTX, dd, PDF_NAME(St));
				if (i)
					fz_append_byte(CTX, b, ',');
				fz_append_printf(CTX, b, "{\"s\":%d,\"st\":", pdf_array_get_int(CTX, starts, i));
				json_str(CTX, b, pdf_to_name(CTX, pdf_dict_get(CTX, dd, PDF_NAME(S))));
				fz_append_string(CTX, b, ",\"p\":");
				json_str(CTX, b, pdf_dict_get_text_string(CTX, dd, PDF_NAME(P)));
				fz_append_printf(CTX, b, ",\"n\":%d}", st ? pdf_to_int(CTX, st) : 1);
			}
		}
		fz_append_byte(CTX, b, ']');
		s = buffer_to_string(CTX, b);
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, starts);
		pdf_drop_obj(CTX, dicts);
		fz_drop_buffer(CTX, b);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* Replaces the page labels with ranges: starting at page index starts[i], style
 * styles[i] ('D', 'r', 'R', 'a', 'A' or 0 for none), prefix, first number.
 * Prefixes are separated by '\n'. count 0 removes the labels. */
int fo_pdf_set_page_labels(fo_ctx *c, fo_doc *d, const int *starts, const char *styles, const char *prefixes, const int *firsts, int count)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	char *copy = NULL;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(copy);
	fz_try(CTX)
	{
		char *prefix, *next;
		pdf_dict_del(CTX, pdf_dict_get(CTX, pdf_trailer(CTX, pdf), PDF_NAME(Root)), PDF_NAME(PageLabels));
		copy = fz_strdup(CTX, prefixes ? prefixes : "");
		prefix = copy;
		for (int i = 0; i < count; i++)
		{
			next = prefix ? strchr(prefix, '\n') : NULL;
			if (next)
				*next = 0;
			pdf_set_page_labels(CTX, pdf, starts[i], (pdf_page_label_style)(unsigned char)styles[i], prefix && prefix[0] ? prefix : NULL, firsts[i]);
			prefix = next ? next + 1 : NULL;
		}
	}
	fz_always(CTX)
		fz_free(CTX, copy);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* Sets an entry of the document information ("info:Title", "info:Author"…); empty removes it. */
int fo_set_metadata(fo_ctx *c, fo_doc *d, const char *key, const char *value)
{
	fz_try(CTX)
	{
		pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
		if (pdf && !value[0] && !strncmp(key, "info:", 5))
		{
			pdf_obj *info = pdf_dict_get(CTX, pdf_trailer(CTX, pdf), PDF_NAME(Info));
			if (pdf_is_dict(CTX, info))
				pdf_dict_dels(CTX, info, key + 5);
		}
		else
			fz_set_metadata(CTX, (fz_document *)d, key, value);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- protect, compress, repair */

/* Collects the pages under `node` that can be read, noting what they inherit
 * (resources, media box, crop box, rotation) from the nodes above. Visited objects
 * are marked (and listed in `seen`, to unmark), which stops at cycles; `bad`
 * counts entries that aren't pages. */
static void collect_pages(fz_context *ctx, pdf_document *doc, pdf_obj *node, pdf_obj *inherit, pdf_obj *out, pdf_obj *seen, int depth, int *bad)
{
	static pdf_obj *const keys[4] = { PDF_NAME(Resources), PDF_NAME(MediaBox), PDF_NAME(CropBox), PDF_NAME(Rotate) };
	pdf_obj *kids = pdf_dict_get(ctx, node, PDF_NAME(Kids));
	pdf_obj *mine = NULL;
	int i, n;
	if (depth > 64 || pdf_mark_obj(ctx, node))
	{
		(*bad)++;
		return;
	}
	pdf_array_push(ctx, seen, node);
	fz_var(mine);
	fz_try(ctx)
	{
		mine = pdf_copy_dict(ctx, inherit);
		for (i = 0; i < 4; i++)
		{
			pdf_obj *v = pdf_dict_get(ctx, node, keys[i]);
			if (v)
				pdf_dict_put(ctx, mine, keys[i], v);
		}
		n = pdf_array_len(ctx, kids);
		for (i = 0; i < n; i++)
		{
			pdf_obj *kid = pdf_array_get(ctx, kids, i);
			if (!pdf_is_dict(ctx, kid))
				(*bad)++;
			else if (pdf_is_array(ctx, pdf_dict_get(ctx, kid, PDF_NAME(Kids))))
				collect_pages(ctx, doc, kid, mine, out, seen, depth + 1, bad);
			else if (pdf_name_eq(ctx, pdf_dict_get(ctx, kid, PDF_NAME(Type)), PDF_NAME(Page)) || pdf_dict_get(ctx, kid, PDF_NAME(Contents)))
			{
				if (pdf_mark_obj(ctx, kid))
				{
					(*bad)++;
					continue;
				}
				pdf_array_push(ctx, seen, kid);
				for (int k = 0; k < 4; k++)
					if (!pdf_dict_get(ctx, kid, keys[k]) && pdf_dict_get(ctx, mine, keys[k]))
						pdf_dict_put(ctx, kid, keys[k], pdf_dict_get(ctx, mine, keys[k]));
				pdf_array_push(ctx, out, kid);
			}
			else
				(*bad)++;
		}
	}
	fz_always(ctx)
		pdf_drop_obj(ctx, mine);
	fz_catch(ctx)
		fz_rethrow(ctx);
}

int fo_pdf_fix_page_tree(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *pages = NULL, *seen = NULL, *inherit = NULL;
	int bad = 0, dropped = 0;
	if (!pdf || !pdf_was_repaired(CTX, pdf))
		return 0;
	fz_var(pages);
	fz_var(seen);
	fz_var(inherit);
	fz_try(CTX)
	{
		pdf_obj *root = pdf_dict_getp(CTX, pdf_trailer(CTX, pdf), "Root/Pages");
		int count = pdf_dict_get_int(CTX, root, PDF_NAME(Count));
		pages = pdf_new_array(CTX, pdf, 64);
		seen = pdf_new_array(CTX, pdf, 64);
		inherit = pdf_new_dict(CTX, pdf, 4);
		collect_pages(CTX, pdf, root, inherit, pages, seen, 0, &bad);
		if ((bad || pdf_array_len(CTX, pages) != count) && pdf_array_len(CTX, pages) > 0)
		{
			/* One flat list of the pages that can be read, each with what it inherited. */
			int n = pdf_array_len(CTX, pages);
			pdf_begin_operation(CTX, pdf, "Repair page tree");
			fz_try(CTX)
			{
				for (int i = 0; i < n; i++)
					pdf_dict_put(CTX, pdf_array_get(CTX, pages, i), PDF_NAME(Parent), root);
				pdf_dict_put(CTX, root, PDF_NAME(Kids), pages);
				pdf_dict_put_int(CTX, root, PDF_NAME(Count), n);
				pdf_dict_del(CTX, root, PDF_NAME(Parent));
			}
			fz_always(CTX)
				pdf_end_operation(CTX, pdf);
			fz_catch(CTX)
				fz_rethrow(CTX);
			dropped = count > n ? count - n : 0;
		}
	}
	fz_always(CTX)
	{
		for (int i = 0; i < pdf_array_len(CTX, seen); i++)
			pdf_unmark_obj(CTX, pdf_array_get(CTX, seen, i));
		pdf_drop_obj(CTX, pages);
		pdf_drop_obj(CTX, seen);
		pdf_drop_obj(CTX, inherit);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return dropped;
}

char *fo_pdf_security_json(fo_ctx *c, fo_doc *d, const char *password)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	fz_buffer *b = NULL;
	char *s = NULL;
	fz_var(b);
	fz_try(CTX)
	{
		int owner = 1, opens = 0, perms = -1, bits = 0;
		const char *method = "";
		if (pdf && pdf->crypt)
		{
			/* pdf_needs_password tries the empty password, which drops the keys when
			 * that's wrong; authenticating again with the one it was opened with (or
			 * none) puts them back, and says whether that one is the owner's. */
			opens = pdf_needs_password(CTX, pdf);
			owner = (pdf_authenticate_password(CTX, pdf, password ? password : "") & 4) != 0;
			perms = pdf_crypt_permissions(CTX, pdf->crypt);
			method = pdf_crypt_method(CTX, pdf->crypt);
			bits = pdf_crypt_length(CTX, pdf->crypt);
		}
		b = fz_new_buffer(CTX, 128);
		fz_append_string(CTX, b, "{\"method\":");
		json_str(CTX, b, method);
		fz_append_printf(CTX, b, ",\"bits\":%d,\"perms\":%d,\"owner\":%s,\"opens\":%s,\"repaired\":%s}", bits, perms,
			owner ? "true" : "false", opens ? "true" : "false", pdf && pdf_was_repaired(CTX, pdf) ? "true" : "false");
		s = buffer_to_string(CTX, b);
	}
	fz_always(CTX)
		fz_drop_buffer(CTX, b);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

int fo_pdf_check_owner(fo_ctx *c, fo_doc *d, const char *candidate, const char *current)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int ok = 0;
	if (!pdf || !pdf->crypt)
		return 1;
	fz_try(CTX)
	{
		ok = (pdf_authenticate_password(CTX, pdf, candidate) & 4) != 0;
		/* A wrong one leaves the document without its keys: authenticate it again. */
		if (!ok)
			pdf_authenticate_password(CTX, pdf, current ? current : "");
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return ok;
}

int fo_pdf_rewrite_images(fo_ctx *c, fo_doc *d, int dpi, int quality, int lossless_too)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_image_rewriter_options o;
	char q[8];
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	memset(&o, 0, sizeof o);
	snprintf(q, sizeof q, "%d", quality);
	/* Pictures shown finer than 1.5x the target are brought down to it; photos
	 * (and, for the smallest size, lossless pictures too) are saved as JPEG. */
	o.color_lossy_image_subsample_method = o.gray_lossy_image_subsample_method = FZ_SUBSAMPLE_BICUBIC;
	o.color_lossless_image_subsample_method = o.gray_lossless_image_subsample_method = FZ_SUBSAMPLE_BICUBIC;
	o.color_lossy_image_subsample_threshold = o.gray_lossy_image_subsample_threshold = dpi * 3 / 2;
	o.color_lossy_image_subsample_to = o.gray_lossy_image_subsample_to = dpi;
	o.color_lossless_image_subsample_threshold = o.gray_lossless_image_subsample_threshold = dpi * 3 / 2;
	o.color_lossless_image_subsample_to = o.gray_lossless_image_subsample_to = dpi;
	o.color_lossy_image_recompress_method = o.gray_lossy_image_recompress_method = FZ_RECOMPRESS_JPEG;
	o.color_lossy_image_recompress_quality = o.gray_lossy_image_recompress_quality = q;
	o.color_lossless_image_recompress_method = o.gray_lossless_image_recompress_method = lossless_too ? FZ_RECOMPRESS_JPEG : FZ_RECOMPRESS_SAME;
	o.color_lossless_image_recompress_quality = o.gray_lossless_image_recompress_quality = lossless_too ? q : NULL;
	/* Black-and-white scans stay sharp: only very fine ones are brought down, to twice the target. */
	o.bitonal_image_subsample_method = FZ_SUBSAMPLE_AVERAGE;
	o.bitonal_image_subsample_threshold = dpi * 3;
	o.bitonal_image_subsample_to = dpi * 2;
	o.bitonal_image_recompress_method = FZ_RECOMPRESS_SAME;
	o.recompress_when = FZ_RECOMPRESS_WHEN_SMALLER;
	pdf_begin_operation(CTX, pdf, "Compress pictures");
	fz_try(CTX)
		pdf_rewrite_images(CTX, pdf, &o);
	fz_always(CTX)
		pdf_end_operation(CTX, pdf);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

int fo_pdf_save_with(fo_ctx *c, fo_doc *d, const char *path, int encrypt, int permissions, const char *owner, const char *user, int compact)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_write_options opts = pdf_default_write_options;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	opts.do_compress = 1;
	opts.do_compress_images = 1;
	opts.do_compress_fonts = 1;
	opts.do_garbage = compact ? 3 : 1;
	if (compact)
	{
		opts.do_use_objstms = 1;
		opts.compression_effort = 100;
	}
	if (encrypt == 0)
		opts.do_encrypt = PDF_ENCRYPT_NONE;
	else if (encrypt == 1)
	{
		opts.do_encrypt = PDF_ENCRYPT_AES_256;
		opts.permissions = permissions;
		fz_strlcpy(opts.opwd_utf8, owner ? owner : "", sizeof opts.opwd_utf8);
		fz_strlcpy(opts.upwd_utf8, user ? user : "", sizeof opts.upwd_utf8);
	}
	fz_try(CTX)
	{
		char *p = os_path(CTX, path);
		fz_try(CTX)
			pdf_save_document(CTX, pdf, p, &opts);
		fz_always(CTX)
			fz_free(CTX, p);
		fz_catch(CTX)
			fz_rethrow(CTX);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- pictures */

static fz_stext_page *pictures_of(fz_context *ctx, fz_display_list *l)
{
	fz_stext_options opts;
	fz_init_stext_options(ctx, &opts);
	opts.flags |= FZ_STEXT_PRESERVE_IMAGES;
	return fz_new_stext_page_from_display_list(ctx, l, &opts);
}

static fz_stext_block *nth_picture(fz_context *ctx, fz_stext_page *t, int idx)
{
	for (fz_stext_block *b = t->first_block; b; b = b->next)
		if (b->type == FZ_STEXT_BLOCK_IMAGE && b->u.i.image && idx-- == 0)
			return b;
	fz_throw(ctx, FZ_ERROR_ARGUMENT, "no such picture");
}

/* The picture's own bytes, when they can be saved as they are (a JPEG or JPEG 2000 file). */
static fz_buffer *original_bytes(fz_context *ctx, fz_image *img, const char **ext)
{
	fz_compressed_buffer *cb = fz_compressed_image_buffer(ctx, img);
	if (!cb || !cb->buffer)
		return NULL;
	if (cb->params.type == FZ_IMAGE_JPEG)
		*ext = "jpg";
	else if (cb->params.type == FZ_IMAGE_JPX)
		*ext = "jp2";
	else
		return NULL;
	return fz_keep_buffer(ctx, cb->buffer);
}

/* The picture as PNG: full resolution, RGB or gray, with its transparency. */
static fz_buffer *picture_png(fz_context *ctx, fz_image *img)
{
	fz_pixmap *pix = NULL, *rgb = NULL, *mask = NULL, *scaled = NULL, *both = NULL;
	fz_buffer *out = NULL;
	fz_var(pix);
	fz_var(rgb);
	fz_var(mask);
	fz_var(scaled);
	fz_var(both);
	fz_try(ctx)
	{
		fz_pixmap *use;
		pix = fz_get_pixmap_from_image(ctx, img, NULL, NULL, NULL, NULL);
		use = pix;
		if (pix->colorspace && !fz_colorspace_is_gray(ctx, pix->colorspace) && !fz_colorspace_is_rgb(ctx, pix->colorspace))
			use = rgb = fz_convert_pixmap(ctx, pix, fz_device_rgb(ctx), NULL, NULL, fz_default_color_params, 1);
		if (img->mask && !use->alpha)
		{
			mask = fz_get_pixmap_from_image(ctx, img->mask, NULL, NULL, NULL, NULL);
			if (mask->n == 1 && !mask->alpha)
			{
				if (mask->w != use->w || mask->h != use->h)
				{
					fz_irect clip = { 0, 0, use->w, use->h };
					scaled = fz_scale_pixmap(ctx, mask, 0, 0, use->w, use->h, &clip);
				}
				both = fz_new_pixmap_from_color_and_mask(ctx, use, scaled ? scaled : mask);
				use = both;
			}
		}
		out = fz_new_buffer_from_pixmap_as_png(ctx, use, fz_default_color_params);
	}
	fz_always(ctx)
	{
		fz_drop_pixmap(ctx, pix);
		fz_drop_pixmap(ctx, rgb);
		fz_drop_pixmap(ctx, mask);
		fz_drop_pixmap(ctx, scaled);
		fz_drop_pixmap(ctx, both);
	}
	fz_catch(ctx)
		fz_rethrow(ctx);
	return out;
}

/* A one-page PDF with just the picture, stored as it is, at the size it has on the page. */
static fz_buffer *picture_pdf(fz_context *ctx, fz_image *img, fz_rect r)
{
	pdf_document *doc = NULL;
	pdf_obj *res = NULL, *page = NULL;
	fz_buffer *contents = NULL, *out = NULL;
	fz_output *o = NULL;
	fz_var(doc);
	fz_var(res);
	fz_var(page);
	fz_var(contents);
	fz_var(out);
	fz_var(o);
	fz_try(ctx)
	{
		float w = fz_max(1, r.x1 - r.x0), h = fz_max(1, r.y1 - r.y0);
		pdf_write_options opts = pdf_default_write_options;
		doc = pdf_create_document(ctx);
		res = pdf_new_dict(ctx, doc, 1);
		pdf_dict_putp_drop(ctx, res, "XObject/Im0", pdf_add_image(ctx, doc, img));
		contents = fz_new_buffer(ctx, 64);
		fz_append_printf(ctx, contents, "q %g 0 0 %g 0 0 cm /Im0 Do Q", w, h);
		page = pdf_add_page(ctx, doc, fz_make_rect(0, 0, w, h), 0, res, contents);
		pdf_insert_page(ctx, doc, -1, page);
		out = fz_new_buffer(ctx, 1024);
		o = fz_new_output_with_buffer(ctx, out);
		opts.do_compress = 1;
		opts.do_garbage = 1;
		pdf_write_document(ctx, doc, o, &opts);
		fz_close_output(ctx, o);
	}
	fz_always(ctx)
	{
		fz_drop_output(ctx, o);
		fz_drop_buffer(ctx, contents);
		pdf_drop_obj(ctx, page);
		pdf_drop_obj(ctx, res);
		pdf_drop_document(ctx, doc);
	}
	fz_catch(ctx)
	{
		fz_drop_buffer(ctx, out);
		fz_rethrow(ctx);
	}
	return out;
}

char *fo_pictures_json(fo_ctx *c, fo_list *l, const float b[4])
{
	fz_stext_page *t = NULL;
	fz_buffer *buf = NULL;
	char *s = NULL;
	fz_var(t);
	fz_var(buf);
	fz_try(CTX)
	{
		int first = 1;
		t = pictures_of(CTX, (fz_display_list *)l);
		buf = fz_new_buffer(CTX, 256);
		fz_append_byte(CTX, buf, '[');
		for (fz_stext_block *block = t->first_block; block; block = block->next)
		{
			fz_image *img;
			fz_compressed_buffer *cb;
			const char *ext = "png";
			unsigned char md5[16];
			if (block->type != FZ_STEXT_BLOCK_IMAGE || !block->u.i.image)
				continue;
			img = block->u.i.image;
			cb = fz_compressed_image_buffer(CTX, img);
			if (cb && cb->params.type == FZ_IMAGE_JPEG)
				ext = "jpg";
			else if (cb && cb->params.type == FZ_IMAGE_JPX)
				ext = "jp2";
			if (!first)
				fz_append_byte(CTX, buf, ',');
			first = 0;
			fz_append_printf(CTX, buf, "{\"r\":[%g,%g,%g,%g],\"w\":%d,\"h\":%d,\"k\":\"%s\",\"m\":%d,\"n\":%zu,\"id\":\"",
				block->bbox.x0 - b[0], block->bbox.y0 - b[1], block->bbox.x1 - b[0], block->bbox.y1 - b[1],
				img->w, img->h, ext, img->mask || img->imagemask ? 1 : 0, cb && cb->buffer ? cb->buffer->len : 0);
			/* The same picture used again (on other pages, too) has the same id. */
			if (cb && cb->buffer)
			{
				fz_md5_buffer(CTX, cb->buffer, md5);
				for (int i = 0; i < 16; i++)
					fz_append_printf(CTX, buf, "%02x", md5[i]);
			}
			fz_append_string(CTX, buf, "\"}");
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_drop_stext_page(CTX, t);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

int fo_picture_data(fo_ctx *c, fo_list *l, int idx, int fmt, unsigned char **data, size_t *len, char ext[8])
{
	fz_stext_page *t = NULL;
	fz_buffer *out = NULL;
	*data = NULL;
	*len = 0;
	fz_var(t);
	fz_var(out);
	fz_try(CTX)
	{
		fz_stext_block *block;
		const char *e = "png";
		t = pictures_of(CTX, (fz_display_list *)l);
		block = nth_picture(CTX, t, idx);
		if (fmt == 0)
			out = original_bytes(CTX, block->u.i.image, &e);
		if (fmt == 2)
		{
			out = picture_pdf(CTX, block->u.i.image, block->bbox);
			e = "pdf";
		}
		if (!out)
		{
			out = picture_png(CTX, block->u.i.image);
			e = "png";
		}
		fz_strlcpy(ext, e, 8);
		*len = out->len;
		*data = fz_malloc(CTX, out->len ? out->len : 1);
		memcpy(*data, out->data, out->len);
	}
	fz_always(CTX)
	{
		fz_drop_stext_page(CTX, t);
		fz_drop_buffer(CTX, out);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- attachments */

/* Every embedded file: the document's (EmbeddedFiles), then those pinned to pages
 * (FileAttachment annotations), each once. `pages` gets the page index, or -1. */
static void collect_attachments(fz_context *ctx, pdf_document *doc, pdf_obj *files, pdf_obj *pages)
{
	pdf_obj *tree = pdf_load_name_tree(ctx, doc, PDF_NAME(EmbeddedFiles));
	fz_try(ctx)
	{
		int n = pdf_dict_len(ctx, tree);
		for (int i = 0; i < n; i++)
		{
			pdf_obj *fs = pdf_dict_get_val(ctx, tree, i);
			if (pdf_is_embedded_file(ctx, fs))
			{
				pdf_array_push(ctx, files, fs);
				pdf_array_push_int(ctx, pages, -1);
			}
		}
		n = pdf_count_pages(ctx, doc);
		for (int p = 0; p < n; p++)
		{
			pdf_obj *annots = pdf_dict_get(ctx, pdf_lookup_page_obj(ctx, doc, p), PDF_NAME(Annots));
			for (int a = 0; a < pdf_array_len(ctx, annots); a++)
			{
				pdf_obj *annot = pdf_array_get(ctx, annots, a);
				pdf_obj *fs = pdf_dict_get(ctx, annot, PDF_NAME(FS));
				int dup = 0;
				if (!pdf_name_eq(ctx, pdf_dict_get(ctx, annot, PDF_NAME(Subtype)), PDF_NAME(FileAttachment)) || !pdf_is_embedded_file(ctx, fs))
					continue;
				for (int k = 0; k < pdf_array_len(ctx, files) && !dup; k++)
					dup = pdf_is_indirect(ctx, fs) && pdf_to_num(ctx, fs) == pdf_to_num(ctx, pdf_array_get(ctx, files, k));
				if (dup)
					continue;
				pdf_array_push(ctx, files, fs);
				pdf_array_push_int(ctx, pages, p);
			}
		}
	}
	fz_always(ctx)
		pdf_drop_obj(ctx, tree);
	fz_catch(ctx)
		fz_rethrow(ctx);
}

char *fo_attachments_json(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *files = NULL, *pages = NULL;
	fz_buffer *b = NULL;
	char *s = NULL;
	fz_var(files);
	fz_var(pages);
	fz_var(b);
	fz_try(CTX)
	{
		b = fz_new_buffer(CTX, 256);
		fz_append_byte(CTX, b, '[');
		if (pdf)
		{
			files = pdf_new_array(CTX, pdf, 8);
			pages = pdf_new_array(CTX, pdf, 8);
			collect_attachments(CTX, pdf, files, pages);
			for (int i = 0; i < pdf_array_len(CTX, files); i++)
			{
				pdf_obj *fs = pdf_array_get(CTX, files, i);
				pdf_filespec_params prm;
				pdf_get_filespec_params(CTX, fs, &prm);
				if (i)
					fz_append_byte(CTX, b, ',');
				char when[32];
				/* fz_append_printf has no %lld. */
				snprintf(when, sizeof when, "%lld", (long long)prm.modified);
				fz_append_printf(CTX, b, "{\"i\":%d,\"page\":%d,\"size\":%d,\"modified\":%s,\"name\":", i, pdf_array_get_int(CTX, pages, i), prm.size, when);
				json_str(CTX, b, prm.filename ? prm.filename : "");
				fz_append_string(CTX, b, ",\"mime\":");
				json_str(CTX, b, prm.mimetype ? prm.mimetype : "");
				fz_append_string(CTX, b, ",\"desc\":");
				json_str(CTX, b, pdf_dict_get_text_string(CTX, fs, PDF_NAME(Desc)));
				fz_append_byte(CTX, b, '}');
			}
		}
		fz_append_byte(CTX, b, ']');
		s = buffer_to_string(CTX, b);
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, files);
		pdf_drop_obj(CTX, pages);
		fz_drop_buffer(CTX, b);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

int fo_attachment_data(fo_ctx *c, fo_doc *d, int idx, unsigned char **data, size_t *len)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *files = NULL, *pages = NULL;
	fz_buffer *buf = NULL;
	*data = NULL;
	*len = 0;
	fz_var(files);
	fz_var(pages);
	fz_var(buf);
	fz_try(CTX)
	{
		if (!pdf)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "not a PDF");
		files = pdf_new_array(CTX, pdf, 8);
		pages = pdf_new_array(CTX, pdf, 8);
		collect_attachments(CTX, pdf, files, pages);
		if (idx < 0 || idx >= pdf_array_len(CTX, files))
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "no such attachment");
		buf = pdf_load_embedded_file_contents(CTX, pdf_array_get(CTX, files, idx));
		if (!buf)
			fz_throw(CTX, FZ_ERROR_FORMAT, "the attachment is empty or can't be read");
		*len = buf->len;
		*data = fz_malloc(CTX, buf->len ? buf->len : 1);
		memcpy(*data, buf->data, buf->len);
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, files);
		pdf_drop_obj(CTX, pages);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- areas */

static int take_buffer(fz_context *ctx, fz_buffer *buf, unsigned char **data, size_t *len)
{
	*len = buf->len;
	*data = fz_malloc(ctx, buf->len ? buf->len : 1);
	memcpy(*data, buf->data, buf->len);
	return 0;
}

int fo_area_image(fo_ctx *c, fo_list *l, const float b[4], const float area[4], int rot, float dpi, int svg, unsigned char **data, size_t *len)
{
	fz_pixmap *pix = NULL;
	fz_device *dev = NULL;
	fz_buffer *buf = NULL;
	fz_output *out = NULL;
	*data = NULL;
	*len = 0;
	fz_var(pix);
	fz_var(dev);
	fz_var(buf);
	fz_var(out);
	fz_try(CTX)
	{
		fz_rect r = fz_make_rect(area[0] + b[0], area[1] + b[1], area[2] + b[0], area[3] + b[1]);
		/* Turned as the page is shown, with the area's corner at 0,0. */
		fz_matrix turn = fz_rotate((float)rot);
		fz_rect t = fz_transform_rect(r, turn);
		turn = fz_concat(turn, fz_translate(-t.x0, -t.y0));
		if (svg)
		{
			/* Vector: text as outlines, so it looks the same everywhere. */
			buf = fz_new_buffer(CTX, 4096);
			out = fz_new_output_with_buffer(CTX, buf);
			dev = fz_new_svg_device(CTX, out, t.x1 - t.x0, t.y1 - t.y0, FZ_SVG_TEXT_AS_PATH, 1);
			fz_run_display_list(CTX, (fz_display_list *)l, dev, turn, fz_make_rect(0, 0, t.x1 - t.x0, t.y1 - t.y0), NULL);
			fz_close_device(CTX, dev);
			fz_close_output(CTX, out);
		}
		else
		{
			float z = dpi / 72;
			fz_matrix ctm = fz_concat(turn, fz_scale(z, z));
			fz_irect box = fz_round_rect(fz_transform_rect(r, ctm));
			if ((int64_t)(box.x1 - box.x0) * (box.y1 - box.y0) > 200000000)
				fz_throw(CTX, FZ_ERROR_LIMIT, "that's too large at this resolution; choose a lower one");
			pix = fz_new_pixmap_with_bbox(CTX, fz_device_rgb(CTX), box, NULL, 0);
			fz_clear_pixmap_with_value(CTX, pix, 255);
			dev = fz_new_draw_device(CTX, fz_identity, pix);
			fz_run_display_list(CTX, (fz_display_list *)l, dev, ctm, fz_rect_from_irect(box), NULL);
			fz_close_device(CTX, dev);
			pix->xres = pix->yres = (int)dpi;
			buf = fz_new_buffer_from_pixmap_as_png(CTX, pix, fz_default_color_params);
		}
		take_buffer(CTX, buf, data, len);
	}
	fz_always(CTX)
	{
		fz_drop_device(CTX, dev);
		fz_drop_output(CTX, out);
		fz_drop_pixmap(CTX, pix);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

char *fo_area_text(fo_ctx *c, fo_list *l, const float b[4], const float area[4])
{
	fz_stext_page *t = NULL;
	char *s = NULL, *copy = NULL;
	fz_var(t);
	fz_var(copy);
	fz_try(CTX)
	{
		fz_stext_options opts;
		fz_init_stext_options(CTX, &opts);
		opts.flags |= FZ_STEXT_DEHYPHENATE;
		t = fz_new_stext_page_from_display_list(CTX, (fz_display_list *)l, &opts);
		copy = fz_copy_rectangle(CTX, t, fz_make_rect(area[0] + b[0], area[1] + b[1], area[2] + b[0], area[3] + b[1]), 1);
		s = fz_strdup(CTX, copy ? copy : "");
	}
	fz_always(CTX)
	{
		fz_free(CTX, copy);
		fz_drop_stext_page(CTX, t);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

int fo_pdf_area_pdf(fo_ctx *c, fo_page *p, const float area[4], int rot, unsigned char **data, size_t *len)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	pdf_document *dst = NULL;
	pdf_graft_map *map = NULL;
	fz_buffer *buf = NULL;
	fz_output *out = NULL;
	*data = NULL;
	*len = 0;
	if (!page)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(dst);
	fz_var(map);
	fz_var(buf);
	fz_var(out);
	fz_try(CTX)
	{
		pdf_write_options opts = pdf_default_write_options;
		fz_matrix ctm;
		fz_rect r;
		pdf_obj *np;
		int idx = pdf_lookup_page_number(CTX, page->doc, page->obj);
		dst = pdf_create_document(CTX);
		map = pdf_new_graft_map(CTX, dst);
		pdf_graft_mapped_page(CTX, map, -1, page->doc, idx);
		/* The page itself, vector and all, shown only within the area. */
		pdf_page_transform(CTX, page, NULL, &ctm);
		r = fz_transform_rect(fz_make_rect(area[0], area[1], area[2], area[3]), fz_invert_matrix(ctm));
		np = pdf_lookup_page_obj(CTX, dst, 0);
		pdf_dict_put_rect(CTX, np, PDF_NAME(MediaBox), r);
		pdf_dict_put_rect(CTX, np, PDF_NAME(CropBox), r);
		pdf_dict_del(CTX, np, PDF_NAME(TrimBox));
		pdf_dict_del(CTX, np, PDF_NAME(BleedBox));
		pdf_dict_del(CTX, np, PDF_NAME(ArtBox));
		if (rot)
			pdf_dict_put_int(CTX, np, PDF_NAME(Rotate), ((pdf_dict_get_inheritable_int(CTX, page->obj, PDF_NAME(Rotate)) + rot) % 360 + 360) % 360);
		buf = fz_new_buffer(CTX, 4096);
		out = fz_new_output_with_buffer(CTX, buf);
		opts.do_compress = 1;
		opts.do_compress_images = 1;
		opts.do_compress_fonts = 1;
		opts.do_garbage = 1;
		pdf_write_document(CTX, dst, out, &opts);
		fz_close_output(CTX, out);
		take_buffer(CTX, buf, data, len);
	}
	fz_always(CTX)
	{
		fz_drop_output(CTX, out);
		fz_drop_buffer(CTX, buf);
		pdf_drop_graft_map(CTX, map);
		pdf_drop_document(CTX, dst);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- text */

fo_text *fo_text_new(fo_ctx *c, fo_list *l)
{
	fz_stext_page *t = NULL;
	fz_stext_options opts;
	fz_try(CTX)
	{
		fz_init_stext_options(CTX, &opts);
		opts.flags |= FZ_STEXT_DEHYPHENATE;
		t = fz_new_stext_page_from_display_list(CTX, (fz_display_list *)l, &opts);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return (fo_text *)t;
}

void fo_text_drop(fo_ctx *c, fo_text *t) { fz_drop_stext_page(CTX, (fz_stext_page *)t); }

static void put_quad(float *out, fz_quad q, const float b[4])
{
	out[0] = q.ul.x - b[0]; out[1] = q.ul.y - b[1];
	out[2] = q.ur.x - b[0]; out[3] = q.ur.y - b[1];
	out[4] = q.ll.x - b[0]; out[5] = q.ll.y - b[1];
	out[6] = q.lr.x - b[0]; out[7] = q.lr.y - b[1];
}

int fo_text_highlight(fo_ctx *c, fo_text *t, const float b[4], float ax, float ay, float bx, float by, float *out, int max)
{
	fz_quad *quads = NULL;
	int n = 0;
	fz_var(quads);
	fz_try(CTX)
	{
		quads = fz_malloc_array(CTX, max, fz_quad);
		n = fz_highlight_selection(CTX, (fz_stext_page *)t, fz_make_point(ax + b[0], ay + b[1]), fz_make_point(bx + b[0], by + b[1]), quads, max);
		for (int i = 0; i < n; i++)
			put_quad(out + i * 8, quads[i], b);
	}
	fz_always(CTX)
		fz_free(CTX, quads);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return n;
}

int fo_text_snap(fo_ctx *c, fo_text *t, const float b[4], float ax, float ay, float bx, float by, int mode, float *out)
{
	fz_try(CTX)
	{
		fz_point a = fz_make_point(ax + b[0], ay + b[1]);
		fz_point e = fz_make_point(bx + b[0], by + b[1]);
		fz_quad q = fz_snap_selection(CTX, (fz_stext_page *)t, &a, &e, mode);
		put_quad(out, q, b);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 1;
}

char *fo_text_copy(fo_ctx *c, fo_text *t, const float b[4], float ax, float ay, float bx, float by)
{
	char *s = NULL;
	fz_try(CTX)
		s = fz_copy_selection(CTX, (fz_stext_page *)t, fz_make_point(ax + b[0], ay + b[1]), fz_make_point(bx + b[0], by + b[1]), 1);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

char *fo_text_all(fo_ctx *c, fo_text *t)
{
	fz_buffer *b = NULL;
	char *s = NULL;
	fz_var(b);
	fz_try(CTX)
	{
		b = fz_new_buffer_from_stext_page(CTX, (fz_stext_page *)t);
		s = buffer_to_string(CTX, b);
	}
	fz_always(CTX)
		fz_drop_buffer(CTX, b);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

int fo_text_char_count(fo_ctx *c, fo_text *t)
{
	int n = 0;
	fz_stext_block *block;
	fz_stext_line *line;
	fz_stext_char *ch;
	(void)c;
	for (block = ((fz_stext_page *)t)->first_block; block; block = block->next)
	{
		if (block->type != FZ_STEXT_BLOCK_TEXT)
			continue;
		for (line = block->u.t.first_line; line; line = line->next)
			for (ch = line->first_char; ch; ch = ch->next)
				if (ch->c > 32)
					n++;
	}
	return n;
}

char *fo_text_search_json(fo_ctx *c, fo_text *t, const float b[4], const char *needle)
{
	enum { MAX_HITS = 1000 };
	fz_quad *quads = NULL;
	int *marks = NULL;
	fz_buffer *buf = NULL;
	char *s = NULL;
	fz_var(quads);
	fz_var(marks);
	fz_var(buf);
	fz_try(CTX)
	{
		int n;
		quads = fz_malloc_array(CTX, MAX_HITS, fz_quad);
		marks = fz_malloc_array(CTX, MAX_HITS, int);
		n = fz_search_stext_page(CTX, (fz_stext_page *)t, needle, marks, quads, MAX_HITS);
		buf = fz_new_buffer(CTX, 256);
		fz_append_byte(CTX, buf, '[');
		for (int i = 0; i < n; i++)
		{
			float q[8];
			if (i == 0)
				fz_append_byte(CTX, buf, '[');
			else if (marks[i])
				fz_append_string(CTX, buf, "],[");
			else
				fz_append_byte(CTX, buf, ',');
			put_quad(q, quads[i], b);
			fz_append_printf(CTX, buf, "%g,%g,%g,%g,%g,%g,%g,%g", q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]);
		}
		if (n > 0)
			fz_append_byte(CTX, buf, ']');
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_free(CTX, quads);
		fz_free(CTX, marks);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* ---------------------------------------------------------------- digital signatures */

/* A signer whose signature (CMS) is made by the app: MuPDF hands us the bytes to
 * sign while it saves, and the callback signs them with the certificate from the
 * Windows store (the token asks for its PIN itself). */
typedef struct
{
	pdf_pkcs7_signer base;
	int refs;
	fo_sign_cb cb;
	void *user;
	size_t max;
	pdf_pkcs7_distinguished_name dn;
} fo_signer;

static pdf_pkcs7_signer *signer_keep(fz_context *ctx, pdf_pkcs7_signer *s)
{
	return fz_keep_imp(ctx, s, &((fo_signer *)s)->refs);
}

static void signer_drop(fz_context *ctx, pdf_pkcs7_signer *s_)
{
	fo_signer *s = (fo_signer *)s_;
	if (fz_drop_imp(ctx, s, &s->refs))
	{
		fz_free(ctx, s->dn.cn);
		fz_free(ctx, s->dn.o);
		fz_free(ctx, s->dn.ou);
		fz_free(ctx, s->dn.email);
		fz_free(ctx, s->dn.c);
		fz_free(ctx, s);
	}
}

static char *dup_or_null(fz_context *ctx, const char *s)
{
	return s && *s ? fz_strdup(ctx, s) : NULL;
}

static pdf_pkcs7_distinguished_name *signer_name(fz_context *ctx, pdf_pkcs7_signer *s_)
{
	fo_signer *s = (fo_signer *)s_;
	pdf_pkcs7_distinguished_name *dn = fz_malloc_struct(ctx, pdf_pkcs7_distinguished_name);
	fz_try(ctx)
	{
		dn->cn = dup_or_null(ctx, s->dn.cn);
		dn->o = dup_or_null(ctx, s->dn.o);
		dn->ou = dup_or_null(ctx, s->dn.ou);
		dn->email = dup_or_null(ctx, s->dn.email);
		dn->c = dup_or_null(ctx, s->dn.c);
	}
	fz_catch(ctx)
	{
		pdf_signature_drop_distinguished_name(ctx, dn);
		fz_rethrow(ctx);
	}
	return dn;
}

static size_t signer_max(fz_context *ctx, pdf_pkcs7_signer *s)
{
	(void)ctx;
	return ((fo_signer *)s)->max;
}

static int signer_digest(fz_context *ctx, pdf_pkcs7_signer *s_, fz_stream *in, unsigned char *digest, size_t len)
{
	fo_signer *s = (fo_signer *)s_;
	fz_buffer *buf = NULL;
	ptrdiff_t n = 0;
	if (!in)
		return 0;
	fz_var(buf);
	fz_try(ctx)
	{
		unsigned char *data;
		size_t dlen;
		buf = fz_read_all(ctx, in, 1 << 20);
		dlen = fz_buffer_storage(ctx, buf, &data);
		n = s->cb(s->user, data, dlen, digest, len);
	}
	fz_always(ctx)
		fz_drop_buffer(ctx, buf);
	fz_catch(ctx)
		fz_rethrow(ctx);
	if (n <= 0)
		fz_throw(ctx, FZ_ERROR_GENERIC, "the document couldn't be signed");
	return (int)n;
}

int fo_sign_page(fo_ctx *c, fo_page *p, int num, const float r[4], fo_sign_cb cb, void *user, size_t max,
	const char *const dn[5], const char *reason, const char *location,
	const unsigned char *img, size_t img_len, int flags, int lock)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	fo_signer *s = NULL;
	fz_image *image = NULL;
	fz_buffer *ib = NULL;
	pdf_annot *w = NULL;
	if (!page)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_var(s);
	fz_var(image);
	fz_var(ib);
	fz_var(w);
	/* One operation: with the undo journal on, every change must be inside one. */
	pdf_begin_operation(CTX, page->doc, "Sign");
	fz_try(CTX)
	{
		char name[64];
		fz_rect rect = fz_make_rect(r[0], r[1], r[2], r[3]);
		s = fz_malloc_struct(CTX, fo_signer);
		s->refs = 1;
		s->base.keep = signer_keep;
		s->base.drop = signer_drop;
		s->base.get_signing_name = signer_name;
		s->base.max_digest_size = signer_max;
		s->base.create_digest = signer_digest;
		s->cb = cb;
		s->user = user;
		s->max = max;
		s->dn.cn = dup_or_null(CTX, dn[0]);
		s->dn.o = dup_or_null(CTX, dn[1]);
		s->dn.ou = dup_or_null(CTX, dn[2]);
		s->dn.email = dup_or_null(CTX, dn[3]);
		s->dn.c = dup_or_null(CTX, dn[4]);

		if (num >= 0)
		{
			/* An empty signature field the form already has. */
			for (w = pdf_first_widget(CTX, page); w; w = pdf_next_widget(CTX, w))
				if (pdf_to_num(CTX, pdf_annot_obj(CTX, w)) == num)
					break;
			if (!w || pdf_widget_type(CTX, w) != PDF_WIDGET_TYPE_SIGNATURE)
				fz_throw(CTX, FZ_ERROR_ARGUMENT, "that isn't a signature field");
			pdf_keep_annot(CTX, w);
			if (pdf_widget_is_signed(CTX, w))
				fz_throw(CTX, FZ_ERROR_ARGUMENT, "that field is already signed");
		}
		else
		{
			snprintf(name, sizeof name, "Signature%d", pdf_count_signatures(CTX, page->doc) + 1);
			w = pdf_create_signature_widget(CTX, page, name);
			/* An empty box makes an invisible signature. */
			pdf_set_annot_rect(CTX, w, fz_is_empty_rect(rect) ? fz_make_rect(0, 0, 0, 0) : rect);
			/* MuPDF locks every form field by default; only when asked. */
			if (!lock)
				pdf_dict_del(CTX, pdf_annot_obj(CTX, w), PDF_NAME(Lock));
		}
		if (img && img_len)
		{
			ib = fz_new_buffer_from_copied_data(CTX, img, img_len);
			image = fz_new_image_from_buffer(CTX, ib);
		}
		pdf_sign_signature(CTX, w, &s->base, flags, image, reason && *reason ? reason : NULL, location && *location ? location : NULL);
		{
			/* MuPDF only draws these in the box; readers (Adobe, ours) look for them in
			 * the signature. They must go in the same revision as the signature: after a
			 * signing MuPDF puts any change in a new revision (not covered by it). */
			pdf_obj *v = pdf_dict_get(CTX, pdf_annot_obj(CTX, w), PDF_NAME(V));
			int was = page->doc->disallow_new_increments;
			page->doc->disallow_new_increments = 1;
			fz_try(CTX)
			{
				if (reason && *reason)
					pdf_dict_put_text_string(CTX, v, PDF_NAME(Reason), reason);
				if (location && *location)
					pdf_dict_put_text_string(CTX, v, PDF_NAME(Location), location);
				if (dn[0] && *dn[0])
					pdf_dict_put_text_string(CTX, v, PDF_NAME(Name), dn[0]);
			}
			fz_always(CTX)
				page->doc->disallow_new_increments = was;
			fz_catch(CTX)
				fz_rethrow(CTX);
		}
		pdf_end_operation(CTX, page->doc);
	}
	fz_always(CTX)
	{
		fz_drop_image(CTX, image);
		fz_drop_buffer(CTX, ib);
		pdf_drop_annot(CTX, w);
		/* The document keeps its own reference until it's saved. */
		if (s)
			pdf_drop_signer(CTX, &s->base);
	}
	fz_catch(CTX)
	{
		FAIL();
		pdf_abandon_operation(CTX, page->doc);
		return -1;
	}
	return 0;
}

/* Signature fields, found through the form's field tree (no pages loaded). */
#define MAX_SIGS 256

static int field_is_terminal(fz_context *ctx, pdf_obj *field)
{
	pdf_obj *kids = pdf_dict_get(ctx, field, PDF_NAME(Kids));
	int i, n = pdf_array_len(ctx, kids);
	for (i = 0; i < n; i++)
		if (pdf_dict_get(ctx, pdf_array_get(ctx, kids, i), PDF_NAME(T)))
			return 0;
	return 1;
}

static void collect_sigs(fz_context *ctx, pdf_obj *field, pdf_obj **out, int *n, int depth)
{
	if (depth > 32 || *n >= MAX_SIGS || pdf_mark_obj(ctx, field))
		return;
	fz_try(ctx)
	{
		if (field_is_terminal(ctx, field))
		{
			if (pdf_dict_get_inheritable(ctx, field, PDF_NAME(FT)) == PDF_NAME(Sig))
				out[(*n)++] = field;
		}
		else
		{
			pdf_obj *kids = pdf_dict_get(ctx, field, PDF_NAME(Kids));
			int i, k = pdf_array_len(ctx, kids);
			for (i = 0; i < k; i++)
				collect_sigs(ctx, pdf_array_get(ctx, kids, i), out, n, depth + 1);
		}
	}
	fz_always(ctx)
		pdf_unmark_obj(ctx, field);
	fz_catch(ctx)
		fz_rethrow(ctx);
}

static int find_sigs(fz_context *ctx, pdf_document *doc, pdf_obj **out)
{
	pdf_obj *fields = pdf_dict_getp(ctx, pdf_trailer(ctx, doc), "Root/AcroForm/Fields");
	int i, n = 0, k = pdf_array_len(ctx, fields);
	for (i = 0; i < k; i++)
		collect_sigs(ctx, pdf_array_get(ctx, fields, i), out, &n, 0);
	return n;
}

static int sig_page(fz_context *ctx, pdf_document *doc, pdf_obj *field)
{
	pdf_obj *widget = field;
	pdf_obj *kids = pdf_dict_get(ctx, field, PDF_NAME(Kids));
	if (pdf_array_len(ctx, kids) > 0)
		widget = pdf_array_get(ctx, kids, 0);
	int i, k, n;
	if (pdf_dict_get(ctx, widget, PDF_NAME(P)))
		return pdf_lookup_page_number(ctx, doc, pdf_dict_get(ctx, widget, PDF_NAME(P)));
	/* No /P (MuPDF doesn't write one for new signatures): look in the pages' Annots. */
	n = pdf_count_pages(ctx, doc);
	for (i = 0; i < n; i++)
	{
		pdf_obj *annots = pdf_dict_get(ctx, pdf_lookup_page_obj(ctx, doc, i), PDF_NAME(Annots));
		int m = pdf_array_len(ctx, annots);
		for (k = 0; k < m; k++)
			if (pdf_objcmp(ctx, pdf_array_get(ctx, annots, k), widget) == 0)
				return i;
	}
	return -1;
}

/* Where the signed revision ends: the end of the last byte range. */
static int64_t sig_end(fz_context *ctx, pdf_obj *field)
{
	pdf_obj *br = pdf_dict_getl(ctx, field, PDF_NAME(V), PDF_NAME(ByteRange), NULL);
	int n = pdf_array_len(ctx, br);
	if (n < 2)
		return 0;
	return pdf_array_get_int(ctx, br, n - 2) + pdf_array_get_int(ctx, br, n - 1);
}

/* JSON [{"i","name","page","signed","changed":bool (bytes added after it),
 * "invalidAt": saves ago it was invalidated by a disallowed change (0: none),
 * "reason","location","m" (PDF date),"sub" (SubFilter),"whole" (covers the file up to its update)}] */
char *fo_sigs_json(fo_ctx *c, fo_doc *d)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *sigs[MAX_SIGS];
	fz_buffer *buf = NULL;
	char *s = NULL;
	if (!pdf)
		return NULL;
	fz_var(buf);
	fz_try(CTX)
	{
		int i, n = find_sigs(CTX, pdf, sigs);
		buf = fz_new_buffer(CTX, 256);
		fz_append_byte(CTX, buf, '[');
		for (i = 0; i < n; i++)
		{
			pdf_obj *f = sigs[i];
			pdf_obj *v = pdf_dict_get_inheritable(CTX, f, PDF_NAME(V));
			int signed_ = pdf_signature_is_signed(CTX, pdf, f);
			int changed = 0, invalid = 0;
			char *name = pdf_load_field_name(CTX, f);
			if (signed_)
			{
				fz_try(CTX)
				{
					changed = pdf_signature_incremental_change_since_signing(CTX, pdf, f);
					invalid = pdf_validate_signature(CTX, pdf, f);
				}
				fz_catch(CTX)
					invalid = -2; /* damaged */
			}
			if (i)
				fz_append_byte(CTX, buf, ',');
			fz_append_printf(CTX, buf, "{\"i\":%d,\"name\":", i);
			json_str(CTX, buf, name);
			fz_free(CTX, name);
			{
				/* fz_append_printf has no 64-bit integers. */
				char end[32];
				snprintf(end, sizeof end, "%lld", (long long)sig_end(CTX, f));
				fz_append_printf(CTX, buf, ",\"page\":%d,\"signed\":%s,\"changed\":%s,\"invalidAt\":%d,\"end\":%s,\"reason\":",
					sig_page(CTX, pdf, f), signed_ ? "true" : "false", changed ? "true" : "false", invalid, end);
			}
			json_str(CTX, buf, pdf_dict_get_text_string(CTX, v, PDF_NAME(Reason)));
			fz_append_string(CTX, buf, ",\"location\":");
			json_str(CTX, buf, pdf_dict_get_text_string(CTX, v, PDF_NAME(Location)));
			fz_append_string(CTX, buf, ",\"m\":");
			json_str(CTX, buf, pdf_dict_get_text_string(CTX, v, PDF_NAME(M)));
			fz_append_string(CTX, buf, ",\"signer\":");
			json_str(CTX, buf, pdf_dict_get_text_string(CTX, v, PDF_NAME(Name)));
			fz_append_string(CTX, buf, ",\"sub\":");
			json_str(CTX, buf, pdf_to_name(CTX, pdf_dict_get(CTX, v, PDF_NAME(SubFilter))));
			fz_append_byte(CTX, buf, '}');
		}
		fz_append_byte(CTX, buf, ']');
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
		fz_drop_buffer(CTX, buf);
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* The bytes signature `i` covers and its CMS blob, for checking by the app.
 * Both are freed with fo_free. Returns 0, or -1. */
int fo_sig_data(fo_ctx *c, fo_doc *d, int i, unsigned char **data, size_t *len, unsigned char **cms, size_t *cms_len)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	pdf_obj *sigs[MAX_SIGS];
	fz_stream *stm = NULL;
	fz_buffer *buf = NULL;
	char *contents = NULL;
	*data = NULL;
	*cms = NULL;
	if (!pdf)
		return -1;
	fz_var(stm);
	fz_var(buf);
	fz_var(contents);
	fz_try(CTX)
	{
		int n = find_sigs(CTX, pdf, sigs);
		unsigned char *p;
		size_t l;
		if (i < 0 || i >= n || !pdf_signature_is_signed(CTX, pdf, sigs[i]))
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "not a signed signature field");
		*cms_len = pdf_signature_contents(CTX, pdf, sigs[i], &contents);
		stm = pdf_signature_hash_bytes(CTX, pdf, sigs[i]);
		buf = fz_read_all(CTX, stm, 1 << 20);
		l = fz_buffer_storage(CTX, buf, &p);
		*data = fz_malloc(CTX, l ? l : 1);
		memcpy(*data, p, l);
		*len = l;
		*cms = (unsigned char *)contents;
		contents = NULL;
	}
	fz_always(CTX)
	{
		fz_drop_stream(CTX, stm);
		fz_drop_buffer(CTX, buf);
		fz_free(CTX, contents);
	}
	fz_catch(CTX)
	{
		fz_free(CTX, *data);
		*data = NULL;
		FAIL();
		return -1;
	}
	return 0;
}

/* ---------------------------------------------------------------- OCR */

/* MuPDF (source/fitz/leptonica-wrap.c): Leptonica, which Tesseract draws its
 * images with, allocates through one global context. MuPDF sets it only while its
 * own OCR runs and refuses a second thread; ours is set once, to a context of its
 * own with no locks and an empty store, which amounts to plain malloc/free and is
 * safe from any number of threads. */
void fz_set_leptonica_mem(fz_context *ctx);

static fz_context *g_lept = NULL;

int fo_ocr_init(void)
{
	if (g_lept)
		return 0;
	g_lept = fz_new_context(NULL, NULL, FZ_STORE_UNLIMITED);
	if (!g_lept)
		return -1;
	fz_try(g_lept)
		fz_set_leptonica_mem(g_lept);
	fz_catch(g_lept)
		return -1;
	return 0;
}

/* Draws everything but text: what's left to read is in pictures or outlines. */
static void nt_fill_text(fz_context *ctx, fz_device *dev, const fz_text *text, fz_matrix ctm, fz_colorspace *cs, const float *color, float alpha, fz_color_params cp) {}
static void nt_stroke_text(fz_context *ctx, fz_device *dev, const fz_text *text, const fz_stroke_state *st, fz_matrix ctm, fz_colorspace *cs, const float *color, float alpha, fz_color_params cp) {}
static void nt_ignore_text(fz_context *ctx, fz_device *dev, const fz_text *text, fz_matrix ctm) {}

static fz_device *new_notext_device(fz_context *ctx, fz_device *target)
{
	fz_device *dev = fz_new_passthrough_device_of_size(ctx, target, sizeof(fz_device));
	dev->fill_text = nt_fill_text;
	dev->stroke_text = nt_stroke_text;
	dev->ignore_text = nt_ignore_text;
	return dev;
}

typedef struct
{
	fz_context *ctx;
	fz_buffer *buf;
	float s;
	int line;
	int words;
	int failed;
} ocr_out;

/* Called from C++: no MuPDF exception may escape it. */
static void ocr_word(void *arg, int line, const int lb[4], const int bl[4], const int wb[4], float conf, const char *text)
{
	ocr_out *o = (ocr_out *)arg;
	fz_context *ctx = o->ctx;
	float s = o->s;
	if (o->failed)
		return;
	fz_try(ctx)
	{
		if (line != o->line)
		{
			if (o->line >= 0)
				fz_append_string(ctx, o->buf, "]},");
			o->line = line;
			fz_append_printf(ctx, o->buf, "{\"b\":[%.2f,%.2f,%.2f,%.2f],\"bl\":[%.2f,%.2f,%.2f,%.2f],\"w\":[",
				lb[0] / s, lb[1] / s, lb[2] / s, lb[3] / s, bl[0] / s, bl[1] / s, bl[2] / s, bl[3] / s);
		}
		else
			fz_append_byte(ctx, o->buf, ',');
		fz_append_printf(ctx, o->buf, "[%.2f,%.2f,%.2f,%.2f,%.0f,", wb[0] / s, wb[1] / s, wb[2] / s, wb[3] / s, conf);
		json_str(ctx, o->buf, text);
		fz_append_byte(ctx, o->buf, ']');
		o->words++;
	}
	fz_catch(ctx)
		o->failed = 1;
}

/* Renders what isn't text yet in grey at scale: text drawing is left out, and text
 * the page already has but that isn't drawn (e.g. an earlier OCR layer over a scan)
 * is painted over. *dark counts the pixels darker than mid-grey. NULL if aborted. */
static fz_pixmap *render_untexted(fz_context *ctx, fz_display_list *l, fz_rect b, float scale, fz_cookie *k, size_t *dark)
{
	fz_pixmap *pix = NULL;
	fz_device *draw = NULL, *dev = NULL;
	fz_stext_page *text = NULL;
	int w, h;
	float bf[4] = { b.x0, b.y0, b.x1, b.y1 };
	fz_var(pix);
	fz_var(draw);
	fz_var(dev);
	fz_var(text);
	fo_render_size(bf, scale, 0, &w, &h);
	if (w < 8 || h < 8)
		fz_throw(ctx, FZ_ERROR_ARGUMENT, "page too small");
	fz_try(ctx)
	{
		fz_matrix m = page_matrix(b, scale, 0);
		fz_stext_options opts = { 0 };
		pix = fz_new_pixmap_with_bbox(ctx, fz_device_gray(ctx), fz_make_irect(0, 0, w, h), NULL, 0);
		fz_clear_pixmap_with_value(ctx, pix, 255);
		draw = fz_new_draw_device(ctx, fz_identity, pix);
		dev = new_notext_device(ctx, draw);
		fz_run_display_list(ctx, l, dev, m, fz_infinite_rect, k);
		fz_close_device(ctx, dev);
		if (k && k->abort)
			break;

		text = fz_new_stext_page_from_display_list(ctx, l, &opts);
		for (fz_stext_block *block = text->first_block; block; block = block->next)
		{
			if (block->type != FZ_STEXT_BLOCK_TEXT)
				continue;
			for (fz_stext_line *line = block->u.t.first_line; line; line = line->next)
			{
				fz_irect r = fz_round_rect(fz_transform_rect(line->bbox, m));
				r.x0 = fz_maxi(r.x0 - 2, 0);
				r.y0 = fz_maxi(r.y0 - 2, 0);
				r.x1 = fz_mini(r.x1 + 2, w);
				r.y1 = fz_mini(r.y1 + 2, h);
				for (int y = r.y0; y < r.y1; y++)
					if (r.x1 > r.x0)
						memset(pix->samples + (size_t)y * pix->stride + r.x0, 255, (size_t)(r.x1 - r.x0));
			}
		}

		*dark = 0;
		for (int y = 0; y < h; y++)
		{
			const unsigned char *p = pix->samples + (size_t)y * pix->stride;
			for (int x = 0; x < w; x++)
				*dark += p[x] < 128;
		}
	}
	fz_always(ctx)
	{
		fz_drop_device(ctx, dev);
		fz_drop_device(ctx, draw);
		fz_drop_stext_page(ctx, text);
	}
	fz_catch(ctx)
	{
		fz_drop_pixmap(ctx, pix);
		fz_rethrow(ctx);
	}
	if (k && k->abort)
	{
		fz_drop_pixmap(ctx, pix);
		return NULL;
	}
	return pix;
}

/* How much of the page is left to read (see render_untexted), in parts per 10,000
 * of its area, from a quick low-resolution render. -1 on error. */
int fo_ocr_ink(fo_ctx *c, fo_list *l, const float bf[4])
{
	fz_pixmap *pix = NULL;
	size_t dark = 0;
	int ink = -1;
	fz_var(pix);
	fz_try(CTX)
	{
		pix = render_untexted(CTX, (fz_display_list *)l, fz_make_rect(bf[0], bf[1], bf[2], bf[3]), 50.0f / 72, NULL, &dark);
		ink = (int)(dark * 10000 / ((size_t)pix->w * pix->h));
	}
	fz_always(CTX)
		fz_drop_pixmap(CTX, pix);
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return ink;
}

/* Recognises a page: renders what isn't text yet (see render_untexted), then runs
 * Tesseract on it.
 * JSON {"l":[{"b":[line box],"bl":[baseline x1,y1,x2,y2],"w":[[x0,y0,x1,y1,conf,"text"],...]}],"blank":1?}
 * in points relative to the page origin. *aborted is set when k was aborted. */
char *fo_ocr_page(fo_ctx *c, fo_list *l, const float bf[4], float dpi, po_tess *t, fo_cookie *k, int *aborted)
{
	fz_pixmap *pix = NULL;
	fz_buffer *buf = NULL;
	char *s = NULL;
	*aborted = 0;
	fz_var(pix);
	fz_var(buf);
	fz_try(CTX)
	{
		float scale = dpi / 72;
		int w, h, rc;
		size_t dark = 0;
		ocr_out out = { 0 };

		/* Very large pages (posters, drawings) are read at a lower resolution. */
		fo_render_size(bf, scale, 0, &w, &h);
		if ((double)w * h > 40e6)
			scale *= (float)sqrt(40e6 / ((double)w * h));
		pix = render_untexted(CTX, (fz_display_list *)l, fz_make_rect(bf[0], bf[1], bf[2], bf[3]), scale, (fz_cookie *)k, &dark);
		if (!pix)
		{
			*aborted = 1;
			break;
		}

		buf = fz_new_buffer(CTX, 4096);
		fz_append_string(CTX, buf, "{\"l\":[");

		/* Nothing (or only specks) left: no need to wake Tesseract. */
		if (dark < (size_t)pix->w * pix->h / 5000)
		{
			fz_append_string(CTX, buf, "],\"blank\":1}");
			s = buffer_to_string(CTX, buf);
			break;
		}

		out.ctx = CTX;
		out.buf = buf;
		out.s = scale;
		out.line = -1;
		rc = po_tess_recognize(t, pix->samples, pix->w, pix->h, (int)pix->stride, (int)(scale * 72 + 0.5f), ocr_word, &out,
			k ? &((fz_cookie *)k)->abort : NULL);
		if (rc == 1)
		{
			*aborted = 1;
			break;
		}
		if (rc < 0 || out.failed)
			fz_throw(CTX, FZ_ERROR_LIBRARY, "text recognition failed");
		if (out.line >= 0)
			fz_append_string(CTX, buf, "]}");
		fz_append_string(CTX, buf, "]}");
		s = buffer_to_string(CTX, buf);
	}
	fz_always(CTX)
	{
		fz_drop_pixmap(CTX, pix);
		fz_drop_buffer(CTX, buf);
	}
	fz_catch(CTX)
	{
		FAIL();
		return NULL;
	}
	return s;
}

/* The invisible font of recognised text (as Tesseract and MuPDF write it): every
 * character is one blank glyph half an em wide, and ToUnicode maps each code
 * straight to the character it stands for. */
static const char glyphless_cidtogid[] =
"\x78\x9c\xec\xc2\x01\x09\x00\x00\x00\x02\xa0\xfa\x7f\xba\x21\x89"
	"\xa6\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x80\x7b\x03\x00\x00\xff\xff\xec\xc2\x01\x0d\x00\x00\x00\xc2\x20"
	"\xdf\xbf\xb4\x45\x18\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\xeb\x00\x00\x00\xff\xff\xec\xc2\x01\x0d\x00"
	"\x00\x00\xc2\x20\xdf\xbf\xb4\x45\x18\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\xeb\x00\x00\x00\xff\xff\xed"
	"\xc2\x01\x0d\x00\x00\x00\xc2\x20\xdf\xbf\xb4\x45\x18\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xeb\x00\xff"
	"\x00\x10";

static const char glyphless_ttf[] =
"\x00\x01\x00\x00\x00\x0a\x00\x80\x00\x03\x00\x20\x4f\x53\x2f\x32"
	"\x56\xde\xc8\x94\x00\x00\x01\x28\x00\x00\x00\x60\x63\x6d\x61\x70"
	"\x00\x0a\x00\x34\x00\x00\x01\x90\x00\x00\x00\x1e\x67\x6c\x79\x66"
	"\x15\x22\x41\x24\x00\x00\x01\xb8\x00\x00\x00\x18\x68\x65\x61\x64"
	"\x0b\x78\xf1\x65\x00\x00\x00\xac\x00\x00\x00\x36\x68\x68\x65\x61"
	"\x0c\x02\x04\x02\x00\x00\x00\xe4\x00\x00\x00\x24\x68\x6d\x74\x78"
	"\x04\x00\x00\x00\x00\x00\x01\x88\x00\x00\x00\x08\x6c\x6f\x63\x61"
	"\x00\x0c\x00\x00\x00\x00\x01\xb0\x00\x00\x00\x06\x6d\x61\x78\x70"
	"\x00\x04\x00\x05\x00\x00\x01\x08\x00\x00\x00\x20\x6e\x61\x6d\x65"
	"\xf2\xeb\x16\xda\x00\x00\x01\xd0\x00\x00\x00\x4b\x70\x6f\x73\x74"
	"\x00\x01\x00\x01\x00\x00\x02\x1c\x00\x00\x00\x20\x00\x01\x00\x00"
	"\x00\x01\x00\x00\xb0\x94\x71\x10\x5f\x0f\x3c\xf5\x04\x07\x08\x00"
	"\x00\x00\x00\x00\xcf\x9a\xfc\x6e\x00\x00\x00\x00\xd4\xc3\xa7\xf2"
	"\x00\x00\x00\x00\x04\x00\x08\x00\x00\x00\x00\x10\x00\x02\x00\x00"
	"\x00\x00\x00\x00\x00\x01\x00\x00\x08\x00\xff\xff\x00\x00\x04\x00"
	"\x00\x00\x00\x00\x04\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x02\x00\x01\x00\x00\x00\x02\x00\x04"
	"\x00\x01\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03\x00\x00\x01\x90\x00\x05"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x05\x00\x01\x00\x01\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x47\x4f\x4f\x47\x00\x40\x00\x00\x00\x00\x00\x01\xff\xff"
	"\x00\x00\x00\x01\x00\x01\x80\x00\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x04\x00\x00\x00"
	"\x00\x00\x00\x02\x00\x01\x00\x00\x00\x00\x00\x14\x00\x03\x00\x00"
	"\x00\x00\x00\x14\x00\x06\x00\x0a\x00\x00\x00\x00\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x0c\x00\x00\x00\x01\x00\x00\x00\x00\x04\x00"
	"\x08\x00\x00\x03\x00\x00\x31\x21\x11\x21\x04\x00\xfc\x00\x08\x00"
	"\x00\x00\x00\x03\x00\x2a\x00\x00\x00\x03\x00\x00\x00\x05\x00\x16"
	"\x00\x00\x00\x01\x00\x00\x00\x00\x00\x05\x00\x0b\x00\x16\x00\x03"
	"\x00\x01\x04\x09\x00\x05\x00\x16\x00\x00\x00\x56\x00\x65\x00\x72"
	"\x00\x73\x00\x69\x00\x6f\x00\x6e\x00\x20\x00\x31\x00\x2e\x00\x30"
	"\x56\x65\x72\x73\x69\x6f\x6e\x20\x31\x2e\x30\x00\x00\x01\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00"
	"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";

static const char glyphless_tounicode[] =
	"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
	"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
	"/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n"
	"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
	"1 beginbfrange\n<0000> <FFFF> <0000>\nendbfrange\n"
	"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";

static pdf_obj *add_stream_bytes(fz_context *ctx, pdf_document *doc, const char *data, size_t len, pdf_obj *dict, int compressed)
{
	fz_buffer *b = fz_new_buffer_from_copied_data(ctx, (const unsigned char *)data, len);
	pdf_obj *ref = NULL;
	fz_try(ctx)
		ref = pdf_add_stream(ctx, doc, b, dict, compressed);
	fz_always(ctx)
		fz_drop_buffer(ctx, b);
	fz_catch(ctx)
		fz_rethrow(ctx);
	return ref;
}

static pdf_obj *add_glyphless_font(fz_context *ctx, pdf_document *doc)
{
	pdf_obj *d = NULL, *map = NULL, *file = NULL, *uni = NULL, *desc = NULL, *cid = NULL, *font = NULL, *arr;
	fz_var(d);
	fz_var(map);
	fz_var(file);
	fz_var(uni);
	fz_var(desc);
	fz_var(cid);
	fz_try(ctx)
	{
		d = pdf_new_dict(ctx, doc, 1);
		pdf_dict_put(ctx, d, PDF_NAME(Filter), PDF_NAME(FlateDecode));
		map = add_stream_bytes(ctx, doc, glyphless_cidtogid, sizeof glyphless_cidtogid - 1, d, 1);
		pdf_drop_obj(ctx, d);
		d = pdf_new_dict(ctx, doc, 1);
		pdf_dict_put_int(ctx, d, PDF_NAME(Length1), sizeof glyphless_ttf - 1);
		file = add_stream_bytes(ctx, doc, glyphless_ttf, sizeof glyphless_ttf - 1, d, 0);
		uni = add_stream_bytes(ctx, doc, glyphless_tounicode, sizeof glyphless_tounicode - 1, NULL, 0);

		desc = pdf_add_new_dict(ctx, doc, 10);
		pdf_dict_put(ctx, desc, PDF_NAME(Type), PDF_NAME(FontDescriptor));
		pdf_dict_put_name(ctx, desc, PDF_NAME(FontName), "GlyphLessFont");
		pdf_dict_put_int(ctx, desc, PDF_NAME(Flags), 5);
		pdf_dict_put_rect(ctx, desc, PDF_NAME(FontBBox), fz_make_rect(0, 0, 500, 1000));
		pdf_dict_put_int(ctx, desc, PDF_NAME(ItalicAngle), 0);
		pdf_dict_put_int(ctx, desc, PDF_NAME(Ascent), 1000);
		pdf_dict_put_int(ctx, desc, PDF_NAME(Descent), -1);
		pdf_dict_put_int(ctx, desc, PDF_NAME(CapHeight), 1000);
		pdf_dict_put_int(ctx, desc, PDF_NAME(StemV), 80);
		pdf_dict_put(ctx, desc, PDF_NAME(FontFile2), file);

		cid = pdf_add_new_dict(ctx, doc, 8);
		pdf_dict_put(ctx, cid, PDF_NAME(Type), PDF_NAME(Font));
		pdf_dict_put(ctx, cid, PDF_NAME(Subtype), PDF_NAME(CIDFontType2));
		pdf_dict_put_name(ctx, cid, PDF_NAME(BaseFont), "GlyphLessFont");
		{
			pdf_obj *info = pdf_dict_put_dict(ctx, cid, PDF_NAME(CIDSystemInfo), 3);
			pdf_dict_put_string(ctx, info, PDF_NAME(Registry), "Adobe", 5);
			pdf_dict_put_string(ctx, info, PDF_NAME(Ordering), "Identity", 8);
			pdf_dict_put_int(ctx, info, PDF_NAME(Supplement), 0);
		}
		pdf_dict_put(ctx, cid, PDF_NAME(FontDescriptor), desc);
		pdf_dict_put(ctx, cid, PDF_NAME(CIDToGIDMap), map);
		pdf_dict_put_int(ctx, cid, PDF_NAME(DW), 500);

		font = pdf_add_new_dict(ctx, doc, 6);
		pdf_dict_put(ctx, font, PDF_NAME(Type), PDF_NAME(Font));
		pdf_dict_put(ctx, font, PDF_NAME(Subtype), PDF_NAME(Type0));
		pdf_dict_put_name(ctx, font, PDF_NAME(BaseFont), "GlyphLessFont");
		pdf_dict_put(ctx, font, PDF_NAME(Encoding), PDF_NAME(Identity_H));
		arr = pdf_dict_put_array(ctx, font, PDF_NAME(DescendantFonts), 1);
		pdf_array_push(ctx, arr, cid);
		pdf_dict_put(ctx, font, PDF_NAME(ToUnicode), uni);
	}
	fz_always(ctx)
	{
		pdf_drop_obj(ctx, d);
		pdf_drop_obj(ctx, map);
		pdf_drop_obj(ctx, file);
		pdf_drop_obj(ctx, uni);
		pdf_drop_obj(ctx, desc);
		pdf_drop_obj(ctx, cid);
	}
	fz_catch(ctx)
	{
		pdf_drop_obj(ctx, font);
		fz_rethrow(ctx);
	}
	return font;
}

static int is_ocr_stream(fz_context *ctx, pdf_obj *o)
{
	return pdf_to_bool(ctx, pdf_dict_gets(ctx, o, "PaginautOCR"));
}

static pdf_obj *add_ocr_stream(fz_context *ctx, pdf_document *doc, fz_buffer *b)
{
	pdf_obj *d = pdf_new_dict(ctx, doc, 1);
	pdf_obj *ref = NULL;
	fz_try(ctx)
	{
		pdf_dict_puts_drop(ctx, d, "PaginautOCR", PDF_TRUE);
		ref = pdf_add_stream(ctx, doc, b, d, 0);
	}
	fz_always(ctx)
		pdf_drop_obj(ctx, d);
	fz_catch(ctx)
		fz_rethrow(ctx);
	return ref;
}

int fo_ocr_apply(fo_ctx *c, fo_page *p, const char *content, size_t len, int *font_num)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	pdf_obj *font = NULL, *pre = NULL, *post = NULL;
	fz_buffer *b = NULL;
	if (!page)
	{
		snprintf(c->err, sizeof c->err, "not a PDF page");
		return -1;
	}
	fz_var(font);
	fz_var(pre);
	fz_var(post);
	fz_var(b);
	fz_try(CTX)
	{
		pdf_document *doc = page->doc;
		pdf_obj *pobj = page->obj;
		pdf_obj *contents = pdf_dict_get(CTX, pobj, PDF_NAME(Contents));
		pdf_obj *arr, *res, *fonts;
		fz_matrix ctm, inv;

		/* Removing: drop every layer added earlier. */
		if (len == 0)
		{
			for (int i = pdf_array_len(CTX, contents) - 1; i >= 0; i--)
				if (is_ocr_stream(CTX, pdf_array_get(CTX, contents, i)))
					pdf_array_delete(CTX, contents, i);
			break;
		}

		/* Adding: a new layer next to any earlier ones (each run reads only what
		 * isn't text yet, so layers never cover the same words). */
		if (pdf_is_array(CTX, contents))
			arr = contents;
		else
		{
			arr = pdf_new_array(CTX, doc, 3);
			if (contents)
				pdf_array_push(CTX, arr, contents);
			pdf_dict_put_drop(CTX, pobj, PDF_NAME(Contents), arr);
		}

		if (*font_num > 0)
			font = pdf_new_indirect(CTX, doc, *font_num, 0);
		else
		{
			font = add_glyphless_font(CTX, doc);
			*font_num = pdf_to_num(CTX, font);
		}
		res = pdf_dict_get(CTX, pobj, PDF_NAME(Resources));
		if (!res)
		{
			pdf_obj *inherited = pdf_dict_get_inheritable(CTX, pobj, PDF_NAME(Resources));
			res = inherited ? pdf_copy_dict(CTX, inherited) : pdf_new_dict(CTX, doc, 1);
			pdf_dict_put_drop(CTX, pobj, PDF_NAME(Resources), res);
		}
		fonts = pdf_dict_get(CTX, res, PDF_NAME(Font));
		if (!fonts)
			fonts = pdf_dict_put_dict(CTX, res, PDF_NAME(Font), 1);
		pdf_dict_puts(CTX, fonts, "PgnOCR", font);

		/* The page's own content runs inside q â€¦ Q, so whatever state it leaves
		 * behind can't move the text. The text is in page space as shown (origin
		 * top-left, y down); the matrix takes it back to the PDF's own space. */
		pdf_page_transform(CTX, page, NULL, &ctm);
		inv = fz_invert_matrix(ctm);
		b = fz_new_buffer_from_copied_data(CTX, (const unsigned char *)"q\n", 2);
		pre = add_ocr_stream(CTX, doc, b);
		fz_drop_buffer(CTX, b);
		b = NULL;
		b = fz_new_buffer(CTX, len + 128);
		fz_append_printf(CTX, b, "Q\nq\n%g %g %g %g %g %g cm\n", inv.a, inv.b, inv.c, inv.d, inv.e, inv.f);
		fz_append_data(CTX, b, content, len);
		fz_append_string(CTX, b, "\nQ\n");
		post = add_ocr_stream(CTX, doc, b);
		pdf_array_insert(CTX, arr, pre, 0);
		pdf_array_push(CTX, arr, post);
	}
	fz_always(CTX)
	{
		pdf_drop_obj(CTX, font);
		pdf_drop_obj(CTX, pre);
		pdf_drop_obj(CTX, post);
		fz_drop_buffer(CTX, b);
	}
	fz_catch(CTX)
	{
		FAIL();
		return -1;
	}
	return 0;
}

/* 1 if the page carries a text layer made by fo_ocr_apply. */
int fo_ocr_has_layer(fo_ctx *c, fo_page *p)
{
	pdf_page *page = pdf_page_from_fz_page(CTX, (fz_page *)p);
	int found = 0;
	if (!page)
		return 0;
	fz_try(CTX)
	{
		pdf_obj *contents = pdf_dict_get(CTX, page->obj, PDF_NAME(Contents));
		for (int i = 0; i < pdf_array_len(CTX, contents) && !found; i++)
			found = is_ocr_stream(CTX, pdf_array_get(CTX, contents, i));
	}
	fz_catch(CTX)
		return 0;
	return found;
}

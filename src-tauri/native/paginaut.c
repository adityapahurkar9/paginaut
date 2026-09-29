#include "paginaut.h"
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

static int char_style(fz_context *ctx, fz_stext_char *ch, float line_size, float baseline)
{
	int s = 0;
	if ((ch->flags & FZ_STEXT_BOLD) || (ch->font && fz_font_is_bold(ctx, ch->font)))
		s |= 1;
	if (ch->font && fz_font_is_italic(ctx, ch->font))
		s |= 2;
	if (ch->font && fz_font_is_monospaced(ctx, ch->font))
		s |= 4;
	if (ch->size < 0.8f * line_size && ch->origin.y < baseline - 0.15f * line_size)
		s |= 8;
	return s;
}

static void reflow_line(fz_context *ctx, fz_buffer *buf, fz_buffer *span, fz_stext_line *line, const float o[4])
{
	float line_size = 0, baseline = 0, size = -1;
	int style = -1, spans = 0;
	fz_stext_char *ch;
	for (ch = line->first_char; ch; ch = ch->next)
		if (ch->size > line_size)
		{
			line_size = ch->size;
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
		float sz = floorf(ch->size * 2 + 0.5f) / 2;
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

/* Undo (redo = 0) or redo one journal step. Returns 1 if a step was taken. */
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
			/* The journal restores the fields' drawn appearance too; redrawing
			 * here would be a new edit and wipe out the redo history. */
			if (redo)
				pdf_redo(CTX, pdf);
			else
				pdf_undo(CTX, pdf);
			done = 1;
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

int fo_pdf_apply_page_edits(fo_ctx *c, fo_doc *d, const int *rot, int npages, const int *order, int count)
{
	pdf_document *pdf = pdf_specifics(CTX, (fz_document *)d);
	int identity = 1;
	if (!pdf)
	{
		snprintf(c->err, sizeof c->err, "not a PDF");
		return -1;
	}
	fz_try(CTX)
	{
		int n = pdf_count_pages(CTX, pdf);
		if (npages != n)
			fz_throw(CTX, FZ_ERROR_ARGUMENT, "page count changed (%d vs %d)", npages, n);
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
			pdf_rearrange_pages(CTX, pdf, count, order, PDF_CLEAN_STRUCTURE_KEEP);
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

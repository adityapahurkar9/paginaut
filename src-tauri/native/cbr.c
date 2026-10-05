/*
 * RAR archives for MuPDF, so comic books packed with RAR (CBR) open like CBZ ones. Two readers,
 * each where the other can't go:
 *   RAR 1.5 to 4: unarr (native/unarr, LGPL; from The Unarchiver), which reads solid archives.
 *   RAR 5: libarchive's reader (native/libarchive, BSD), the only one there is under a free licence.
 * Both read forwards only. A plain archive is cheap to step through (data is skipped by seeking),
 * so its pages are read from it directly. A solid one is one compressed stream, where reaching a
 * page means unpacking everything before it: its entries are unpacked once, while they're listed,
 * into a temporary file that pages are then read from. That file is deleted however the app ends:
 * on Windows it's opened delete-on-close (the system deletes it when the last handle goes, crash
 * or not); elsewhere it's unlinked as soon as it's made, so it has no name to leave behind.
 *
 * After MuPDF's own libarchive handler (source/fitz/unlibarchive.c), which MuPDF's build leaves
 * out; this one has its own names, as MuPDF's libraries carry stubs of those.
 */
#include <mupdf/fitz.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "libarchive/archive.h"
#include "libarchive/archive_entry.h"
#include "unarr/unarr.h"

/* unarr's way to read from a stream of our own (common/unarr-imp.h, not in its public header). */
typedef void (*ar_stream_close_fn)(void *data);
typedef size_t (*ar_stream_read_fn)(void *data, void *buffer, size_t count);
typedef bool (*ar_stream_seek_fn)(void *data, off64_t offset, int origin);
typedef off64_t (*ar_stream_tell_fn)(void *data);
ar_stream *ar_open_stream(void *data, ar_stream_close_fn close, ar_stream_read_fn read, ar_stream_seek_fn seek, ar_stream_tell_fn tell);

#ifdef _WIN32
#define temp_open(p) ((FILE *)fz_fopen_utf8((p), "w+bD"))
#define temp_seek(f, o) _fseeki64((f), (o), SEEK_SET)
#else
#define temp_open(p) fopen((p), "w+b")
#define temp_seek(f, o) fseeko((f), (off_t)(o), SEEK_SET)
#endif

typedef struct
{
	size_t len;
	char *name;
	/* Where its data is in the temporary file (solid archives). */
	int64_t at;
	/* RAR 4: where its header is, for unarr to go back to. */
	off64_t offset;
} rar_entry;

typedef struct
{
	fz_archive super;
	int v5;
	/* RAR 5: reading the archive (plain archives; opened on the first read), and entries read past. */
	struct archive *archive;
	int current;
	/* RAR 4: unarr's reader. */
	ar_stream *ustream;
	ar_archive *uarch;
	int count, max;
	rar_entry *entries;
	/* A solid archive's entries, unpacked (NULL for plain archives, or with nowhere to put them). */
	FILE *unpacked;
	fz_context *ctx; /* while a reader calls back */
	uint8_t block[4096];
} rar_archive;

/* Where solid archives are unpacked (set at startup: po_set_temp_dir). */
static char *temp_dir = NULL;

void po_set_temp_dir(const char *dir)
{
	char *d = NULL;
	if (dir)
	{
		size_t n = strlen(dir) + 1;
		d = malloc(n);
		if (d)
			memcpy(d, dir, n);
	}
	free(temp_dir);
	temp_dir = d;
}

/* ---------------------------------------------------------------- RAR 5: libarchive's callbacks */

static la_ssize_t la_read(struct archive *a, void *data, const void **buf)
{
	rar_archive *arch = data;
	fz_context *ctx = arch->ctx;
	la_ssize_t ret = 0;
	fz_try(ctx)
	{
		size_t z = fz_available(ctx, arch->super.file, sizeof arch->block);
		if (z > sizeof arch->block)
			z = sizeof arch->block;
		memcpy(arch->block, arch->super.file->rp, z);
		arch->super.file->rp += z;
		*buf = arch->block;
		ret = (la_ssize_t)z;
	}
	fz_catch(ctx)
	{
		archive_set_error(a, ARCHIVE_FATAL, "%s", fz_convert_error(ctx, NULL));
		return -1;
	}
	return ret;
}

static la_int64_t la_skip(struct archive *a, void *data, la_int64_t skip)
{
	rar_archive *arch = data;
	fz_context *ctx = arch->ctx;
	int64_t moved = 0;
	fz_try(ctx)
	{
		int64_t pos = fz_tell(ctx, arch->super.file);
		fz_seek(ctx, arch->super.file, pos + skip, SEEK_SET);
		moved = fz_tell(ctx, arch->super.file) - pos;
	}
	fz_catch(ctx)
	{
		archive_set_error(a, ARCHIVE_FATAL, "%s", fz_convert_error(ctx, NULL));
		return -1;
	}
	return moved;
}

static la_int64_t la_seek(struct archive *a, void *data, la_int64_t offset, int whence)
{
	rar_archive *arch = data;
	fz_context *ctx = arch->ctx;
	int64_t pos = 0;
	fz_try(ctx)
	{
		fz_seek(ctx, arch->super.file, offset, whence);
		pos = fz_tell(ctx, arch->super.file);
	}
	fz_catch(ctx)
	{
		archive_set_error(a, ARCHIVE_FATAL, "%s", fz_convert_error(ctx, NULL));
		return -1;
	}
	return pos;
}

static int la_close(struct archive *a, void *data)
{
	return ARCHIVE_OK; /* the stream goes with the fz_archive */
}

/* Starts reading a RAR 5 archive from the beginning; nonzero on failure. */
static int la_begin(fz_context *ctx, rar_archive *arch)
{
	int r;
	if (arch->archive)
		archive_read_free(arch->archive);
	fz_seek(ctx, arch->super.file, 0, SEEK_SET);
	arch->archive = archive_read_new();
	archive_read_support_format_rar5(arch->archive);
	arch->ctx = ctx;
	r = archive_read_set_seek_callback(arch->archive, la_seek);
	if (r == ARCHIVE_OK)
		r = archive_read_open2(arch->archive, arch, NULL, la_read, la_skip, la_close);
	arch->ctx = NULL;
	if (r != ARCHIVE_OK)
	{
		archive_read_free(arch->archive);
		arch->archive = NULL;
	}
	arch->current = 0;
	return r != ARCHIVE_OK;
}

/* An entry header (or skip) read: ARCHIVE_WARN is fine (e.g. a name that can't be converted to UTF-8). */
#define LA_OK(r) ((r) == ARCHIVE_OK || (r) == ARCHIVE_WARN)

/* ---------------------------------------------------------------- RAR 4: unarr's stream */

static void ua_close(void *data)
{
	/* the stream goes with the fz_archive */
}

static size_t ua_read(void *data, void *buffer, size_t count)
{
	rar_archive *arch = data;
	fz_context *ctx = arch->ctx;
	size_t n = 0;
	fz_try(ctx)
		n = fz_read(ctx, arch->super.file, buffer, count);
	fz_catch(ctx)
		return 0;
	return n;
}

static bool ua_seek(void *data, off64_t offset, int origin)
{
	rar_archive *arch = data;
	fz_context *ctx = arch->ctx;
	fz_try(ctx)
		fz_seek(ctx, arch->super.file, offset, origin);
	fz_catch(ctx)
		return false;
	return true;
}

static off64_t ua_tell(void *data)
{
	rar_archive *arch = data;
	fz_context *ctx = arch->ctx;
	off64_t pos = -1;
	fz_try(ctx)
		pos = fz_tell(ctx, arch->super.file);
	fz_catch(ctx)
		return -1;
	return pos;
}

/* Unpacks the current RAR 4 entry, `n` bytes, `chunk` bytes at a time, to `out` (a buffer) or `f`. */
static int ua_unpack(rar_archive *arch, size_t n, unsigned char *out, FILE *f, unsigned char *chunk, size_t chunk_size)
{
	while (n > 0)
	{
		size_t k = n < chunk_size ? n : chunk_size;
		unsigned char *dst = out ? out : chunk;
		if (!ar_entry_uncompress(arch->uarch, dst, k))
			return -1;
		if (out)
			out += k;
		else if (fwrite(chunk, 1, k, f) != k)
			return 1;
		n -= k;
	}
	return 0;
}

/* ---------------------------------------------------------------- recognising */

/* RAR 1.5 to 4 ("Rar!\x1a\x07\x00") and RAR 5 ("Rar!\x1a\x07\x01\x00"). */
static int recognize_rar(fz_context *ctx, fz_stream *file)
{
	unsigned char sig[8] = { 0 };
	fz_seek(ctx, file, 0, SEEK_SET);
	if (fz_read(ctx, file, sig, 8) < 7)
		return 0;
	if (memcmp(sig, "Rar!\x1a\x07", 6))
		return 0;
	return sig[6] == 0 || (sig[6] == 1 && sig[7] == 0);
}

/* Reads a variable-length RAR 5 number at h[*i]. */
static uint64_t vint(const unsigned char *h, size_t n, size_t *i)
{
	uint64_t v = 0;
	int shift = 0;
	while (*i < n && shift < 64)
	{
		unsigned char b = h[(*i)++];
		v |= (uint64_t)(b & 0x7f) << shift;
		if (!(b & 0x80))
			break;
		shift += 7;
	}
	return v;
}

/* Whether the archive is solid: the main header's flag (RAR 4: 0x0008; RAR 5: archive flags 0x0004). */
static int rar_is_solid(fz_context *ctx, fz_stream *file)
{
	unsigned char h[64];
	size_t n, i;
	fz_seek(ctx, file, 0, SEEK_SET);
	n = fz_read(ctx, file, h, sizeof h);
	if (n >= 14 && h[6] == 0)
		/* Marker (7), then the main header: CRC (2), type 0x73, flags (2). */
		return h[9] == 0x73 && ((h[10] | h[11] << 8) & 0x0008) != 0;
	if (n > 12 && h[6] == 1)
	{
		uint64_t flags;
		/* Signature (8), CRC32 (4), size, type 1, flags, [extra size], [data size], archive flags. */
		i = 12;
		vint(h, n, &i);
		if (vint(h, n, &i) != 1)
			return 0;
		flags = vint(h, n, &i);
		if (flags & 1)
			vint(h, n, &i);
		if (flags & 2)
			vint(h, n, &i);
		return (vint(h, n, &i) & 0x0004) != 0;
	}
	return 0;
}

/* ---------------------------------------------------------------- the fz_archive */

static void drop_rar(fz_context *ctx, fz_archive *arch_)
{
	rar_archive *arch = (rar_archive *)arch_;
	int i;
	if (arch->archive)
		archive_read_free(arch->archive);
	if (arch->uarch)
		ar_close_archive(arch->uarch);
	if (arch->ustream)
		ar_close(arch->ustream);
	if (arch->unpacked)
		fclose(arch->unpacked);
	for (i = 0; i < arch->count; i++)
		fz_free(ctx, arch->entries[i].name);
	fz_free(ctx, arch->entries);
}

static int find_entry(rar_archive *arch, const char *name)
{
	int i;
	if (!*name)
		return -1;
	for (i = 0; i < arch->count; i++)
		if (!strcmp(name, arch->entries[i].name))
			return i;
	return -1;
}

static int has_rar_entry(fz_context *ctx, fz_archive *arch, const char *name)
{
	return find_entry((rar_archive *)arch, name) >= 0;
}

static const char *list_rar_entry(fz_context *ctx, fz_archive *arch_, int i)
{
	rar_archive *arch = (rar_archive *)arch_;
	return i >= 0 && i < arch->count ? arch->entries[i].name : NULL;
}

static int count_rar_entries(fz_context *ctx, fz_archive *arch)
{
	return ((rar_archive *)arch)->count;
}

static fz_buffer *read_unpacked(fz_context *ctx, rar_archive *arch, rar_entry *e)
{
	fz_buffer *buf = fz_new_buffer(ctx, e->len ? e->len : 1);
	if (temp_seek(arch->unpacked, e->at) != 0 || fread(buf->data, 1, e->len, arch->unpacked) != e->len)
	{
		fz_drop_buffer(ctx, buf);
		fz_throw(ctx, FZ_ERROR_SYSTEM, "cannot read an unpacked RAR entry");
	}
	buf->len = e->len;
	return buf;
}

/* RAR 4: unarr goes to the entry's header (in a solid archive, unpacking from the start again). */
static fz_buffer *read_v4(fz_context *ctx, rar_archive *arch, rar_entry *e)
{
	fz_buffer *buf = fz_new_buffer(ctx, e->len ? e->len : 1);
	int ok;
	arch->ctx = ctx;
	ok = ar_parse_entry_at(arch->uarch, e->offset) && ua_unpack(arch, e->len, buf->data, NULL, NULL, e->len ? e->len : 1) == 0;
	arch->ctx = NULL;
	if (!ok)
	{
		fz_drop_buffer(ctx, buf);
		fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot unpack a RAR entry");
	}
	buf->len = e->len;
	return buf;
}

/* RAR 5: libarchive steps forwards to the entry (going back starts over; data is skipped by seeking). */
static fz_buffer *read_v5(fz_context *ctx, rar_archive *arch, int idx)
{
	struct archive_entry *entry;
	fz_buffer *buf = NULL;
	if (!arch->archive || arch->current > idx)
		if (la_begin(ctx, arch))
			fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot reopen the RAR archive");
	fz_var(buf);
	arch->ctx = ctx;
	fz_try(ctx)
	{
		la_ssize_t got;
		size_t size;
		while (arch->current < idx)
		{
			int r = archive_read_next_header(arch->archive, &entry);
			if (!LA_OK(r) || !LA_OK(archive_read_data_skip(arch->archive)))
				fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot skip over a RAR entry");
			arch->current++;
		}
		if (!LA_OK(archive_read_next_header(arch->archive, &entry)))
			fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot read a RAR entry");
		arch->current++;
		size = arch->entries[idx].len;
		buf = fz_new_buffer(ctx, size ? size : 1);
		got = archive_read_data(arch->archive, buf->data, size);
		if (got < 0)
			fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot unpack a RAR entry: %s", archive_error_string(arch->archive));
		buf->len = (size_t)got;
	}
	fz_always(ctx)
		arch->ctx = NULL;
	fz_catch(ctx)
	{
		fz_drop_buffer(ctx, buf);
		/* The reader may be anywhere now: start over next time. */
		if (arch->archive)
			archive_read_free(arch->archive);
		arch->archive = NULL;
		fz_rethrow(ctx);
	}
	return buf;
}

static fz_buffer *read_rar_entry(fz_context *ctx, fz_archive *arch_, const char *name)
{
	rar_archive *arch = (rar_archive *)arch_;
	int idx = find_entry(arch, name);
	if (idx < 0)
		return NULL;
	if (arch->unpacked)
		return read_unpacked(ctx, arch, &arch->entries[idx]);
	return arch->v5 ? read_v5(ctx, arch, idx) : read_v4(ctx, arch, &arch->entries[idx]);
}

static fz_stream *open_rar_entry(fz_context *ctx, fz_archive *arch, const char *name)
{
	fz_buffer *buf = read_rar_entry(ctx, arch, name);
	fz_stream *stm = NULL;
	if (!buf)
		return NULL;
	fz_try(ctx)
		stm = fz_open_buffer(ctx, buf);
	fz_always(ctx)
		fz_drop_buffer(ctx, buf);
	fz_catch(ctx)
		fz_rethrow(ctx);
	return stm;
}

/* A temporary file for a solid archive's entries (none if there's nowhere to put it). */
static FILE *begin_unpacked(rar_archive *arch)
{
	char path[1024];
	FILE *f;
	if (!temp_dir)
		return NULL;
	snprintf(path, sizeof path, "%s/paginaut-rar-%p.tmp", temp_dir, (void *)arch);
	f = temp_open(path);
#ifndef _WIN32
	/* Nameless from now on: it goes when it's closed, or when the app ends however it ends. */
	if (f)
		remove(path);
#endif
	return f;
}

static rar_entry *add_entry(fz_context *ctx, rar_archive *arch, const char *name, size_t len)
{
	rar_entry *e;
	if (arch->count == arch->max)
	{
		arch->max = arch->max ? arch->max * 2 : 64;
		arch->entries = fz_realloc(ctx, arch->entries, sizeof *arch->entries * arch->max);
	}
	e = &arch->entries[arch->count++];
	e->name = fz_strdup(ctx, name);
	e->len = len;
	e->at = 0;
	e->offset = 0;
	return e;
}

/* Gives up the temporary file (out of space): pages come from the archive itself after all. */
static void drop_unpacked(rar_archive *arch)
{
	if (arch->unpacked)
		fclose(arch->unpacked);
	arch->unpacked = NULL;
}

/* RAR 4 through unarr: lists the entries, unpacking a solid archive's as it goes. */
static void list_v4(fz_context *ctx, rar_archive *arch, unsigned char *chunk, size_t chunk_size)
{
	int64_t at = 0;
	arch->ustream = ar_open_stream(arch, ua_close, ua_read, ua_seek, ua_tell);
	if (!arch->ustream)
		fz_throw(ctx, FZ_ERROR_SYSTEM, "out of memory");
	fz_seek(ctx, arch->super.file, 0, SEEK_SET);
	arch->ctx = ctx;
	arch->uarch = ar_open_rar_archive(arch->ustream);
	if (!arch->uarch)
		fz_throw(ctx, FZ_ERROR_FORMAT, "cannot open the RAR archive");
	while (ar_parse_entry(arch->uarch))
	{
		const char *name = ar_entry_get_name(arch->uarch);
		rar_entry *e = add_entry(ctx, arch, name ? name : "", ar_entry_get_size(arch->uarch));
		e->offset = ar_entry_get_offset(arch->uarch);
		if (arch->unpacked && *e->name)
		{
			int r = ua_unpack(arch, e->len, NULL, arch->unpacked, chunk, chunk_size);
			if (r < 0)
				fz_throw(ctx, FZ_ERROR_FORMAT, "cannot unpack a RAR entry");
			if (r > 0)
				drop_unpacked(arch);
			e->at = at;
			at += (int64_t)e->len;
		}
	}
	if (!ar_at_eof(arch->uarch))
		fz_throw(ctx, FZ_ERROR_FORMAT, "damaged RAR archive");
	arch->ctx = NULL;
}

/* RAR 5 through libarchive: lists the entries, unpacking a solid archive's as it goes. */
static void list_v5(fz_context *ctx, rar_archive *arch, unsigned char *chunk, size_t chunk_size)
{
	int64_t at = 0;
	if (la_begin(ctx, arch))
		fz_throw(ctx, FZ_ERROR_FORMAT, "cannot open the RAR archive");
	arch->ctx = ctx;
	for (;;)
	{
		struct archive_entry *entry;
		const char *path;
		rar_entry *e;
		int r = archive_read_next_header(arch->archive, &entry);
		if (r == ARCHIVE_EOF)
			break;
		if (!LA_OK(r))
			fz_throw(ctx, FZ_ERROR_FORMAT, "damaged RAR archive: %s", archive_error_string(arch->archive));
		path = archive_entry_pathname_utf8(entry);
		if (!path)
			path = archive_entry_pathname(entry);
		/* Directories are entries to libarchive too: listed nameless, to keep positions in step. */
		if (!path || archive_entry_filetype(entry) == AE_IFDIR)
			path = "";
		e = add_entry(ctx, arch, path, *path ? (size_t)archive_entry_size(entry) : 0);
		if (arch->unpacked && *path)
		{
			la_ssize_t got;
			size_t len = 0;
			while ((got = archive_read_data(arch->archive, chunk, chunk_size)) > 0)
			{
				if (fwrite(chunk, 1, (size_t)got, arch->unpacked) != (size_t)got)
					break;
				len += (size_t)got;
			}
			if (got < 0)
				fz_throw(ctx, FZ_ERROR_FORMAT, "cannot unpack a RAR entry: %s", archive_error_string(arch->archive));
			if (got > 0)
			{
				drop_unpacked(arch);
				continue;
			}
			e->len = len;
			e->at = at;
			at += (int64_t)len;
		}
	}
	arch->ctx = NULL;
	/* Read from the start again when a page is wanted (never, when it's all unpacked). */
	archive_read_free(arch->archive);
	arch->archive = NULL;
}

static fz_archive *open_rar(fz_context *ctx, fz_stream *file)
{
	rar_archive *arch = fz_new_derived_archive(ctx, file, rar_archive);
	unsigned char *chunk = NULL;
	const size_t chunk_size = 1 << 16;
	unsigned char sig[8] = { 0 };
	arch->super.format = "rar";
	arch->super.count_entries = count_rar_entries;
	arch->super.list_entry = list_rar_entry;
	arch->super.has_entry = has_rar_entry;
	arch->super.read_entry = read_rar_entry;
	arch->super.open_entry = open_rar_entry;
	arch->super.drop_archive = drop_rar;
	fz_var(chunk);
	fz_try(ctx)
	{
		fz_seek(ctx, file, 0, SEEK_SET);
		fz_read(ctx, file, sig, 8);
		arch->v5 = sig[6] == 1;
		if (rar_is_solid(ctx, file))
			arch->unpacked = begin_unpacked(arch);
		if (arch->unpacked)
			chunk = fz_malloc(ctx, chunk_size);
		if (arch->v5)
			list_v5(ctx, arch, chunk, chunk_size);
		else
			list_v4(ctx, arch, chunk, chunk_size);
		if (arch->unpacked)
			fflush(arch->unpacked);
	}
	fz_always(ctx)
		fz_free(ctx, chunk);
	fz_catch(ctx)
	{
		arch->ctx = NULL;
		fz_drop_archive(ctx, &arch->super);
		fz_rethrow(ctx);
	}
	return &arch->super;
}

static const fz_archive_handler rar_handler = { recognize_rar, open_rar };

/* Called once each new MuPDF context has its document handlers. */
void po_register_rar(fz_context *ctx)
{
	fz_register_archive_handler(ctx, &rar_handler);
}

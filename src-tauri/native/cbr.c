/*
 * RAR archives for MuPDF, so comic books packed with RAR (CBR) open like CBZ ones: an archive
 * handler on libarchive's RAR and RAR5 readers (vendored in native/libarchive, only those
 * formats). After MuPDF's own libarchive handler (source/fitz/unlibarchive.c), which MuPDF's
 * build leaves out; this one has its own names, as MuPDF's libraries carry stubs of those.
 */
#include <mupdf/fitz.h>
#include <string.h>

#include "libarchive/archive.h"
#include "libarchive/archive_entry.h"

typedef struct
{
	size_t len;
	char *name;
} rar_entry;

typedef struct
{
	fz_archive super;
	struct archive *archive;
	/* Entries read past so far (libarchive reads forwards only). */
	int current;
	int count, max;
	rar_entry *entries;
	fz_context *ctx; /* while libarchive calls back */
	uint8_t block[4096];
} rar_archive;

static la_ssize_t rar_read(struct archive *a, void *data, const void **buf)
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

static la_int64_t rar_skip(struct archive *a, void *data, la_int64_t skip)
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

static la_int64_t rar_seek(struct archive *a, void *data, la_int64_t offset, int whence)
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

static int rar_close(struct archive *a, void *data)
{
	return ARCHIVE_OK; /* the stream goes with the fz_archive */
}

/* Starts reading from the beginning; nonzero on failure. */
static int rar_begin(fz_context *ctx, rar_archive *arch)
{
	int r;
	if (arch->archive)
		archive_read_free(arch->archive);
	fz_seek(ctx, arch->super.file, 0, SEEK_SET);
	arch->archive = archive_read_new();
	archive_read_support_format_rar(arch->archive);
	archive_read_support_format_rar5(arch->archive);
	arch->ctx = ctx;
	r = archive_read_set_seek_callback(arch->archive, rar_seek);
	if (r == ARCHIVE_OK)
		r = archive_read_open2(arch->archive, arch, NULL, rar_read, rar_skip, rar_close);
	arch->ctx = NULL;
	if (r != ARCHIVE_OK)
	{
		archive_read_free(arch->archive);
		arch->archive = NULL;
	}
	arch->current = 0;
	return r != ARCHIVE_OK;
}

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

static void drop_rar(fz_context *ctx, fz_archive *arch_)
{
	rar_archive *arch = (rar_archive *)arch_;
	int i;
	if (arch->archive)
		archive_read_free(arch->archive);
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

static fz_buffer *read_rar_entry(fz_context *ctx, fz_archive *arch_, const char *name)
{
	rar_archive *arch = (rar_archive *)arch_;
	struct archive_entry *entry;
	fz_buffer *buf = NULL;
	int idx = find_entry(arch, name);
	if (idx < 0)
		return NULL;
	/* Pages are mostly read in order; going back means reading from the start again. */
	if (!arch->archive || arch->current > idx)
		if (rar_begin(ctx, arch))
			fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot reopen the RAR archive");
	fz_var(buf);
	arch->ctx = ctx;
	fz_try(ctx)
	{
		la_ssize_t got;
		size_t size;
		while (arch->current < idx)
		{
			if (archive_read_next_header(arch->archive, &entry) != ARCHIVE_OK || archive_read_data_skip(arch->archive) != ARCHIVE_OK)
				fz_throw(ctx, FZ_ERROR_LIBRARY, "cannot skip over a RAR entry");
			arch->current++;
		}
		if (archive_read_next_header(arch->archive, &entry) != ARCHIVE_OK)
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

static fz_archive *open_rar(fz_context *ctx, fz_stream *file)
{
	rar_archive *arch = fz_new_derived_archive(ctx, file, rar_archive);
	arch->super.format = "rar";
	arch->super.count_entries = count_rar_entries;
	arch->super.list_entry = list_rar_entry;
	arch->super.has_entry = has_rar_entry;
	arch->super.read_entry = read_rar_entry;
	arch->super.open_entry = open_rar_entry;
	arch->super.drop_archive = drop_rar;
	fz_try(ctx)
	{
		if (rar_begin(ctx, arch))
			fz_throw(ctx, FZ_ERROR_FORMAT, "cannot open the RAR archive");
		/* List the entries (their names as UTF-8). */
		arch->ctx = ctx;
		for (;;)
		{
			struct archive_entry *entry;
			const char *path;
			int r = archive_read_next_header(arch->archive, &entry);
			if (r == ARCHIVE_EOF)
				break;
			if (r != ARCHIVE_OK && r != ARCHIVE_WARN)
				fz_throw(ctx, FZ_ERROR_FORMAT, "damaged RAR archive: %s", archive_error_string(arch->archive));
			path = archive_entry_pathname_utf8(entry);
			if (!path)
				path = archive_entry_pathname(entry);
			/* Directories are entries to libarchive too: listed nameless, to keep positions in step. */
			if (!path || archive_entry_filetype(entry) == AE_IFDIR)
				path = "";
			if (arch->count == arch->max)
			{
				arch->max = arch->max ? arch->max * 2 : 64;
				arch->entries = fz_realloc(ctx, arch->entries, sizeof *arch->entries * arch->max);
			}
			arch->entries[arch->count].name = fz_strdup(ctx, path);
			arch->entries[arch->count].len = *path ? (size_t)archive_entry_size(entry) : 0;
			arch->count++;
		}
		arch->ctx = NULL;
		if (rar_begin(ctx, arch))
			fz_throw(ctx, FZ_ERROR_FORMAT, "cannot reopen the RAR archive");
	}
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

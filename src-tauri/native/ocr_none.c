/* The Tesseract bridge for builds without OCR (the Android viewer): no recogniser can be made. */
#include "ocr.h"
#include <mupdf/fitz.h>
#include <stdio.h>

/* Only in MuPDF builds with Tesseract; fo_ocr_init calls it. */
void fz_set_leptonica_mem(fz_context *ctx);
void fz_set_leptonica_mem(fz_context *ctx)
{
	(void)ctx;
}

po_tess *po_tess_new(const char *langs, const char *files, char *err, int errlen)
{
	(void)langs;
	(void)files;
	if (err && errlen > 0)
		snprintf(err, (size_t)errlen, "Text recognition isn't part of this build.");
	return NULL;
}

void po_tess_drop(po_tess *t)
{
	(void)t;
}

int po_tess_recognize(po_tess *t, const unsigned char *gray, int w, int h, int stride, int dpi,
	po_word_fn word, void *arg, volatile int *abort_flag)
{
	(void)t; (void)gray; (void)w; (void)h; (void)stride; (void)dpi; (void)word; (void)arg; (void)abort_flag;
	return -1;
}
